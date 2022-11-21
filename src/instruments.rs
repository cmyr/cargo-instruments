//! interfacing with the `instruments` command line tool

use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{anyhow, Result};
use cargo::core::Workspace;
use semver::Version;

use crate::opt::AppConfig;

/// Holds available templates.
pub struct TemplateCatalog {
    standard_templates: Vec<String>,
    custom_templates: Vec<String>,
}

/// Represents the Xcode Instrument version detected.
pub enum XcodeInstruments {
    XcTrace,
    InstrumentsBinary,
}

impl XcodeInstruments {
    /// Detects which version of Xcode Instruments is installed and if it can be launched.
    pub(crate) fn detect() -> Result<XcodeInstruments> {
        let cur_version = get_macos_version()?;
        let macos_xctrace_version = Version::parse("10.15.0").unwrap();

        if cur_version >= macos_xctrace_version {
            // This is the check used by Homebrew,see
            // https://github.com/Homebrew/install/blob/a1d820fc8950312c35073700d0ea88a531bc5950/install.sh#L216
            let clt_git_filepath = Path::new("/Library/Developer/CommandLineTools/usr/bin/git");
            if clt_git_filepath.exists() {
                return Ok(XcodeInstruments::XcTrace);
            }
        } else {
            let instruments_app_filepath = Path::new("/usr/bin/instruments");
            if instruments_app_filepath.exists() {
                return Ok(XcodeInstruments::InstrumentsBinary);
            }
        }
        Err(anyhow!(
            "Xcode Instruments is not installed. Please install the Xcode Command Line Tools."
        ))
    }

    /// Return a catalog of available Instruments Templates.
    ///
    /// The custom templates only appears if you have custom templates.
    pub(crate) fn available_templates(&self) -> Result<TemplateCatalog> {
        match self {
            XcodeInstruments::XcTrace => parse_xctrace_template_list(),
            XcodeInstruments::InstrumentsBinary => parse_instruments_template_list(),
        }
    }

    /// Prepare the Xcode Instruments profiling command
    ///
    /// If the `xctrace` tool is used, the prepared command looks like
    ///
    /// ```sh
    /// xcrun xctrace record --template MyTemplate \
    ///                      --time-limit 5000ms \
    ///                      --output path/to/tracefile \
    ///                      --launch \
    ///                      --
    /// ```
    ///
    /// If the older `instruments` tool is used, the prepared command looks
    /// like
    ///
    /// ```sh
    /// instruments -t MyTemplate \
    ///             -D /path/to/tracefile \
    ///             -l 5000ms
    /// ```
    fn profiling_command(
        &self,
        template_name: &str,
        trace_filepath: &Path,
        time_limit: Option<usize>,
    ) -> Result<Command> {
        match self {
            XcodeInstruments::XcTrace => {
                let mut command = Command::new("xcrun");
                command.args(["xctrace", "record"]);

                command.args(["--template", template_name]);

                if let Some(limit_millis) = time_limit {
                    let limit_millis_str = format!("{}ms", limit_millis);
                    command.args(["--time-limit", &limit_millis_str]);
                }

                command.args(["--output", trace_filepath.to_str().unwrap()]);
                // redirect stdin & err to the user's terminal
                if let Some(tty) = get_tty()? {
                    command.args(["--target-stdin", &tty, "--target-stdout", &tty]);
                }

                command.args(["--launch", "--"]);
                Ok(command)
            }
            XcodeInstruments::InstrumentsBinary => {
                let mut command = Command::new("instruments");
                command.args(["-t", template_name]);

                command.arg("-D").arg(trace_filepath);

                if let Some(limit) = time_limit {
                    command.args(["-l", &limit.to_string()]);
                }
                Ok(command)
            }
        }
    }
}

/// Return the macOS version.
///
/// This function parses the output of `sw_vers -productVersion` (a string like '11.2.3`)
/// and returns the corresponding semver struct `Version{major: 11, minor: 2, patch: 3}`.
fn get_macos_version() -> Result<Version> {
    let Output { status, stdout, .. } =
        Command::new("sw_vers").args(["-productVersion"]).output()?;

    if !status.success() {
        return Err(anyhow!("macOS version cannot be determined"));
    }

    semver_from_utf8(&stdout)
}

/// Returns a semver given a slice of bytes
///
/// This function tries to construct a semver struct given a raw utf8 byte array
/// that may not contain a patch number, `"11.1"` is parsed as `"11.1.0"`.
fn semver_from_utf8(version: &[u8]) -> Result<Version> {
    let to_semver = |version_string: &str| {
        Version::parse(version_string).map_err(|error| {
            anyhow!("cannot parse version: `{}`, because of {}", version_string, error)
        })
    };

    let version_string = std::str::from_utf8(version)?;
    match version_string.split('.').count() {
        1 => to_semver(&format!("{}.0.0", version_string.trim())),
        2 => to_semver(&format!("{}.0", version_string.trim())),
        3 => to_semver(version_string.trim()),
        _ => Err(anyhow!("invalid version: {}", version_string)),
    }
}

