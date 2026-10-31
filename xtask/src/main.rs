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
    /// Require registry-only dependencies and no source overrides.
    Sources,
}

fn main() {
    let cli = Cli::parse();
    let root = std::env::current_dir().expect("cwd");
    let result = match cli.command {
        Command::Pins => pins::check_pins(&root),
        Command::Layers => layers::check_layers(&root),
        Command::Sources => sources::check_sources(&root),
    };
    if let Err(report) = result {
        eprintln!("{report}");
        std::process::exit(1);
    }
}
