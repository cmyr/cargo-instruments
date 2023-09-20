//! CLI argument handling

use anyhow::Result;
use cargo::core::resolver::CliFeatures;
use cargo::ops::Packages;
use std::fmt;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(bin_name = "cargo")]
pub(crate) enum Cli {
    /// Profile a binary with Xcode Instruments.
    ///
    /// By default, cargo-instruments will build your main binary.
    #[structopt(
        name = "instruments",
        after_help = "EXAMPLE:\n    cargo instruments -t time    Profile main binary with the (recommended) Time Profiler."
    )]
    Instruments(AppConfig),
}

#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::TrailingVarArg)]
pub(crate) struct AppConfig {
    /// List available templates
    #[structopt(short = "l", long)]
    pub(crate) list_templates: bool,

    /// Specify the instruments template to run
    ///
    /// To see available templates, pass `--list-templates`.
    #[structopt(
        short = "t",
        long = "template",
        value_name = "TEMPLATE",
        required_unless = "list-templates"
    )]
    pub(crate) template_name: Option<String>,

    /// Specify package for example/bin/bench
    ///
    /// For package that has only one bin, it's the same as `--bin PACKAGE_NAME`
    #[structopt(short = "p", long, value_name = "NAME")]
    package: Option<String>,

    /// Example binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    example: Option<String>,

    /// Binary to run
    #[structopt(long, group = "target", value_name = "NAME")]
    bin: Option<String>,

    /// Benchmark target to run
    #[structopt(long, group = "target", value_name = "NAME")]
    bench: Option<String>,

    /// Test harness target to run
    #[structopt(long, group = "target", value_name = "NAME")]
    harness: Option<String>,

    /// Test target to run
    #[structopt(long, value_name = "NAME")]
    test: Option<String>,

    /// Pass --release to cargo
    #[structopt(long, conflicts_with = "profile")]
    release: bool,

    /// Pass --profile NAME to cargo
    #[structopt(long, value_name = "NAME")]
    profile: Option<String>,

    /// Output .trace file to the given path
    ///
    /// Defaults to `target/instruments/{name}_{template-name}_{date}.trace`.
    ///
    /// If the file already exists, a new Run will be added.
    #[structopt(short = "o", long = "output", value_name = "PATH", parse(from_os_str))]
    pub(crate) trace_filepath: Option<PathBuf>,

    /// Limit recording time to the specified value (in milliseconds)
    ///
    /// The program will be terminated after this limit is exceeded.
    #[structopt(long, value_name = "MILLIS")]
    pub(crate) time_limit: Option<usize>,

    /// Open the generated .trace file after profiling
    ///
    /// The trace file will open in Xcode Instruments.
    #[structopt(long, hidden = true)]
    pub(crate) open: bool,

    /// Do not open the generated trace file in Instruments.app.
    #[structopt(long)]
    pub(crate) no_open: bool,

    /// Features to pass to cargo.
    #[structopt(long, value_name = "CARGO-FEATURES")]
    pub(crate) features: Option<String>,

    /// Path to Cargo.toml
    #[structopt(long, value_name = "PATH")]
    pub(crate) manifest_path: Option<PathBuf>,

    /// Activate all features for the selected target.
    #[structopt(long, display_order = 1001)]
    pub(crate) all_features: bool,

    /// Do not activate the default features for the selected target
    #[structopt(long, display_order = 1001)]
    pub(crate) no_default_features: bool,

    /// Arguments passed to the target binary.
    ///
    /// To pass flags, precede child args with `--`,
    /// e.g. `cargo instruments -- -t test1.txt --slow-mode`.
    #[structopt(value_name = "ARGS")]
    pub(crate) target_args: Vec<String>,
}

/// Represents the kind of target to profile.
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    Main,
    Example(String),
    Bin(String),
    Bench(String),
    Test(String, String),
}

/// The package in which to look for the specified target (example/bin/bench)
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Package {
    Default,
    Package(String),
}

impl From<Package> for Packages {
    fn from(p: Package) -> Self {
        match p {
            Package::Default => Packages::Default,
            Package::Package(s) => Packages::Packages(vec![s]),
        }
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Package::Default => {
                write!(f, "Default: search all packages for example/bin/bench")
            }
            Package::Package(s) => write!(f, "{}", s),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Target::Main => write!(f, "src/main.rs"),
            Target::Example(bin) => write!(f, "examples/{}.rs", bin),
            Target::Bin(bin) => write!(f, "bin/{}.rs", bin),
            Target::Bench(bench) => write!(f, "bench {}", bench),
            Target::Test(harness, test) => write!(f, "test {} {}", harness, test),
        }
    }
}

/// Cargo-specific options
pub(crate) struct CargoOpts {
    pub(crate) package: Package,
    pub(crate) target: Target,
    pub(crate) profile: String,
    pub(crate) features: CliFeatures,
}

