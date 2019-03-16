use cargo::core::Workspace;

//use crate::error::Error;
use crate::opt::{Opts, Target};
use failure::{Error, format_err};

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

pub(crate) fn run(args: &Opts) -> Result<(), Error> {
    eprintln!("opts: {:?}", args);
    let cfg = cargo::util::config::Config::default()?;
    let path = cargo::util::important_paths::find_root_manifest_for_wd(cfg.cwd())?;
    let workspace = Workspace::new(&path, &cfg)?;
    build_target(args, &workspace)?;
    //let current_name = ws.current().map(|p| p.manifest().name())?;
    Ok(())
}

fn build_target(args: &Opts, workspace: &Workspace) -> Result<(), Error> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileOptions;
    use cargo::core::shell::Verbosity;

    workspace.config().shell().set_verbosity(Verbosity::Normal);

    let cargo_args = args.to_cargo_opts();

    validate_target(&cargo_args.target, workspace)?;
    return Ok(());

    //TODO: verify that workspace contains a valid target
    let mut opts = CompileOptions::new(workspace.config(), CompileMode::Build)?;
    opts.build_config.release = cargo_args.release;
    //eprintln!("compie options: {:?}", &opts);

    let result = cargo::ops::compile(workspace, &opts)?;
    debug_compilation_result(&result);
    Ok(())
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
    eprintln!("\
    tests: {:?}\n\
    bins: {:?}\n\
    ", &result.tests, &result.binaries);
}
