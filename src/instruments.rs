use std::fs;
use std::path::PathBuf;
use std::process::Command;

use failure::{format_err, Error};

use crate::opt::Opts;

/// Check that `instruments` is in $PATH.
pub(crate) fn check_existence() -> Result<(), Error> {
    let path = ["/", "usr", "bin", "instruments"].iter().collect::<PathBuf>();
    match path.exists() {
        true => Ok(()),
        false => Err(format_err!(
            "/usr/bin/instruments does not exist. \
             Please install the Xcode Command Line Tools."
        )),
    }
}

pub(crate) fn run(args: &Opts, exec_path: PathBuf, workspace_root: PathBuf) -> Result<(), Error> {
    let outfile = get_out_file(args, &exec_path, &workspace_root)?;
    let template = resolve_template(&args);

    eprintln!("profiling {:?} with '{}', saving to {:?}", exec_path, template, outfile);

    if args.ddebug {
        return Err(format_err!("aborted for debug"));
    }

    let status = Command::new("instruments")
        .args(&["-t", &template])
        .args(&["-l", "5000"])
        .arg("-D")
        .arg(&outfile)
        .arg(&exec_path)
        .status()?;

    match status.success() {
        false => Err(format_err!("instruments failed")),
        true => Ok(()),
    }
}

fn get_out_file(
    args: &Opts,
    exec_path: &PathBuf,
    workspace_root: &PathBuf,
) -> Result<PathBuf, Error> {
    if let Some(path) = args.output.clone() {
        return Ok(path);
    }

    let exec_name = exec_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or(format_err!("invalid exec path {:?}", exec_path))?;

    let filename = format!("{}_{}", exec_name, now_timestamp());
    let mut path = get_target_dir(workspace_root)?;
    path.push(filename);
    path.set_extension("trace");
    Ok(path)
}

fn get_target_dir(workspace_root: &PathBuf) -> Result<PathBuf, Error> {
    let mut target_dir = workspace_root.clone();
    target_dir.push("target");
    target_dir.push("instruments");
    eprintln!("target_dir: {:?}", &target_dir);
    if !target_dir.exists() {
        fs::create_dir(&target_dir)?;
    }
    Ok(target_dir)
}

fn now_timestamp() -> impl std::fmt::Display {
    use chrono::prelude::*;
    let now = Local::now();
    let fmt = "%FT%T";
    now.format(&fmt)
}

fn resolve_template(_args: &Opts) -> String {
    String::from("Time Profiler")
}
