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
    #[structopt(short = "t")]
    pub(crate) template: Option<String>,
    /// example binary to run
    #[structopt(long, parse(from_os_str))]
    pub(crate) example: Option<PathBuf>,
    #[structopt(long = "test", default_value = "")]
    test: String,
    #[structopt(long = "release")]
    release: bool,
    /// Output file, stdout if not present
    #[structopt(short = "o", long = "out", parse(from_os_str))]
    pub(crate) output: Option<PathBuf>,
}


// options:
//
// cargo instrument
// cargo instrument -t time --test some_test_case
// cargo instrument -t time --example my_example
// cargo instrument -t time --example my_example -o my_output
