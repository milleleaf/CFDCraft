//! Every command that reaches the compute host is a fixed template filled with
//! pieces that were checked first: a run id, a relative path, a plain folder and
//! numbers. No piece can hold a quote, so the single quotes around it cannot be
//! closed from inside. Free text (the text of a file) only ever travels on
//! standard input.

use crate::host::{
    ContainerState, OWNED_PREFIX, RemoteFile, TimeDir, is_owned, is_time_name, valid_rel,
    valid_run_id,
};

/// The one work directory a run folder is ever removed from.
pub const PROJECT_WORK_DIR: &str = "/data/cfdcraft/work";

/// The most bytes one `tail` call returns.
pub const MAX_TAIL: u64 = 1 << 20;
/// The most bytes of text one read or write moves.
pub const MAX_TEXT: usize = 1 << 20;
/// The longest stop grace period.
pub const MAX_GRACE: u32 = 300;
/// A listing longer than this is cut.
pub const MAX_LISTING_LINES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("invalid {what}")]
    Invalid { what: &'static str },
    #[error("{what} is out of range")]
    Bounds { what: &'static str },
    #[error("text of {len} bytes is over the limit")]
    TextTooLarge { len: usize },
    #[error("unreadable {what}")]
    Unreadable { what: &'static str },
}

/// An absolute folder of plain characters: `[A-Za-z0-9/._-]`, at most 128, no
/// `..`, no empty parts and no trailing slash.
pub fn plain_dir(dir: &str) -> bool {
    dir.len() <= 128
        && dir.starts_with('/')
        && !dir.ends_with('/')
        && !dir.contains("..")
        && !dir.contains("//")
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
}

/// Refuse text over the limit.
pub fn check_text(text: &str) -> Result<(), CommandError> {
    if text.len() > MAX_TEXT {
        return Err(CommandError::TextTooLarge { len: text.len() });
    }
    Ok(())
}

fn run_dir(work: &str, run_id: &str) -> Result<String, CommandError> {
    if !plain_dir(work) {
        return Err(CommandError::Invalid {
            what: "work directory",
        });
    }
    if !valid_run_id(run_id) {
        return Err(CommandError::Invalid { what: "run id" });
    }
    Ok(format!("{work}/{run_id}"))
}

fn run_path(work: &str, run_id: &str, rel: &str) -> Result<String, CommandError> {
    let dir = run_dir(work, run_id)?;
    if !valid_rel(rel) {
        return Err(CommandError::Invalid { what: "path" });
    }
    Ok(format!("{dir}/{rel}"))
}

/// The start of a shell command that works on one run file. It sets `p` to the real path of the
/// file, every link followed, and goes on only when `p` lies inside the run folder. The solver runs
/// as the host user with the run folder mounted writable, so a link it makes must never lead a
/// read or a write outside the folder. Both paths are built from checked pieces, so the single
/// quotes cannot be closed from inside.
fn guarded(run: &str, path: &str) -> String {
    format!("d=$(readlink -f '{run}') && p=$(readlink -f '{path}') && case \"$p\" in \"$d\"/*) ")
}

/// Like [`guarded`] for a file that may not exist yet. `readlink -f` of a missing last part fails
/// on some systems, so the folder that holds the file is resolved instead, and an existing link in
/// the last place is refused. `p` is that folder's real path and the file name.
fn guarded_for_write(run: &str, path: &str) -> String {
    let (parent, name) = path.rsplit_once('/').unwrap_or((path, ""));
    format!(
        "d=$(readlink -f '{run}') && q=$(readlink -f '{parent}') && p=\"$q/{name}\" && [ ! -L \"$p\" ] && case \"$q\" in \"$d\"|\"$d\"/*) "
    )
}

fn owned(run_id: &str) -> Result<(), CommandError> {
    if is_owned(run_id) {
        Ok(())
    } else {
        Err(CommandError::Invalid {
            what: "container name",
        })
    }
}

/// The bytes of a run file after `offset`, at most `cap` of them. `tail -c +N` is
/// one-based, so an offset of `n` bytes already read starts at `n + 1`. A path that leaves the run
/// folder through a link, or is not a plain file, gives nothing.
pub fn tail_command(
    work: &str,
    run_id: &str,
    rel: &str,
    offset: u64,
    cap: u64,
) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    let path = run_path(work, run_id, rel)?;
    if cap == 0 {
        return Err(CommandError::Bounds { what: "byte cap" });
    }
    let cap = cap.min(MAX_TAIL);
    let start = offset
        .checked_add(1)
        .ok_or(CommandError::Bounds { what: "offset" })?;
    let guard = guarded(&run, &path);
    Ok(format!(
        "{guard}[ -f \"$p\" ] && tail -c +{start} \"$p\" 2>/dev/null | head -c {cap};; esac || true"
    ))
}

/// Every file under `dir` with its size in bytes, as `wc -c` prints it.
pub fn files_command(work: &str, run_id: &str, dir: &str) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    if !valid_rel(dir) {
        return Err(CommandError::Invalid { what: "path" });
    }
    Ok(format!(
        "cd '{run}' 2>/dev/null && find '{dir}' -type f -exec wc -c {{}} + 2>/dev/null || true"
    ))
}

