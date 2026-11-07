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
    /// Print the mesh key and the result key of a study.
    Keys {
        study: PathBuf,
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
        /// Remove the host label from every run record in the pack; the source file keeps its labels.
        #[arg(long)]
        redact_host: bool,
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
    /// Say whether a study file's mesh and result are up to date, and if not why.
    Status {
        doc: PathBuf,
        /// Report this variant instead of the study itself; base is the study.
        #[arg(long)]
        scenario: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the runs a study file holds, as JSON data.
    Runs {
        doc: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print the check verdicts and key numbers of a run, as JSON data.
    #[command(name = "result")]
    ShowResult {
        doc: PathBuf,
        /// The run to show; by default the newest that recorded a result and is not a rerun.
        #[arg(long)]
        run: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print what a run was made from, as JSON data.
    Provenance {
        doc: PathBuf,
        /// The run to show; by default the newest.
        #[arg(long)]
        run: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Compare the key numbers of two runs; exits 1 when they differ beyond the tolerance.
    Compare {
        doc: PathBuf,
        /// The run to compare against.
        a: String,
        /// The run to compare.
        b: String,
        /// The absolute tolerance (default 1e-12).
        #[arg(long, allow_negative_numbers = true)]
        abs_tol: Option<f64>,
        /// The relative tolerance (default 1e-9).
        #[arg(long, allow_negative_numbers = true)]
        rel_tol: Option<f64>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Run the study file's case on the compute host for whatever is out of date, and record the run.
    Solve {
        doc: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        /// Run the Re=100 cavity and compare it with the reference; refused for any other study.
        #[arg(long)]
        validate: bool,
        /// Solve this variant instead of the study itself; base is the study.
        #[arg(long)]
        scenario: Option<String>,
        /// Make a result that was pruned again; otherwise a pruned result is left alone.
        #[arg(long)]
        remake_pruned: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Run the settings a recorded run was made from again, and compare with that run.
    Rerun {
        doc: PathBuf,
        /// The run to repeat.
        #[arg(long)]
        run: String,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        /// The absolute tolerance of the comparison (default 1e-12).
        #[arg(long, allow_negative_numbers = true)]
        abs_tol: Option<f64>,
        /// The relative tolerance of the comparison (default 1e-9).
        #[arg(long, allow_negative_numbers = true)]
        rel_tol: Option<f64>,
        /// Run with this build's tools even when the run was made with others.
        #[arg(long)]
        allow_tool_change: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Bring out-of-date items of one or more study files up to date, mesh before result.
    Update {
        #[arg(required = true)]
        docs: Vec<PathBuf>,
        /// Update only this item, written name:mesh or name:result; repeat for several.
        #[arg(long = "item")]
        items: Vec<String>,
        /// Update the chosen items in this order, one --order for each; a result stays after its mesh.
        #[arg(long = "order")]
        order: Vec<String>,
        /// Show what would be done and start nothing; needs no compute host.
        #[arg(long)]
        plan: bool,
        /// The ssh key file; needed unless --plan is given.
        #[arg(long)]
        key: Option<PathBuf>,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        /// Run the Re=100 cavity and compare it with the reference; refused for any other study.
        #[arg(long)]
        validate: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Make, list, show, compare, name, pin and annotate the variants of a study file.
    Scenario {
        #[command(subcommand)]
        action: ScenarioAction,
    },
    /// Plan, create and check parameter sweeps of a study file.
    Sweep {
        #[command(subcommand)]
        action: SweepAction,
    },
    /// Say how many bytes of stored results each variant of a study file holds, as JSON data.
    Disk {
        doc: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Remove stored results no pinned variant needs; shows what would go unless --apply is given.
    Prune {
        doc: PathBuf,
        /// Prune only this variant's results; repeat for several. By default every variant.
        #[arg(long = "scenario")]
        scenarios: Vec<String>,
        /// Really remove the results. Without it nothing is removed.
        #[arg(long)]
        apply: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print the monitored quantities, the balances and the verdict of a recorded run or of a job.
    Monitors {
        doc: PathBuf,
        /// The recorded run to show, made again from its stored result.
        #[arg(long)]
        run: Option<String>,
        /// The queued job to show, as `queue watch` does; by default the active or newest job.
        #[arg(long)]
        job: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Queue jobs for a study file, run them in the background and watch them.
    Queue {
        #[command(subcommand)]
        action: QueueAction,
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
        /// Run solves inside the server, as before, so they stop when it stops; by default they
        /// are queued and run by a runner that goes on after the server ends.
        #[arg(long)]
        attached_jobs: bool,
    },
}
#[derive(Subcommand)]
enum ScenarioAction {
    /// Make a variant, or change the changes of one that exists.
    Set {
        doc: PathBuf,
        /// The variant to make or change.
        id: String,
        /// The variant it descends from; by default the study itself.
        #[arg(long)]
        parent: Option<String>,
        /// A change, written SETTING=JSON, for example /mesh/cells_x=64; repeat for several.
        #[arg(long = "set")]
        sets: Vec<String>,
        /// A setting to put back to what the parent has; repeat for several.
        #[arg(long = "unset")]
        unsets: Vec<String>,
        /// A name for the variant.
        #[arg(long)]
        name: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the variants of a study file, as JSON data.
    List {
        doc: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print one variant with its changes and keys, as JSON data.
    Show {
        doc: PathBuf,
        /// The variant to show; base is the study itself.
        id: String,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Say how the second variant differs from the first.
    Diff {
        doc: PathBuf,
        /// The variant to compare against.
        a: String,
        /// The variant to compare.
        b: String,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Name, pin or annotate a variant; an empty name or notes text clears it.
    Label {
        doc: PathBuf,
        /// The variant to label; base is the study itself.
        id: String,
        /// The new name.
        #[arg(long)]
        name: Option<String>,
        /// Keep the stored results of this variant when results are pruned.
        #[arg(long, conflicts_with = "unpin")]
        pin: bool,
        /// Stop keeping the stored results of this variant.
        #[arg(long)]
        unpin: bool,
        /// The new notes.
        #[arg(long)]
        notes: Option<String>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
}
#[derive(Subcommand)]
enum SweepAction {
    /// Print the points a sweep would make; nothing is stored.
    Plan {
        doc: PathBuf,
        /// A file holding the sweep as JSON.
        spec: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Store the points of a sweep as variants.
    Create {
        doc: PathBuf,
        /// A file holding the sweep as JSON.
        spec: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Check every point of a stored sweep without solving anything; exits 1 when a point fails.
    Preflight {
        doc: PathBuf,
        /// The sweep to check.
        sweep: String,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
}
#[derive(Subcommand)]
enum QueueAction {
    /// Put jobs in the queue: the study, a variant, every point of a sweep, or a run to continue.
    Add {
        doc: PathBuf,
        /// Queue this variant instead of the study itself; base is the study.
        #[arg(long)]
        scenario: Option<String>,
        /// Queue every point of this stored sweep, in point order.
        #[arg(long)]
        sweep: Option<String>,
        /// Queue the continuation of this stopped run.
        #[arg(long = "continue")]
        continue_run: Option<String>,
        /// Make a result that was pruned again; otherwise a pruned result is left alone.
        #[arg(long)]
        remake_pruned: bool,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the jobs of the queue in the order they start, one line each.
    List {
        doc: PathBuf,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Move a waiting job to a place among the waiting jobs.
    Reorder {
        doc: PathBuf,
        /// The job to move, for example q-003.
        job: String,
        /// The place to move it to among the waiting jobs; 1 starts first.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        to: u32,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Cancel a job: a waiting one ends at once, a running one is asked to stop.
    Cancel {
        doc: PathBuf,
        /// The job to cancel, for example q-002.
        job: String,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Ask a running job to write its latest results and stop.
    Stop {
        doc: PathBuf,
        /// The job to stop, for example q-001.
        job: String,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Start the runner that advances the queue and return at once; it goes on after this
    /// terminal is closed.
    Drain {
        doc: PathBuf,
        /// The ssh key file for the compute host. It is read to connect and never printed.
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "box")]
        profile: String,
        #[arg(long, default_value = "hosts.toml")]
        hosts: PathBuf,
        /// The folder for the tunnel sockets; by default a short folder of the system's.
        #[arg(long)]
        socket_dir: Option<PathBuf>,
        /// Run the queue in this process and return when it has been idle for a while.
        #[arg(long)]
        foreground: bool,
        /// With --foreground, end after this many idle looks, five seconds apart; the default is 12.
        #[arg(long)]
        idle_exit: Option<u32>,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Show the monitored quantities, the balances and the verdict of a job; nothing reaches the
    /// compute host.
    Watch {
        doc: PathBuf,
        /// The job to watch, for example q-001; by default the active or newest job.
        #[arg(long)]
        job: Option<String>,
        /// Show it again every few seconds until the job has ended.
        #[arg(long)]
        follow: bool,
        /// With --follow, the seconds between two looks.
        #[arg(long, default_value_t = 5)]
        poll: u64,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print a piece of the solver log of a job, with host paths hidden, and where the next piece
    /// begins; nothing reaches the compute host.
    Log {
        doc: PathBuf,
        /// The job whose log to read, for example q-001.
        job: String,
        /// The byte to start from; the next offset of the previous piece.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// The most lines to print.
        #[arg(long, default_value_t = 40)]
        lines: usize,
        /// Print one JSON report on stdout instead of text.
        #[arg(long)]
        json: bool,
    },
}
/// The command tree is large enough that parsing it overflows the 1 MiB main-thread stack on Windows,
/// so the whole program runs on a thread with a roomier one.
const CLI_STACK_BYTES: usize = 16 * 1024 * 1024;

fn main() {
    let worker = std::thread::Builder::new()
        .stack_size(CLI_STACK_BYTES)
        .spawn(run);
    match worker.map(std::thread::JoinHandle::join) {
        Ok(Ok(())) => {}
        _ => std::process::exit(101),
    }
}

fn run() {
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
        Commands::Keys { study, json } => cmd::keys::execute(&study, json),
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
        Commands::Pack {
            doc,
            out,
            redact_host,
            json,
        } => cmd::document::pack(&doc, &out, redact_host, json),
        Commands::Unpack { packed, dir, json } => cmd::document::unpack(&packed, &dir, json),
        Commands::Status {
            doc,
            scenario,
            json,
        } => cmd::repro::status(&doc, scenario.as_deref(), json),
        Commands::Runs { doc, json } => cmd::repro::runs(&doc, json),
        Commands::ShowResult { doc, run, json } => cmd::repro::result(&doc, run.as_deref(), json),
        Commands::Provenance { doc, run, json } => {
            cmd::repro::provenance(&doc, run.as_deref(), json)
        }
        Commands::Compare {
            doc,
            a,
            b,
            abs_tol,
            rel_tol,
            json,
        } => cmd::repro::compare(&doc, &a, &b, abs_tol, rel_tol, json),
        Commands::Solve {
            doc,
            key,
            profile,
            hosts,
            validate,
            scenario,
            remake_pruned,
            json,
        } => cmd::repro::solve(
            &doc,
            &cmd::repro::Host {
                key,
                profile,
                hosts,
                validate,
            },
            scenario.as_deref(),
            remake_pruned,
            json,
        ),
        Commands::Rerun {
            doc,
            run,
            key,
            profile,
            hosts,
            abs_tol,
            rel_tol,
            allow_tool_change,
            json,
        } => cmd::repro::rerun(&cmd::repro::RerunArgs {
            doc,
            run,
            host: cmd::repro::Host {
                key,
                profile,
                hosts,
                validate: false,
            },
            abs_tol,
            rel_tol,
            allow_tool_change,
            json,
        }),
        Commands::Update {
            docs,
            items,
            order,
            plan,
            key,
            profile,
            hosts,
            validate,
            json,
        } => cmd::repro::update(&cmd::repro::UpdateArgs {
            docs,
            items,
            order,
            plan,
            key,
            profile,
            hosts,
            validate,
            json,
        }),
        Commands::Scenario { action } => match action {
            ScenarioAction::Set {
                doc,
                id,
                parent,
                sets,
                unsets,
                name,
                json,
            } => cmd::scenario::set(&cmd::scenario::SetArgs {
                doc,
                id,
                parent,
                sets,
                unsets,
                name,
                json,
            }),
            ScenarioAction::List { doc, json } => cmd::scenario::list(&doc, json),
            ScenarioAction::Show { doc, id, json } => cmd::scenario::show(&doc, &id, json),
            ScenarioAction::Diff { doc, a, b, json } => cmd::scenario::diff(&doc, &a, &b, json),
            ScenarioAction::Label {
                doc,
                id,
                name,
                pin,
                unpin,
                notes,
                json,
            } => cmd::scenario::label(&cmd::scenario::LabelArgs {
                doc,
                id,
                name,
                pin,
                unpin,
                notes,
                json,
            }),
        },
        Commands::Sweep { action } => match action {
            SweepAction::Plan { doc, spec, json } => cmd::sweep::plan(&doc, &spec, json),
            SweepAction::Create { doc, spec, json } => cmd::sweep::create(&doc, &spec, json),
            SweepAction::Preflight { doc, sweep, json } => {
                cmd::sweep::preflight(&doc, &sweep, json)
            }
        },
        Commands::Disk { doc, json } => cmd::disk::disk(&doc, json),
        Commands::Prune {
            doc,
            scenarios,
            apply,
            json,
        } => cmd::disk::prune_results(&doc, &scenarios, apply, json),
        Commands::Monitors {
            doc,
            run,
            job,
            json,
        } => cmd::watch::table(&doc, run.as_deref(), job.as_deref(), json),
        Commands::Queue { action } => match action {
            QueueAction::Add {
                doc,
                scenario,
                sweep,
                continue_run,
                remake_pruned,
                json,
            } => cmd::queue::add(&cmd::queue::AddArgs {
                doc,
                scenario,
                sweep,
                continue_run,
                remake_pruned,
                json,
            }),
            QueueAction::List { doc, json } => cmd::queue::list(&doc, json),
            QueueAction::Reorder { doc, job, to, json } => {
                cmd::queue::reorder(&doc, &job, to, json)
            }
            QueueAction::Cancel { doc, job, json } => cmd::queue::cancel(&doc, &job, json),
            QueueAction::Stop { doc, job, json } => cmd::queue::stop(&doc, &job, json),
            QueueAction::Watch {
                doc,
                job,
                follow,
                poll,
                json,
            } => cmd::watch::watch(&doc, job.as_deref(), follow, poll, json),
            QueueAction::Log {
                doc,
                job,
                offset,
                lines,
                json,
            } => cmd::watch::log_of(&doc, &job, offset, lines, json),
            QueueAction::Drain {
                doc,
                key,
                profile,
                hosts,
                socket_dir,
                foreground,
                idle_exit,
                json,
            } => cmd::queue::drain(&cmd::queue::DrainArgs {
                doc,
                key,
                profile,
                hosts,
                socket_dir,
                foreground,
                idle_exit,
                json,
            }),
        },
        Commands::Mcp {
            workspace,
            key,
            profile,
            hosts,
            attached_jobs,
        } => cfdcraft_mcp::serve_stdio_blocking(cfdcraft_mcp::McpConfig {
            workspace,
            key,
            hosts,
            profile,
            queued_jobs: !attached_jobs,
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
