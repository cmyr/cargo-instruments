mod app;
mod instruments;
mod opt;

fn main() {
    use structopt::StructOpt;
    let opt::Cli::Instrument(args) = opt::Cli::from_args();

    if let Err(e) = app::run(args) {
        eprintln!("{:?}", e);
        std::process::exit(1);
    }
}
