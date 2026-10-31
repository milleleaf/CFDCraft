mod cmd;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cfdcraft",
    about = "Compile and validate reproducible CFD studies",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Compile a solver-neutral study into an ESI v2512 case.
    Compile { study: PathBuf, out_dir: PathBuf },
    /// Compile and run a study, then judge solver evidence.
    Run {
        study: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
    },
    /// Run the Re=100 cavity and compare both centrelines with Ghia.
    Validate {
        study: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        #[arg(long)]
        tolerance_override: Option<PathBuf>,
    },
}
fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Compile { study, out_dir } => cmd::compile::execute(&study, &out_dir),
        Commands::Run {
            study,
            key,
            profile,
            hosts,
        } => cmd::run::execute(&study, &key, &hosts, &profile, false, None),
        Commands::Validate {
            study,
            key,
            profile,
            hosts,
            tolerance_override,
        } => cmd::run::execute(
            &study,
            &key,
            &hosts,
            &profile,
            true,
            tolerance_override.as_deref(),
        ),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2)
        }
    }
}
