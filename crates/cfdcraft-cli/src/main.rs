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
    /// Save a study as a `.cfdcraft` file.
    Save {
        study: PathBuf,
        out: PathBuf,
        /// Replace the output file if it already exists.
        #[arg(long)]
        replace: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Open a `.cfdcraft` file and print its study hash; an older file prints its migration note.
    Open {
        file: PathBuf,
        /// Also write the study as JSON to this new file; an existing file is never overwritten.
        #[arg(long)]
        study_out: Option<PathBuf>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Link a file to a study file, stored relative to the study file's folder.
    Link {
        doc: PathBuf,
        file: PathBuf,
        /// Name for the link; by default made from the file name.
        #[arg(long)]
        id: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Check that the files a study file links to are where the links say.
    CheckLinks {
        doc: PathBuf,
        /// Also read each file and warn when it has changed since it was linked.
        #[arg(long)]
        verify: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Pack a study file and its stored results into one file; linked files are not included.
    Pack {
        doc: PathBuf,
        out: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Unpack a pack into a folder: the study file and its stored results.
    Unpack {
        packed: PathBuf,
        dir: PathBuf,
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
        Commands::Save {
            study,
            out,
            replace,
            json,
        } => cmd::document::save(&study, &out, replace, json),
        Commands::Open {
            file,
            study_out,
            json,
        } => cmd::document::open(&file, study_out.as_deref(), json),
        Commands::Link {
            doc,
            file,
            id,
            json,
        } => cmd::document::link(&doc, &file, id.as_deref(), json),
        Commands::CheckLinks { doc, verify, json } => {
            cmd::document::check_links(&doc, verify, json)
        }
        Commands::Pack { doc, out, json } => cmd::document::pack(&doc, &out, json),
        Commands::Unpack { packed, dir, json } => cmd::document::unpack(&packed, &dir, json),
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
