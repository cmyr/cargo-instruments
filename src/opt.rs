use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(bin_name = "cargo")]
pub(crate) enum Cli {
    #[structopt(name = "profile")]
    Profile(Opts),
}

#[derive(Debug, StructOpt)]
#[structopt(name = "cargo-profile", about = "About cargo-profile")]
pub(crate) struct Opts {
    /// macOS only, specify the template for instruments
    #[structopt(short = "t")]
    pub(crate) template: Option<String>,
    /// example binary to run
    #[structopt(long, parse(from_os_str))]
    pub(crate) example: Option<PathBuf>,
    /// Output file, stdout if not present
    #[structopt(short = "o", long = "out", parse(from_os_str))]
    pub(crate) output: Option<PathBuf>,
}