/// Parse xctrace template listing.
///
/// Xctrace prints the list on either stderr (older versions) or stdout (recent).
/// In either case, the expected output is:
///
/// ```
/// == Standard Templates ==
/// Activity Monitor
/// Allocations
/// Animation Hitches
/// App Launch
/// Core Data
/// Counters
/// Energy Log
/// File Activity
/// Game Performance
/// Leaks
/// Logging
/// Metal System Trace
/// Network
/// SceneKit
/// SwiftUI
/// System Trace
/// Time Profiler
/// Zombies
///
/// == Custom Templates ==
/// MyTemplate
/// ```
fn parse_xctrace_template_list() -> Result<TemplateCatalog> {
    let Output { status, stdout, stderr } =
        Command::new("xcrun").args(["xctrace", "list", "templates"]).output()?;

    if !status.success() {
        return Err(anyhow!(
            "Could not list templates. Please check your Xcode Instruments installation."
        ));
    }

    // Some older versions of xctrace print results on stderr,
    // newer version print results on stdout.
    let output = if stdout.is_empty() { stderr } else { stdout };

    let templates_str = std::str::from_utf8(&output)?;
    let mut templates_iter = templates_str.lines();

    let standard_templates = templates_iter
        .by_ref()
        .skip(1)
        .map(|line| line.trim())
        .take_while(|line| !line.starts_with('=') && !line.is_empty())
        .map(|line| line.into())
        .collect::<Vec<_>>();

    if standard_templates.is_empty() {
        return Err(anyhow!(
            "No available templates. Please check your Xcode Instruments installation."
        ));
    }

    let custom_templates = templates_iter
        .map(|line| line.trim())
        .skip_while(|line| line.starts_with('=') || line.is_empty())
        .map(|line| line.into())
        .collect::<Vec<_>>();

    Ok(TemplateCatalog { standard_templates, custom_templates })
}

/// Parse /usr/bin/instruments template list.
///
/// The expected output on stdout is:
///
/// ```
/// Known Templates:
/// "Activity Monitor"
/// "Allocations"
/// "Animation Hitches"
/// "App Launch"
/// "Blank"
/// "Core Data"
/// "Counters"
/// "Energy Log"
/// "File Activity"
/// "Game Performance"
/// "Leaks"
/// "Logging"
/// "Metal System Trace"
/// "Network"
/// "SceneKit"
/// "SwiftUI"
/// "System Trace"
/// "Time Profiler"
/// "Zombies"
/// "~/Library/Application Support/Instruments/Templates/MyTemplate.tracetemplate"
/// ```
fn parse_instruments_template_list() -> Result<TemplateCatalog> {
    let Output { status, stdout, .. } =
        Command::new("instruments").args(["-s", "templates"]).output()?;

    if !status.success() {
        return Err(anyhow!(
            "Could not list templates. Please check your Xcode Instruments installation."
        ));
    }

    let templates_str = std::str::from_utf8(&stdout)?;

    let standard_templates = templates_str
        .lines()
        .skip(1)
        .map(|line| line.trim().trim_matches('"'))
        .take_while(|line| !line.starts_with("~/Library/"))
        .map(|line| line.into())
        .collect::<Vec<_>>();

    if standard_templates.is_empty() {
        return Err(anyhow!(
            "No available templates. Please check your Xcode Instruments installation."
        ));
    }

    let custom_templates = templates_str
        .lines()
        .map(|line| line.trim().trim_matches('"'))
        .skip_while(|line| !line.starts_with("~/Library/"))
        .take_while(|line| !line.is_empty())
        .map(|line| Path::new(line).file_stem().unwrap().to_string_lossy())
        .map(|line| line.into())
        .collect::<Vec<_>>();

    Ok(TemplateCatalog { standard_templates, custom_templates })
}

/// Render the template catalog content as a string.
///
/// The returned string is similar to
///
/// ```text
/// Xcode Instruments templates:
///
/// built-in            abbrev
/// --------------------------
/// Activity Monitor
/// Allocations         (alloc)
/// Animation Hitches
/// App Launch
/// Core Data
/// Counters
/// Energy Log
/// File Activity       (io)
/// Game Performance
/// Leaks
/// Logging
/// Metal System Trace
/// Network
/// SceneKit
/// SwiftUI
/// System Trace        (sys)
/// Time Profiler       (time)
/// Zombies
///
/// custom
/// --------------------------
/// MyTemplate
/// ```
pub fn render_template_catalog(catalog: &TemplateCatalog) -> String {
    let mut output: String = "Xcode Instruments templates:\n".into();

    let max_width = catalog
        .standard_templates
        .iter()
        .chain(catalog.custom_templates.iter())
        .map(|name| name.len())
        .max()
        .unwrap();

    // column headers
    write!(&mut output, "\n{:width$}abbrev", "built-in", width = max_width + 2).unwrap();
    write!(&mut output, "\n{:-<width$}", "", width = max_width + 8).unwrap();

    for name in &catalog.standard_templates {
        output.push('\n');
        if let Some(abbrv) = abbrev_name(name.trim_matches('"')) {
            write!(&mut output, "{:width$}({abbrev})", name, width = max_width + 2, abbrev = abbrv)
                .unwrap();
        } else {
            output.push_str(name);
        }
    }

    output.push('\n');

    // column headers
    write!(&mut output, "\n{:width$}", "custom", width = max_width + 2).unwrap();
    write!(&mut output, "\n{:-<width$}", "", width = max_width + 8).unwrap();

    for name in &catalog.custom_templates {
        output.push('\n');
        output.push_str(name);
    }

    output.push('\n');

    output
}

