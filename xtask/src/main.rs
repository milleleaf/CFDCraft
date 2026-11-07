pub mod dockerfiles;
pub mod equivalents;
pub mod layers;
pub mod pins;
pub mod sources;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "cfdcraft workspace governance")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Require an exact `=x.y.z` pin on every dependency.
    Pins,
    /// Assert the L0 model crate stays free of compute/UI dependencies.
    Layers,
    /// Require registry dependencies (Craft libraries only from their git source, by commit) and no source overrides.
    Sources,
    /// Refuse in-house copies of the shared crash-safe save.
    Equivalents,
    /// Require every solver image definition to pin its base, check its downloads and reset its entrypoint.
    Dockerfiles,
}

fn main() {
    let cli = Cli::parse();
    let root = std::env::current_dir().expect("cwd");
    let result = match cli.command {
        Command::Pins => pins::check_pins(&root),
        Command::Layers => layers::check_layers(&root),
        Command::Sources => sources::check_sources(&root),
        Command::Equivalents => equivalents::check_equivalents(&root),
        Command::Dockerfiles => dockerfiles::check_dockerfiles(&root),
    };
    if let Err(report) = result {
        eprintln!("{report}");
        std::process::exit(1);
    }
}