/// The numeric folders of a run with their sizes in kilobytes. A parallel run writes its times
/// into `processor*/<time>`, so those are added up per time name and listed after the folders at
/// the top; the largest line is then what one write of all the processes takes.
pub fn time_dirs_command(work: &str, run_id: &str) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    Ok(format!(
        "cd '{run}' 2>/dev/null && {{ du -sk [0-9]* 2>/dev/null; du -sk processor*/[0-9]* 2>/dev/null | awk '{{n=split($2,a,\"/\"); s[a[n]]+=$1}} END{{for(k in s) print s[k], k}}'; }} || true"
    ))
}

/// The free space of the work disk.
pub fn df_command(work: &str) -> Result<String, CommandError> {
    if !plain_dir(work) {
        return Err(CommandError::Invalid {
            what: "work directory",
        });
    }
    Ok(format!("df -Pk '{work}'"))
}

/// A run file as text. One byte more than the text limit is read, so the caller
/// can tell a file that is too large from one that is exactly at the limit.
pub fn read_text_command(work: &str, run_id: &str, rel: &str) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    let path = run_path(work, run_id, rel)?;
    let guard = guarded(&run, &path);
    Ok(format!(
        "{guard}[ -f \"$p\" ] && head -c {} \"$p\";; *) exit 1;; esac",
        MAX_TEXT + 1
    ))
}

/// Write standard input to a temporary name and rename it over the file, so a
/// reader never sees half a file. The temporary name is made new (a link left there is removed,
/// never written through), and a path that leaves the run folder through a link is refused. The
/// text is not part of the command.
pub fn write_text_command(work: &str, run_id: &str, rel: &str) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    let path = run_path(work, run_id, rel)?;
    let guard = guarded_for_write(&run, &path);
    Ok(format!(
        "{guard}rm -f \"$p.tmp\" && (set -C; cat > \"$p.tmp\") && mv \"$p.tmp\" \"$p\";; *) exit 1;; esac"
    ))
}

/// The run folder as a tar stream on standard output, without the per-process
/// folders.
pub fn download_command(work: &str, run_id: &str) -> Result<String, CommandError> {
    let run = run_dir(work, run_id)?;
    Ok(format!("tar -C '{run}' --exclude='processor*' -cf - ."))
}

/// Remove the run folder `<work>/<run id>`, only when `work` is the project work directory.
pub fn remove_run_dir_command(work: &str, run_id: &str) -> Result<String, CommandError> {
    if work != PROJECT_WORK_DIR {
        return Err(CommandError::Invalid {
            what: "work directory",
        });
    }
    remove_run_dir_in(work, run_id)
}

/// Like [`remove_run_dir_command`] for any plain work directory, so the command can be run for
/// real in a test folder. The host only ever calls the checked one.
///
/// The id must name a run of this tool (`cfdcraft-` and plain characters, so no slash, no `..`, no
/// quote). The command goes on only when the run folder is not a link and its real path is exactly
/// `<real work>/<id>`; a link, a file, the work directory and anything else is refused with a
/// non-zero status, and a folder that is not there is a success. `rm` does not follow links inside.
pub fn remove_run_dir_in(work: &str, run_id: &str) -> Result<String, CommandError> {
    owned(run_id).map_err(|_| CommandError::Invalid { what: "run id" })?;
    if run_id.len() <= OWNED_PREFIX.len() {
        return Err(CommandError::Invalid { what: "run id" });
    }
    let run = run_dir(work, run_id)?;
    Ok(format!(
        "r='{run}'; [ ! -L \"$r\" ] || exit 1; [ -e \"$r\" ] || exit 0; w=$(readlink -f '{work}') && p=$(readlink -f \"$r\") && [ \"$p\" = \"$w/{run_id}\" ] && [ -d \"$p\" ] && rm -rf -- \"$p\""
    ))
}

