//! CLI argument handling

use std::fmt;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(bin_name = "cargo")]
pub(crate) enum Cli {
    /// Profile a binary with Xcode Instruments.
    ///
    /// By default, cargo-instruments will build your main binary.
    #[structopt(name = "instruments")]
    Instruments(Opts),
}

#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::TrailingVarArg)]
pub(crate) struct Opts {
    /// Specify the instruments template to run
    ///
    /// To see available templates, pass --list.
    #[structopt(short = "t", long, default_value = "time", value_name = "TEMPLATE")]
    pub(crate) template: String,
    /// Example binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    example: Option<String>,
    /// Binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    bin: Option<String>,
    /// Benchmark target to run
    #[structopt(long, group = "target", value_name = "NAME")]
    bench: Option<String>,
    /// Pass --release to cargo
    #[structopt(long)]
    release: bool,
    /// List available templates
    #[structopt(long)]
    pub(crate) list: bool,
    /// Output file. If missing, defaults to 'target/instruments/{name}{date}.trace'
    ///
    /// This file may already exist, in which case a new Run will be added.
    #[structopt(short = "o", long = "out", value_name = "PATH", parse(from_os_str))]
    pub(crate) output: Option<PathBuf>,

    /// Optionally limit the maximum running time of the application.
    /// It will be terminated if this is exceded.
    #[structopt(short = "l", long, value_name = "MILLIS")]
    pub(crate) limit: Option<usize>,

    /// Open the generated .trace file when finished
    #[structopt(long)]
    pub(crate) open: bool,

    /// Arguments passed to the target binary. To pass flags, precede child args
    /// with --, e.g. `cargo instruments -- -t test1.txt --slow-mode`.
    #[structopt(value_name = "ARGS")]
    pub(crate) target_args: Vec<String>,
}

/// The target, parsed from args.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Main,
    Example(String),
    Bin(String),
    Bench(String),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Target::Main => write!(f, "src/main.rs"),
            Target::Example(bin) => write!(f, "examples/{}.rs", bin),
            Target::Bin(bin) => write!(f, "bin/{}.rs", bin),
            Target::Bench(bench) => write!(f, "bench {}", bench),
        }
    }
}

/// Cargo-specific options
pub(crate) struct CargoOpts {
    pub(crate) target: Target,
    pub(crate) release: bool,
}

impl Opts {
    pub(crate) fn to_cargo_opts(&self) -> CargoOpts {
        let target = self.get_target();
        CargoOpts { target, release: self.release }
    }

    fn get_target(&self) -> Target {
        if let Some(example) = self.example.clone() {
            Target::Example(example)
        } else if let Some(bin) = self.bin.clone() {
            Target::Bin(bin)
        } else if let Some(bench) = self.bench.clone() {
            Target::Bench(bench)
        } else {
            Target::Main
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let opts = Opts::from_iter(&["instruments"]);
        assert_eq!(opts.template.as_str(), "time");
        assert!(opts.example.is_none());
        assert!(opts.bin.is_none());
        assert!(!opts.release);
        assert!(opts.output.is_none());
    }

    #[test]
    #[should_panic(expected = "cannot be used with one or more of the other")]
    fn group_is_exclusive() {
        let opts = Opts::from_iter(&["instruments", "--bin", "bin_arg"]);
        assert!(opts.example.is_none());
        assert_eq!(opts.bin.unwrap().as_str(), "bin_arg");

        let opts = Opts::from_iter(&["instruments", "--example", "example_binary"]);
        assert!(opts.bin.is_none());
        assert_eq!(opts.example.unwrap().as_str(), "example_binary");
        let _opts =
            Opts::from_iter_safe(&["instruments", "--bin", "thing", "--example", "other"]).unwrap();
    }

    #[test]
    fn limit_millis() {
        let opts = Opts::from_iter(&["instruments", "-l", "420"]);
        assert_eq!(opts.limit, Some(420));
        let opts = Opts::from_iter(&["instruments", "--limit", "808"]);
        assert_eq!(opts.limit, Some(808));
        let opts = Opts::from_iter(&["instruments"]);
        assert_eq!(opts.limit, None);
    }

    #[test]
    fn var_args() {
        let opts = Opts::from_iter(&[
            "instruments",
            "-t",
            "alloc",
            "--limit",
            "808",
            "--",
            "hi",
            "-h",
            "--bin",
        ]);
        assert_eq!(opts.template, "alloc");
        assert_eq!(opts.limit, Some(808));
        assert_eq!(opts.target_args, vec!["hi", "-h", "--bin"]);
    }
}
