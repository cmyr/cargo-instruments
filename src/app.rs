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
pub(crate) fn run(app_config: AppConfig) -> Result<()> {
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
    let manifest_path = important_paths::find_root_manifest_for_wd(cargo_config.cwd())?;
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

    let cargo_options = app_config.to_cargo_opts();
    let target_filepath = match build_target(&cargo_options, &workspace) {
        Ok(path) => path,
        Err(e) => {
            workspace.config().shell().status_with_color("Failed", &e, Color::Red)?;
            return Err(e);
        }
    };

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
            .unwrap_or_else(|_| trace_filepath.as_path())
            .to_string_lossy();
        workspace.config().shell().status("Trace file", trace_shortpath)?;
    }

    // 6. Open Xcode Instruments if asked
    if !app_config.no_open {
        launch_instruments(&trace_filepath)?;
    }

    Ok(())
}

/// Attempts to validate and build the specified target. On success, returns
/// the path to the built executable.
fn build_target(cargo_options: &CargoOpts, workspace: &Workspace) -> Result<PathBuf> {
    use cargo::core::shell::Verbosity;
    workspace.config().shell().set_verbosity(Verbosity::Normal);

    validate_target(&cargo_options.target, &workspace)?;

    let compile_options = make_compile_opts(&cargo_options, workspace.config())?;
    let result = cargo::ops::compile(workspace, &compile_options)?;

    if let Target::Bench(ref bench) = cargo_options.target {
        result
            .tests
            .iter()
            .find(|unit_output| unit_output.unit.target.name() == bench)
            .map(|unit_output| unit_output.path.clone())
            .ok_or_else(|| anyhow!("no benchmark '{}'", bench))
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

/// Validate that the target can be built.
///
/// This searches the workspace for the provided target, returning an Error if
/// it can't be found.
fn validate_target(target: &Target, workspace: &Workspace) -> Result<()> {
    let package = workspace.current()?;
    let mut targets = package.targets().iter();

    let has_target = match target {
        Target::Main => targets.any(|t| t.is_bin()),
        Target::Bin(name) => targets.any(|t| t.is_bin() && t.name() == name),
        Target::Example(name) => targets.any(|t| t.is_example() && t.name() == name),
        Target::Bench(name) => targets.any(|t| t.is_bench() && t.name() == name),
    };

    if !has_target {
        return Err(anyhow!("missing target {}", target));
    }

    Ok(())
}

/// Generate `CompileOptions` for Cargo.
///
/// This additionally filters options based on user args, so that Cargo
/// builds as little as possible.
fn make_compile_opts(cargo_options: &CargoOpts, cfg: &Config) -> Result<CompileOptions> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileFilter;

    let mut compile_options = CompileOptions::new(cfg, CompileMode::Build)?;
    let profile = if cargo_options.release { "release" } else { "dev" };

    compile_options.build_config.requested_profile = InternedString::new(profile);
    compile_options.features = cargo_options.features.clone();

    if cargo_options.target != Target::Main {
        let (bins, examples, benches) = match &cargo_options.target {
            Target::Bin(bin) => (vec![bin.clone()], vec![], vec![]),
            Target::Example(bin) => (vec![], vec![bin.clone()], vec![]),
            Target::Bench(bin) => (vec![], vec![], vec![bin.clone()]),
            _ => unreachable!(),
        };

        compile_options.filter = CompileFilter::from_raw_arguments(
            false,
            bins,
            false,
            Vec::new(),
            false,
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
