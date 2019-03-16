use std::path::PathBuf;

use cargo::core::Workspace;
use cargo::ops::CompileOptions;
use cargo::util::config::Config;
use failure::{format_err, Error};

use crate::instruments;
use crate::opt::{CargoOpts, Opts, Target};

// RESCOPE:
//
// only build main executable or an example
//
//
// TODOS:
//
// - support --bin and --example
// - only time profiler

// FUTURE:
//
// allow building of a benchmark

pub(crate) fn run(args: Opts) -> Result<(), Error> {
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

    let exec_path = build_target(&args, &workspace)?;

    instruments::run(&args, exec_path, workspace_root)
}

fn build_target(args: &Opts, workspace: &Workspace) -> Result<PathBuf, Error> {
    use cargo::core::shell::Verbosity;
    workspace.config().shell().set_verbosity(Verbosity::Normal);

    let cargo_args = args.to_cargo_opts();

    validate_target(&cargo_args.target, workspace)?;

    let opts = make_compile_opts(&cargo_args, workspace.config())?;

    let result = cargo::ops::compile(workspace, &opts)?;
    debug_compilation_result(&result);

    match result.binaries.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format_err!("no targets found")),
        other => Err(format_err!("unexpectedly built multiple targets: {:?}", other)),
    }
}

fn make_compile_opts<'a>(
    cargo_args: &CargoOpts,
    cfg: &'a Config,
) -> Result<CompileOptions<'a>, Error> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileFilter;

    let mut opts = CompileOptions::new(cfg, CompileMode::Build)?;
    opts.build_config.release = cargo_args.release;
    if &cargo_args.target != &Target::Main {
        let (bins, examples) = match &cargo_args.target {
            Target::Bin(bin) => (vec![bin.clone()], vec![]),
            Target::Example(bin) => (vec![], vec![bin.clone()]),
            _ => unreachable!(),
        };

        opts.filter = CompileFilter::new(
            false,
            bins,
            false,
            Vec::new(),
            false,
            examples,
            false,
            Vec::new(),
            false,
            false,
        );
    }
    Ok(opts)
}

fn validate_target(target: &Target, workspace: &Workspace) -> Result<(), Error> {
    let package = workspace.current()?;
    eprintln!("TARGETS: {:?}", &package.targets());
    let mut targets = package.targets().iter();
    let has_target = match target {
        Target::Main => targets.find(|t| t.is_bin()).is_some(),
        Target::Bin(name) => targets.find(|t| t.is_bin() && t.name() == name).is_some(),
        Target::Example(name) => targets.find(|t| t.is_example() && &t.name() == name).is_some(),
    };
    if !has_target {
        Err(format_err!("missing target {:?}", target))
    } else {
        Ok(())
    }
}

fn debug_compilation_result(result: &cargo::core::compiler::Compilation) {
    eprintln!(
        "\
         tests: {:?}\n\
         bins: {:?}\n\
         ",
        &result.tests, &result.binaries
    );
}
