mod app;
mod instruments;
mod opt;

#[cfg(not(target_os = "macos"))]
compile_error!("cargo-instruments requires macOS.");

fn main() {
    use structopt::StructOpt;
    let opt::Cli::Instruments(args) = opt::Cli::from_args();

    if let Err(e) = app::run(args) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