const STATE_FORMAT: &str = "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}";

/// Docker arguments (after the socket): the state of one container.
pub fn inspect_args(run_id: &str) -> Result<Vec<String>, CommandError> {
    owned(run_id)?;
    Ok(["inspect", "--format", STATE_FORMAT, run_id]
        .map(String::from)
        .to_vec())
}

/// Docker arguments: stop one container after a grace period.
pub fn stop_args(run_id: &str, grace: u32) -> Result<Vec<String>, CommandError> {
    owned(run_id)?;
    if grace > MAX_GRACE {
        return Err(CommandError::Bounds {
            what: "grace period",
        });
    }
    Ok(vec![
        "stop".into(),
        "--time".into(),
        grace.to_string(),
        run_id.into(),
    ])
}

/// Docker arguments: remove one container by its exact name.
pub fn rm_args(run_id: &str) -> Result<Vec<String>, CommandError> {
    owned(run_id)?;
    Ok(["rm", "-f", run_id].map(String::from).to_vec())
}

/// Docker arguments: list the container of this name that also carries this
/// run's label, whatever its state.
pub fn ps_label_args(run_id: &str) -> Result<Vec<String>, CommandError> {
    owned(run_id)?;
    let label = format!("cfdcraft.run={run_id}");
    Ok(vec![
        "ps".into(),
        "-a".into(),
        "--filter".into(),
        format!("label={label}"),
        "--filter".into(),
        format!("name=^{run_id}$"),
        "--format".into(),
        "{{.Names}}".into(),
    ])
}

/// The free kilobytes from `df -Pk`: the fourth column of the first line that has
/// six columns and a number there. A header or a wrapped row has neither.
pub fn parse_df(text: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            return None;
        }
        fields[3].parse().ok()
    })
}

/// `wc -c` lines: a size and a path. The total line and any line that is not a
/// size and an allowed path are skipped; a leading `./` is dropped.
pub fn parse_wc_lines(text: &str) -> Vec<RemoteFile> {
    text.lines()
        .take(MAX_LISTING_LINES)
        .filter_map(|line| {
            let (size, name) = line.trim_start().split_once(char::is_whitespace)?;
            let name = name.trim();
            if name == "total" {
                return None;
            }
            let rel = name.strip_prefix("./").unwrap_or(name);
            if !valid_rel(rel) {
                return None;
            }
            Some(RemoteFile {
                rel: rel.to_string(),
                size: size.parse().ok()?,
            })
        })
        .collect()
}

/// `du -sk` lines: a size and a folder. Only numeric folder names are kept.
pub fn parse_du_lines(text: &str) -> Vec<TimeDir> {
    // A time folder of a parallel run is listed once for the case and once, summed, for its
    // processor folders: one entry per name, sizes added.
    let mut dirs: Vec<TimeDir> = Vec::new();
    for line in text.lines().take(MAX_LISTING_LINES) {
        let Some((kb, name)) = line.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim();
        let Ok(kb) = kb.parse::<u64>() else {
            continue;
        };
        if !is_time_name(name) {
            continue;
        }
        match dirs.iter_mut().find(|d| d.name == name) {
            Some(d) => d.kb = d.kb.saturating_add(kb),
            None => dirs.push(TimeDir {
                name: name.to_string(),
                kb,
            }),
        }
    }
    dirs
}

/// The three fields printed for `STATE_FORMAT`: status, exit code, out-of-memory.
pub fn parse_inspect(text: &str) -> Result<ContainerState, CommandError> {
    let bad = CommandError::Unreadable {
        what: "container state",
    };
    let mut fields = text.split_whitespace();
    let status = fields.next().ok_or(bad.clone())?;
    match status {
        "running" | "restarting" | "paused" | "created" => Ok(ContainerState::Running),
        "exited" | "dead" => {
            let code = fields
                .next()
                .and_then(|c| c.parse().ok())
                .ok_or(bad.clone())?;
            let oom_killed = match fields.next() {
                Some("true") => true,
                Some("false") => false,
                _ => return Err(bad),
            };
            Ok(ContainerState::Exited { code, oom_killed })
        }
        _ => Err(bad),
    }
}
