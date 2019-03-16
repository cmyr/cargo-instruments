//! CLI argument handling

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
pub(crate) struct Opts {
    /// Specify the instruments template to run
    ///
    /// To see available templates, pass --list.
    #[structopt(default_value = "time", value_name = "TEMPLATE")]
    pub(crate) template: String,
    /// Example binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    example: Option<String>,
    /// Binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    bin: Option<String>,
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

    //TODO: remove me
    #[doc(hidden)]
    #[structopt(long = "ddbg")]
    pub(crate) zdev_debug: bool,
}

/// The target, parsed from args.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Main,
    Example(String),
    Bin(String),
}

/// Cargo-specific options
pub(crate) struct CargoOpts {
    pub(crate) target: Target,
    pub(crate) release: bool,
}

impl Opts {
    pub(crate) fn to_cargo_opts(&self) -> CargoOpts {
        let target = match (self.example.as_ref(), self.bin.as_ref()) {
            (Some(example), None) => Target::Example(example.clone()),
            (None, Some(bin)) => Target::Bin(bin.clone()),
            (None, None) => Target::Main,
            (Some(_), Some(_)) => panic!("bin & example are exclusive, enforced by StructOpt"),
        };

        CargoOpts { target, release: self.release }
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
}