/// Compute the tracefile output path, creating the directory structure
/// in `target/instruments` if needed.
fn prepare_trace_filepath(
    target_filepath: &Path,
    template_name: &str,
    app_config: &AppConfig,
    workspace_root: &Path,
) -> Result<PathBuf> {
    if let Some(ref path) = app_config.trace_filepath {
        return Ok(path.to_path_buf());
    }

    let trace_dir = workspace_root.join("target").join("instruments");

    if !trace_dir.exists() {
        fs::create_dir_all(&trace_dir)
            .map_err(|e| anyhow!("failed to create {:?}: {}", &trace_dir, e))?;
    }

    let trace_filename = {
        let target_shortname = target_filepath
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("invalid target path {:?}", target_filepath))?;
        let template_name = template_name.replace(' ', "-");
        let now = chrono::Local::now();

        format!("{}_{}_{}.trace", target_shortname, template_name, now.format("%F_%H%M%S-%3f"))
    };

    let trace_filepath = trace_dir.join(trace_filename);

    Ok(trace_filepath)
}

/// Return the complete template name, replacing abbreviation if provided.
fn resolve_template_name(template_name: &str) -> &str {
    match template_name {
        "time" => "Time Profiler",
        "alloc" => "Allocations",
        "io" => "File Activity",
        "sys" => "System Trace",
        other => other,
    }
}

/// Return the template name abbreviation if available.
fn abbrev_name(template_name: &str) -> Option<&str> {
    match template_name {
        "Time Profiler" => Some("time"),
        "Allocations" => Some("alloc"),
        "File Activity" => Some("io"),
        "System Trace" => Some("sys"),
        _ => None,
    }
}

/// Profile the target binary at `binary_filepath`, write results at
/// `trace_filepath` and returns its path.
pub(crate) fn profile_target(
    target_filepath: &Path,
    xctrace_tool: &XcodeInstruments,
    app_config: &AppConfig,
    workspace: &Workspace,
) -> Result<PathBuf> {
    // 1. Get the template name from config
    // This borrows a ref to the String in Option<String>. The value can be
    // unwrapped because in this version the template was checked earlier to
    // be a `Some(x)`.
    let template_name = resolve_template_name(app_config.template_name.as_deref().unwrap());

    // 2. Compute the trace filepath and create its parent directory
    let workspace_root = workspace.root().to_path_buf();
    let trace_filepath = prepare_trace_filepath(
        target_filepath,
        template_name,
        app_config,
        workspace_root.as_path(),
    )?;

    // 3. Print current activity `Profiling target/debug/tries`
    {
        let target_shortpath = target_filepath
            .strip_prefix(workspace_root)
            .unwrap_or(target_filepath)
            .to_string_lossy();
        let status_detail = format!("{} with template '{}'", target_shortpath, template_name);
        workspace.config().shell().status("Profiling", status_detail)?;
    }

    let mut command =
        xctrace_tool.profiling_command(template_name, &trace_filepath, app_config.time_limit)?;

    command.arg(target_filepath);

    if !app_config.target_args.is_empty() {
        command.args(app_config.target_args.as_slice());
    }

    let output = command.output()?;

    if !output.status.success() {
        let stderr =
            String::from_utf8(output.stderr).unwrap_or_else(|_| "failed to capture stderr".into());
        let stdout =
            String::from_utf8(output.stdout).unwrap_or_else(|_| "failed to capture stdout".into());
        return Err(anyhow!("instruments errored: {} {}", stderr, stdout));
    }

    Ok(trace_filepath)
}

/// get the tty of th current terminal session
fn get_tty() -> Result<Option<String>> {
    let mut command = Command::new("ps");
    command.arg("otty=").arg(std::process::id().to_string());
    Ok(String::from_utf8(command.output()?.stdout)?
        .split_whitespace()
        .next()
        .map(|tty| format!("/dev/{}", tty)))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn semvers_can_be_parsed() {
        assert_eq!(semver_from_utf8(b"2.3.4").unwrap(), Version::parse("2.3.4").unwrap());
        assert_eq!(semver_from_utf8(b"11.1").unwrap(), Version::parse("11.1.0").unwrap());
        assert_eq!(semver_from_utf8(b"11").unwrap(), Version::parse("11.0.0").unwrap());
    }
}
