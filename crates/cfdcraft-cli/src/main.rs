mod cmd;

use cfdcraft_ops::RunMode;
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
    Compile {
        study: PathBuf,
        out_dir: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Write a runnable case: the compiled files plus an Allrun script and a README.
    Export {
        study: PathBuf,
        out_dir: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Scan a case directory for entries that could run code or reach outside the case.
    CheckCase {
        dir: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Compile and run a study, then judge solver evidence.
    Run {
        study: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        /// Run an exported case directory; it must be byte-identical to the study's export.
        #[arg(long)]
        case_dir: Option<PathBuf>,
        /// Run a few iterations only (at most 60). Shows the case loads and iterates, nothing more.
        #[arg(long)]
        smoke: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
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
        /// Run an exported case directory; it must be byte-identical to the study's export.
        #[arg(long)]
        case_dir: Option<PathBuf>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Serve the typed operations to an agent over MCP on standard input and output.
    Mcp {
        /// The directory that holds studies, cases and runs. Created if it is absent.
        #[arg(long)]
        workspace: PathBuf,
        /// The ssh key file for the compute host. A server setting, never a tool argument.
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
    },
}
fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Compile {
            study,
            out_dir,
            json,
        } => cmd::compile::execute(&study, &out_dir, json),
        Commands::Export {
            study,
            out_dir,
            json,
        } => cmd::export::execute(&study, &out_dir, json),
        Commands::CheckCase { dir, json } => cmd::check_case::execute(&dir, json),
        Commands::Run {
            study,
            key,
            profile,
            hosts,
            case_dir,
            smoke,
            json,
        } => cmd::run::execute(&cmd::run::RunArgs {
            mode: if smoke { RunMode::Smoke } else { RunMode::Run },
            study,
            key,
            profile,
            hosts,
            case_dir,
            tolerance_override: None,
            json,
        }),
        Commands::Validate {
            study,
            key,
            profile,
            hosts,
            tolerance_override,
            case_dir,
            json,
        } => cmd::run::execute(&cmd::run::RunArgs {
            mode: RunMode::Validate,
            study,
            key,
            profile,
            hosts,
            case_dir,
            tolerance_override,
            json,
        }),
        Commands::Mcp {
            workspace,
            key,
            profile,
            hosts,
        } => cfdcraft_mcp::serve_stdio_blocking(cfdcraft_mcp::McpConfig {
            workspace,
            key,
            hosts,
            profile,
        })
        .map(|()| 0)
        .map_err(Into::into),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2)
        }
    }
}
