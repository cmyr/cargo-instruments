extern crate structopt;

mod opt;
mod app;

use opt::{Cli, Opts};

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};

use failure::Error;
use structopt::StructOpt;

fn main() {

    //eprintln!("args: {:?}", ::std::env::args().collect::<Vec<_>>());
    let Cli::Instrument(args) = Cli::from_args();

    if let Err(e) = run(args) {
        eprintln!("{:?}", e);
        process::exit(1);
    }
}

fn run(args: Opts) -> Result<(), Error> {
    // do cargo build

    app::run(&args)?;
    //let exec_path = cargo_build(&args)?;
    //let exit_code = run_profiler(&exec_path, &args)?;
    //eprintln!("exited with {:?}", exit_code);
    Ok(())
}

fn run_profiler(exec_path: &Path, args: &Opts) -> Result<ExitStatus, Error> {
    let default_template = "Time Profiler";
    let template = args.template.as_ref().map(|s| s.as_str()).unwrap_or(default_template);
    let out_dir = get_target_dir(&args)?;
    let out_file = out_dir.join(get_timestamp_file_name());

    eprintln!("tracing {:?}, saving to {:?}", exec_path, &out_file);

    let status = Command::new("instruments")
        .arg("-t")
        .arg(&template)
        .arg("-D")
        .arg(&out_file)
        .arg(&exec_path)
        .status()?;
    Ok(status)
}

fn cargo_build(_args: &Opts) -> Result<PathBuf, Error> {
    use cargo::core::compiler::CompileMode;
    use cargo::ops::CompileOptions;
    let cfg = cargo::util::config::Config::default()?;
    let opts = CompileOptions::new(&cfg, CompileMode::Build)?;
    unimplemented!()
    //let result = cargo::ops::compile(&ws, &opts)?;

    //Ok(result.binaries.first().unwrap().to_owned())
}

fn get_target_dir(_args: &Opts) -> Result<PathBuf, Error> {
    let path = PathBuf::from("target/profile");
    if !path.exists() {
        fs::create_dir(&path)?;
    }
    Ok(path)
}

fn get_timestamp_file_name() -> String {
    use chrono::prelude::*;
    let now = Local::now();
    format!("{}.trace", now.to_rfc3339())
}
