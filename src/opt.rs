use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(bin_name = "cargo")]
pub(crate) enum Cli {
    #[structopt(name = "instrument")]
    Instrument(Opts),
}

#[derive(Debug, StructOpt)]
#[structopt(name = "cargo-instrument", about = "About cargo-instrument")]
pub(crate) struct Opts {
    /// macOS only, specify the template for instruments
    #[structopt(short = "t", long)]
    pub(crate) template: Option<String>,
    /// example binary to run
    #[structopt(long, group = "target")]
    example: Option<String>,
    #[structopt(long, group = "target")]
    bin: Option<String>,
    #[structopt(long)]
    release: bool,
    /// Output file, stdout if not present
    #[structopt(short = "o", long = "out", parse(from_os_str))]
    pub(crate) output: Option<PathBuf>,

    //TODO: remove me
    /// development only flag.
    #[structopt(long)]
    pub(crate) ddebug: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Main,
    Example(String),
    Bin(String),
}

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

// options:
//
// cargo instrument
// cargo instrument -t time --test some_test_case
// cargo instrument -t time --example my_example
// cargo instrument -t time --example my_example -o my_output

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let opts = Opts::from_iter(&["instrument"]);
        assert!(opts.template.is_none());
        assert!(opts.example.is_none());
        assert!(opts.bin.is_none());
        assert!(!opts.release);
        assert!(opts.output.is_none());
    }

    #[test]
    #[should_panic(expected = "cannot be used with one or more of the other")]
    fn group_is_exclusive() {
        let opts = Opts::from_iter(&["instrument", "--bin", "bin_arg"]);
        assert!(opts.example.is_none());
        assert_eq!(opts.bin.unwrap().as_str(), "bin_arg");

        let opts = Opts::from_iter(&["instrument", "--example", "example_binary"]);
        assert!(opts.bin.is_none());
        assert_eq!(opts.example.unwrap().as_str(), "example_binary");
        let _opts =
            Opts::from_iter_safe(&["instrument", "--bin", "thing", "--example", "other"]).unwrap();
    }
}
