//! The main application logic.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Result};
use cargo::{
    core::Workspace,
    ops::CompileOptions,
    util::{config::Config, important_paths, interning::InternedString},
};
use termcolor::Color;

use crate::instruments;
use crate::opt::{AppConfig, CargoOpts, Target};

/// Main entrance point, after args have been parsed.
pub(crate) fn run(mut app_config: AppConfig) -> Result<()> {
    // 1. Detect the type of Xcode Instruments installation
    let xctrace_tool = instruments::XcodeInstruments::detect()?;

    // 2. Render available templates if the user asked
    if app_config.list_templates {
        let catalog = xctrace_tool.available_templates()?;
        println!("{}", instruments::render_template_catalog(&catalog));
        return Ok(());
    }

    // 3. Build the specified target
    let cargo_config = Config::default()?;

    let manifest_path = match app_config.manifest_path.as_ref() {
        Some(path) if path.is_absolute() => Ok(path.to_owned()),
        Some(path) => Ok(cargo_config.cwd().join(path)),
        None => important_paths::find_root_manifest_for_wd(cargo_config.cwd()),
    }?;

    let workspace = Workspace::new(&manifest_path, &cargo_config)?;

    // 3.1: warn if --open passed. We do this here so we have access to cargo's
    // pretty-printer
    if app_config.open {
        workspace.config().shell().status_with_color(
            "Warning",
            "--open is now the default behaviour, and will be ignored.",
            Color::Yellow,
        )?;
    }

    let cargo_options = app_config.to_cargo_opts()?;
    let target_filepath = match build_target(&cargo_options, &workspace) {
        Ok(path) => path,
        Err(e) => {
            workspace.config().shell().status_with_color("Failed", &e, Color::Red)?;
            return Err(e);
        }
    };

    #[cfg(target_arch = "aarch64")]
    codesign(&target_filepath, &workspace)?;

    if let Target::Test(_, ref tests) = cargo_options.target {
        app_config.target_args.insert(0, tests.clone());
    }

    // 4. Profile the built target, will display menu if no template was selected
    let trace_filepath =
        match instruments::profile_target(&target_filepath, &xctrace_tool, &app_config, &workspace)
        {
            Ok(path) => path,
            Err(e) => {
                workspace.config().shell().status_with_color("Failed", &e, Color::Red)?;
                return Ok(());
            }
        };

    // 5. Print the trace file's relative path
    {
        let trace_shortpath = trace_filepath
            .strip_prefix(workspace.root().as_os_str())
            .unwrap_or(trace_filepath.as_path())
            .to_string_lossy();
        workspace.config().shell().status("Trace file", trace_shortpath)?;
    }

    // 6. Open Xcode Instruments if asked
    if !app_config.no_open {
        launch_instruments(&trace_filepath)?;
    }

    Ok(())
}

/// On M1 we need to resign with the specified entitlement.
///
/// See https://github.com/cmyr/cargo-instruments/issues/40#issuecomment-894287229
/// for more information.
#[cfg(target_arch = "aarch64")]
fn codesign(path: &Path, workspace: &Workspace) -> Result<()> {
    use std::fmt::Write;

    static ENTITLEMENTS_FILENAME: &str = "entitlements.plist";
    static ENTITLEMENTS_PLIST_DATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
        <key>com.apple.security.get-task-allow</key>
        <true/>
    </dict>
</plist>"#;

    let target_dir = path.parent().ok_or_else(|| anyhow!("failed to get target directory"))?;
    let entitlement_path = target_dir.join(ENTITLEMENTS_FILENAME);
    std::fs::write(&entitlement_path, ENTITLEMENTS_PLIST_DATA.as_bytes())?;

    let output = Command::new("codesign")
        .args(["-s", "-", "-f", "--entitlements"])
        .args([&entitlement_path, path])
        .output()?;
    if !output.status.success() {
        let mut msg = String::new();
        if !output.stdout.is_empty() {
            msg = format!("stdout: \"{}\"", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            if !msg.is_empty() {
                msg.push('\n');
            }
            write!(&mut msg, "stderr: \"{}\"", String::from_utf8_lossy(&output.stderr))?;
        }

        workspace.config().shell().status_with_color("Code signing failed", msg, Color::Red)?;
    }
    Ok(())
}

/// Attempts to validate and build the specified target. On success, returns
/// the path to the built executable.
fn build_target(cargo_options: &CargoOpts, workspace: &Workspace) -> Result<PathBuf> {
    use cargo::core::shell::Verbosity;
    workspace.config().shell().set_verbosity(Verbosity::Normal);

    let compile_options = make_compile_opts(cargo_options, workspace.config())?;
    let result = cargo::ops::compile(workspace, &compile_options)?;

    if let Target::Bench(ref bench) = cargo_options.target {
        result
            .tests
            .iter()
            .find(|unit_output| unit_output.unit.target.name() == bench)
            .map(|unit_output| unit_output.path.clone())
            .ok_or_else(|| anyhow!("no benchmark '{}'", bench))
    } else if let Target::Test(ref harness, _) = cargo_options.target {
        result
            .tests
            .iter()
            .find(|unit_output| unit_output.unit.target.name() == harness)
            .map(|unit_output| unit_output.path.clone())
            .ok_or_else(|| anyhow!("no test '{}'", harness))
    } else {
        match result.binaries.as_slice() {
            [unit_output] => Ok(unit_output.path.clone()),
            [] => Err(anyhow!("no targets found")),
            other => Err(anyhow!(
                "found multiple targets: {:?}",
                other
                    .iter()
                    .map(|unit_output| unit_output.unit.target.name())
                    .collect::<Vec<&str>>()
            )),
        }
    }
}

/// Generate `CompileOptions` for Cargo.
///
/// This additionally filters options based on user args, so that Cargo
/// builds as little as possible.
fn make_compile_opts(cargo_options: &CargoOpts, cfg: &Config) -> Result<CompileOptions> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileFilter;

    let mut compile_options = CompileOptions::new(cfg, CompileMode::Build)?;
    let profile = &cargo_options.profile;

    compile_options.build_config.requested_profile = InternedString::new(profile);
    compile_options.cli_features = cargo_options.features.clone();
    compile_options.spec = cargo_options.package.clone().into();

    if cargo_options.target != Target::Main {
        let (bins, examples, benches, _tests) = match &cargo_options.target {
            Target::Bin(bin) => (vec![bin.clone()], vec![], vec![], vec![]),
            Target::Example(bin) => (vec![], vec![bin.clone()], vec![], vec![]),
            Target::Bench(bin) => (vec![], vec![], vec![bin.clone()], vec![]),
            Target::Test(bin, _test) => (vec![], vec![], vec![], vec![bin.clone()]),
            _ => unreachable!(),
        };

        compile_options.filter = CompileFilter::from_raw_arguments(
            false,
            bins,
            false,
            vec![],
            true,
            examples,
            false,
            benches,
            false,
            false,
        );
    }
    Ok(compile_options)
}

/// Launch Xcode Instruments on the provided trace file.
fn launch_instruments(trace_filepath: &Path) -> Result<()> {
    let status = Command::new("open").arg(trace_filepath).status()?;

    if !status.success() {
        return Err(anyhow!("open failed"));
    }
    Ok(())
}
