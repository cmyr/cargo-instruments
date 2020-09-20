//! The main application logic.

use std::path::PathBuf;
use std::process::Command;

use cargo::core::Workspace;
use cargo::ops::CompileOptions;
use cargo::util::config::Config;
use cargo::util::interning::InternedString;
use anyhow::{Result, anyhow};
use termcolor::Color;

use crate::instruments;
use crate::opt::{CargoOpts, Opts, Target};

/// The main entrance point, once args have been parsed.
pub(crate) fn run(args: Opts) -> Result<()> {
    use cargo::util::important_paths::find_root_manifest_for_wd;
    instruments::check_existence()?;

    if args.list {
        let list = instruments::list()?;
        println!("{}", list);
        return Ok(());
    }

    let cfg = Config::default()?;
    let manifest_path = find_root_manifest_for_wd(cfg.cwd())?;
    let workspace = Workspace::new(&manifest_path, &cfg)?;
    let workspace_root = manifest_path.parent().unwrap().to_owned();

    let exec_path = match build_target(&args, &workspace) {
        Ok(path) => path,
        Err(e) => {
            workspace.config().shell().status_with_color("Failed", &e, Color::Red)?;
            return Ok(());
        }
    };

    let relpath = exec_path.strip_prefix(&workspace_root).unwrap_or_else(|_| exec_path.as_path());
    workspace.config().shell().status("Profiling", relpath.to_string_lossy())?;

    let trace_path = match instruments::run(&args, exec_path, &workspace_root) {
        Ok(path) => path,
        Err(e) => {
            workspace.config().shell().status_with_color("Failed", &e, Color::Red)?;
            return Ok(());
        }
    };

    let reltrace =
        trace_path.strip_prefix(&workspace_root).unwrap_or_else(|_| trace_path.as_path());
    workspace.config().shell().status("Wrote Trace", reltrace.to_string_lossy())?;
    if args.open {
        workspace.config().shell().status("Opening", reltrace.to_string_lossy())?;
        open_file(&trace_path)?;
    }
    Ok(())
}

/// Attempts to build the specified target. On success, returns the path to
/// the built executable.
fn build_target(args: &Opts, workspace: &Workspace) -> Result<PathBuf> {
    use cargo::core::shell::Verbosity;
    workspace.config().shell().set_verbosity(Verbosity::Normal);

    let cargo_args = args.to_cargo_opts();

    validate_target(&cargo_args.target, workspace)?;

    let opts = make_compile_opts(&cargo_args, workspace.config())?;

    let result = cargo::ops::compile(workspace, &opts)?;
    if let Target::Bench(bench) = cargo_args.target {
        result
            .tests
            .iter()
            .find(|b| b.0.target.name() == bench)
            .map(|b| b.1.clone())
            .ok_or_else(|| anyhow!("no benchmark '{}'", bench))
    } else {
        match result.binaries.as_slice() {
            [path] => Ok(path.1.clone()),
            [] => Err(anyhow!("no targets found")),
            other => Err(anyhow!("found multiple targets: {:?}", other)),
        }
    }
}

/// Generate the `CompileOptions`. This is mostly about applying filters based
/// on user args, so we build as little as possible.
fn make_compile_opts (
    cargo_args: &CargoOpts,
    cfg: &Config,
) -> Result<CompileOptions> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileFilter;

    let mut opts = CompileOptions::new(cfg, CompileMode::Build)?;
    let profile = if cargo_args.release { "release" } else { "dev" };
    opts.build_config.requested_profile = InternedString::new(profile);
    if cargo_args.target != Target::Main {
        let (bins, examples, benches) = match &cargo_args.target {
            Target::Bin(bin) => (vec![bin.clone()], vec![], vec![]),
            Target::Example(bin) => (vec![], vec![bin.clone()], vec![]),
            Target::Bench(bin) => (vec![], vec![], vec![bin.clone()]),
            _ => unreachable!(),
        };

        opts.filter = CompileFilter::from_raw_arguments(
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
    Ok(opts)
}

/// Searches the workspace for the named target, returning an Error if it can't
/// be found.
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
        Err(anyhow!("missing target {}", target))
    } else {
        Ok(())
    }
}

fn open_file(file: &PathBuf) -> Result<()> {
    let status = Command::new("open").arg(file).status()?;

    if !status.success() {
        return Err(anyhow!("open failed"));
    }
    Ok(())
}
