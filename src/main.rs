mod app;
mod demangle;
mod instruments;
mod opt;

#[cfg(not(target_os = "macos"))]
compile_error!("cargo-instruments requires macOS.");

fn main() {
    env_logger::init();
    use clap::Parser;
    let opt::Cli::Instruments(args) = opt::Cli::parse();

    if let Err(e) = app::run(args) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
