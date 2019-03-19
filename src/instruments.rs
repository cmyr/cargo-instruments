//! interfacing with the `instruments` command line tool

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use failure::{format_err, Error};

use crate::opt::Opts;

/// Check that `instruments` is in $PATH.
pub(crate) fn check_existence() -> Result<(), Error> {
    let path = ["/", "usr", "bin", "instruments"].iter().collect::<PathBuf>();
    if path.exists() {
        Ok(())
    } else {
        Err(format_err!(
            "/usr/bin/instruments does not exist. \
             Please install the Xcode Command Line Tools."
        ))
    }
}

/// Return a string listing available templates.
pub(crate) fn list() -> Result<String, Error> {
    let Output { status, stdout, .. } =
        Command::new("instruments").args(&["-s", "templates"]).output()?;

    if !status.success() {
        return Err(format_err!("'instruments -s templates' command errored"));
    }

    let templates = String::from_utf8(stdout)?;
    let mut output: String = "Instruments provides the following built-in templates.\n\
                              Aliases are indicated in parentheses.\n"
        .into();

    let mut templates = templates
        .lines()
        .skip(1)
        .map(|line| (line, abbrev_name(line.trim().trim_matches('"'))))
        .collect::<Vec<_>>();

    if templates.is_empty() {
        return Err(format_err!("no templates returned from 'instruments -s templates'"));
    }

    let max_width = templates.iter().map(|(l, _)| l.len()).max().unwrap();

    templates.sort_by_key(|&(_, abbrv)| abbrv.is_none());

    for (name, abbrv) in templates {
        output.push('\n');
        output.push_str(name);
        if let Some(abbrv) = abbrv {
            let some_spaces = "                                              ";
            let lpad = max_width - name.len();
            output.push_str(&some_spaces[..lpad]);
            output.push_str(&format!("({})", abbrv));
        }
    }

    Ok(output)
}

pub(crate) fn run(
    args: &Opts,
    exec_path: PathBuf,
    workspace_root: &PathBuf,
) -> Result<PathBuf, Error> {
    let outfile = get_out_file(args, &exec_path, &workspace_root)?;
    let template = resolve_template(&args);

    let mut command = Command::new("instruments");
    command.args(&["-t", &template]).arg("-D").arg(&outfile);

    if let Some(limit) = args.limit {
        command.args(&["-l", &limit.to_string()]);
    }

    command.arg(&exec_path);

    if !args.target_args.is_empty() {
        command.args(args.target_args.as_slice());
    }

    let output = command.output()?;

    if !output.status.success() {
        let stderr =
            String::from_utf8(output.stderr).unwrap_or_else(|_| "failed to capture stderr".into());
        Err(format_err!("instruments errored: {}", stderr))
    } else {
        Ok(outfile)
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
        .ok_or_else(|| format_err!("invalid exec path {:?}", exec_path))?;

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
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir)
            .map_err(|e| format_err!("failed to create {:?}: {}", &target_dir, e))?;
    }
    Ok(target_dir)
}

fn now_timestamp() -> impl std::fmt::Display {
    use chrono::prelude::*;
    let now = Local::now();
    let fmt = "%FT%T";
    now.format(&fmt)
}

fn resolve_template(args: &Opts) -> &str {
    match args.template.as_str() {
        "time" => "Time Profiler",
        "alloc" => "Allocations",
        "io" => "File Activity",
        "sys" => "System Trace",
        other => other,
    }
}

fn abbrev_name(template: &str) -> Option<&'static str> {
    match template {
        "Time Profiler" => Some("time"),
        "Allocations" => Some("alloc"),
        "File Activity" => Some("io"),
        "System Trace" => Some("sys"),
        _ => None,
    }
}
