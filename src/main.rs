extern crate structopt;

mod app;
mod instruments;
mod opt;

use opt::Cli;

use std::process;

use structopt::StructOpt;

fn main() {
    let Cli::Instrument(args) = Cli::from_args();

    if let Err(e) = app::run(args) {
        eprintln!("{:?}", e);
        process::exit(1);
    }
}