impl AppConfig {
    pub(crate) fn to_cargo_opts(&self) -> Result<CargoOpts> {
        let package = self.get_package();
        let target = self.get_target();
        let features = self.features.clone().map(|s| vec![s]).unwrap_or_default();
        let features = CliFeatures::from_command_line(
            &features,
            self.all_features,
            !self.no_default_features,
        )?;
        let profile = self
            .profile
            .clone()
            .unwrap_or_else(|| (if self.release { "release" } else { "dev" }).to_owned());
        Ok(CargoOpts { package, target, profile, features })
    }

    fn get_package(&self) -> Package {
        if let Some(ref package) = self.package {
            Package::Package(package.clone())
        } else {
            Package::Default
        }
    }

    // valid target: --example,  --bin, --bench, --harness
    fn get_target(&self) -> Target {
        if let Some(ref example) = self.example {
            Target::Example(example.clone())
        } else if let Some(ref bin) = self.bin {
            Target::Bin(bin.clone())
        } else if let Some(ref bench) = self.bench {
            Target::Bench(bench.clone())
        } else if let Some(ref harness) = self.harness {
            let test = self.test.clone().unwrap_or_default();
            Target::Test(harness.clone(), test)

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
        let opts = AppConfig::from_iter(&["instruments", "-t", "template"]);
        assert!(opts.example.is_none());
        assert!(opts.bin.is_none());
        assert!(!opts.release);
        assert!(opts.trace_filepath.is_none());
        assert!(opts.package.is_none());
        assert!(opts.manifest_path.is_none());
    }

    #[test]
    fn package_is_given() {
        let opts =
            AppConfig::from_iter(&["instruments", "--package", "foo", "--template", "alloc"]);
        assert!(opts.example.is_none());
        assert!(opts.bin.is_none());
        assert!(opts.bench.is_none());
        assert_eq!(opts.package.unwrap().as_str(), "foo");

        let opts = AppConfig::from_iter(&[
            "instruments",
            "--package",
            "foo",
            "--template",
            "alloc",
            "--bin",
            "bin_arg",
        ]);
        assert!(opts.example.is_none());
        assert!(opts.bench.is_none());
        assert_eq!(opts.bin.unwrap().as_str(), "bin_arg");
        assert_eq!(opts.package.unwrap().as_str(), "foo");
    }

    #[test]
    #[should_panic(expected = "cannot be used with one or more of the other")]
    fn group_is_exclusive() {
        let opts = AppConfig::from_iter(&["instruments", "-t", "time", "--bin", "bin_arg"]);
        assert!(opts.example.is_none());
        assert_eq!(opts.bin.unwrap().as_str(), "bin_arg");

        let opts =
            AppConfig::from_iter(&["instruments", "-t", "time", "--example", "example_binary"]);
        assert!(opts.bin.is_none());
        assert_eq!(opts.example.unwrap().as_str(), "example_binary");
        let _opts = AppConfig::from_iter_safe(&[
            "instruments",
            "-t",
            "time",
            "--bin",
            "thing",
            "--example",
            "other",
        ])
        .unwrap();
    }

    #[test]
    fn limit_millis() {
        let opts = AppConfig::from_iter(&["instruments", "-t", "time", "--time-limit", "42000"]);
        assert_eq!(opts.time_limit, Some(42000));
        let opts = AppConfig::from_iter(&["instruments", "-t", "time", "--time-limit", "808"]);
        assert_eq!(opts.time_limit, Some(808));
        let opts = AppConfig::from_iter(&["instruments", "-t", "time"]);
        assert_eq!(opts.time_limit, None);
    }

    #[test]
    fn features() {
        let opts = &[
            "instruments",
            "--template",
            "time",
            "--example",
            "hello",
            "--features",
            "svg im",
            "--",
            "hi",
        ];
        let opts = AppConfig::from_iter(opts);
        assert_eq!(opts.template_name, Some("time".into()));
        assert_eq!(opts.example, Some("hello".to_string()));
        assert_eq!(opts.features, Some("svg im".to_string()));
        let features: Vec<_> = opts
            .to_cargo_opts()
            .unwrap()
            .features
            .features
            .iter()
            .map(|feat| feat.to_string())
            .collect();
        assert_eq!(features, vec!["im", "svg"]);
    }

    #[test]
    fn var_args() {
        let opts = AppConfig::from_iter(&[
            "instruments",
            "-t",
            "alloc",
            "--time-limit",
            "808",
            "--",
            "hi",
            "-h",
            "--bin",
        ]);
        assert_eq!(opts.template_name, Some("alloc".into()));
        assert_eq!(opts.time_limit, Some(808));
        assert_eq!(opts.target_args, vec!["hi", "-h", "--bin"]);
    }

    #[test]
    fn manifest_path() {
        let opts = AppConfig::from_iter(&[
            "instruments",
            "--manifest-path",
            "/path/to/Cargo.toml",
            "--template",
            "alloc",
        ]);
        assert!(opts.example.is_none());
        assert!(opts.bin.is_none());
        assert!(opts.bench.is_none());
        assert!(opts.package.is_none());
        assert_eq!(opts.manifest_path.unwrap(), PathBuf::from("/path/to/Cargo.toml"));
    }
}
