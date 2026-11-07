//! An in-memory compute host. It checks every id and path the way the real host
//! does, so a call that is refused here is refused there, and it lets a test play
//! the part of the solver.

use crate::host::{
    ContainerState, Host, HostError, RemoteFile, TimeDir, is_owned, is_time_name, valid_rel,
    valid_run_id,
};
use cfdcraft_executor::{Holder, ReleaseOutcome};
use std::{collections::BTreeMap, path::Path};

const INFRA: &str = "CFD-INFRA-005";
const MAX_TEXT: usize = 1 << 20;

type Key = (String, String);

pub struct FakeBox {
    files: BTreeMap<Key, Vec<u8>>,
    containers: BTreeMap<String, ContainerState>,
    holder: Option<Holder>,
    free_kb: u64,
    fail: u32,
    obeys_stop: bool,
    lock_only: bool,
    solver_log: String,
    calls: Vec<String>,
}

impl Default for FakeBox {
    fn default() -> Self {
        Self::new()
    }
}

fn refused(reason: &'static str) -> HostError {
    HostError::Refused { reason }
}

fn key(run_id: &str, rel: &str) -> Key {
    (run_id.to_string(), rel.to_string())
}

fn load_case(
    files: &mut BTreeMap<Key, Vec<u8>>,
    run_id: &str,
    dir: &Path,
    prefix: &str,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if kind.is_dir() {
            load_case(files, run_id, &entry.path(), &rel)?;
        } else if kind.is_file() {
            files.insert(key(run_id, &rel), std::fs::read(entry.path())?);
        }
    }
    Ok(())
}

impl FakeBox {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            containers: BTreeMap::new(),
            holder: None,
            free_kb: 1 << 32,
            fail: 0,
            obeys_stop: false,
            lock_only: false,
            solver_log: "log.simpleFoam".to_string(),
            calls: Vec::new(),
        }
    }

    /// A box whose pretend solver writes its log under another name.
    pub fn with_solver_log(name: &str) -> Self {
        Self {
            solver_log: name.to_string(),
            ..Self::new()
        }
    }

    /// Add text to the end of a run file, making it if needed: the solver's part.
    pub fn append(&mut self, run_id: &str, rel: &str, text: &str) {
        self.files
            .entry(key(run_id, rel))
            .or_default()
            .extend_from_slice(text.as_bytes());
    }

    pub fn set_container(&mut self, run_id: &str, state: ContainerState) {
        if state == ContainerState::None {
            self.containers.remove(run_id);
        } else {
            self.containers.insert(run_id.to_string(), state);
        }
    }

    pub fn set_free_kb(&mut self, kb: u64) {
        self.free_kb = kb;
    }

    /// The next `n` calls return `Unreachable`, then the box recovers.
    pub fn fail_next(&mut self, n: u32) {
        self.fail = n;
    }

    /// When on, a controlDict written with `stopAt writeNow` or `noWriteNow` is
    /// obeyed by the pretend solver.
    pub fn solver_obeys_stop(&mut self, on: bool) {
        self.obeys_stop = on;
    }

    pub fn set_holder(&mut self, holder: Option<Holder>) {
        self.holder = holder;
    }

    /// While on, a `start` takes the lock, copies nothing, makes no container and then fails as
    /// if the connection dropped: a start that died half way. Off again with `false`.
    pub fn start_leaves_lock_only(&mut self, on: bool) {
        self.lock_only = on;
    }

    /// Removes the container of a run without a trace in the call log, as if it had never been
    /// made or had been removed behind the caller's back.
    pub fn drop_container(&mut self, run_id: &str) {
        self.containers.remove(run_id);
    }

    /// Every call made so far, in order, one short line each.
    pub fn calls(&self) -> &[String] {
        &self.calls
    }

    pub fn file(&self, run_id: &str, rel: &str) -> Option<&[u8]> {
        self.files.get(&key(run_id, rel)).map(Vec::as_slice)
    }

    /// Log the call, then play a scripted connection failure if one is due.
    fn enter(&mut self, call: String) -> Result<(), HostError> {
        self.calls.push(call);
        if self.fail > 0 {
            self.fail -= 1;
            return Err(HostError::Unreachable);
        }
        Ok(())
    }

    fn run_files<'a>(&'a self, run_id: &'a str) -> impl Iterator<Item = (&'a str, &'a Vec<u8>)> {
        self.files
            .iter()
            .filter(move |((id, _), _)| id == run_id)
            .map(|((_, rel), bytes)| (rel.as_str(), bytes))
    }

    /// The pretend solver reads a stop request: it ends the log and, for
    /// `writeNow`, writes the current time.
    fn obey_stop(&mut self, run_id: &str, control_dict: &str) {
        let asks = |word: &str| {
            control_dict.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with("stopAt") && l.contains(word)
            })
        };
        let write_now = asks("writeNow");
        if !(write_now || asks("noWriteNow")) {
            return;
        }
        if self.containers.get(run_id) != Some(&ContainerState::Running) {
            return;
        }
        let solver_log = self.solver_log.clone();
        let time = self.file(run_id, &solver_log).and_then(|log| {
            String::from_utf8_lossy(log).lines().rev().find_map(|l| {
                l.trim()
                    .strip_prefix("Time = ")
                    .map(|t| t.trim().to_string())
            })
        });
        self.append(run_id, &solver_log, "End\n");
        if let Some(t) = time.filter(|t| write_now && valid_rel(t)) {
            self.append(run_id, &format!("{t}/U"), "fake\n");
            self.append(run_id, &format!("{t}/p"), "fake\n");
        }
        self.set_container(
            run_id,
            ContainerState::Exited {
                code: 0,
                oom_killed: false,
            },
        );
    }
}

impl Host for FakeBox {
    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        self.enter(format!("start {run_id}"))?;
        if !is_owned(run_id) {
            return Err(refused("invalid run id"));
        }
        if doc.is_empty()
            || doc.len() > 40
            || !doc.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(refused("invalid document label"));
        }
        if np == 0 {
            return Err(refused("invalid process count"));
        }
        let running = self
            .containers
            .values()
            .any(|s| *s == ContainerState::Running);
        if self.holder.is_some() || running {
            return Err(HostError::Busy { stale: false });
        }
        if self.lock_only {
            self.holder = Holder::new(run_id, run_id, 0, 3600, "fake").ok();
            return Err(HostError::Unreachable);
        }
        let mut loaded = BTreeMap::new();
        load_case(&mut loaded, run_id, case, "").map_err(|_| HostError::Failed { class: INFRA })?;
        self.files.extend(loaded);
        self.holder = Holder::new(run_id, run_id, 0, 3600, "fake").ok();
        self.containers
            .insert(run_id.to_string(), ContainerState::Running);
        Ok(())
    }

    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        self.enter(format!("inspect {run_id}"))?;
        if !is_owned(run_id) {
            return Err(refused("invalid run id"));
        }
        Ok(self
            .containers
            .get(run_id)
            .cloned()
            .unwrap_or(ContainerState::None))
    }

    fn holder(&mut self) -> Result<Option<Holder>, HostError> {
        self.enter("holder".to_string())?;
        Ok(self.holder.clone())
    }

    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError> {
        self.enter(format!("tail {run_id} {rel} {offset}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        if !valid_rel(rel) {
            return Err(refused("invalid path"));
        }
        if max == 0 {
            return Err(refused("byte cap"));
        }
        let Some(bytes) = self.file(run_id, rel) else {
            return Ok(Vec::new());
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let cap = usize::try_from(max.min(1 << 20)).unwrap_or(1 << 20);
        let end = start.saturating_add(cap).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError> {
        self.enter(format!("files {run_id} {dir}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        if !valid_rel(dir) {
            return Err(refused("invalid path"));
        }
        let below = format!("{}/", dir.trim_end_matches('/'));
        Ok(self
            .run_files(run_id)
            .filter(|(rel, _)| dir == "." || rel.starts_with(&below))
            .map(|(rel, bytes)| RemoteFile {
                rel: rel.to_string(),
                size: bytes.len() as u64,
            })
            .collect())
    }

    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError> {
        self.enter(format!("time_dirs {run_id}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        // The folders at the top, then the times of the processor folders added up per name, as
        // the real host lists them.
        let mut sizes: BTreeMap<&str, u64> = BTreeMap::new();
        let mut procs: BTreeMap<&str, u64> = BTreeMap::new();
        for (rel, bytes) in self.run_files(run_id) {
            let Some((first, rest)) = rel.split_once('/') else {
                continue;
            };
            let (into, time) = if first.starts_with("processor") {
                (&mut procs, rest.split_once('/').map(|(t, _)| t))
            } else {
                (&mut sizes, Some(first))
            };
            if let Some(time) = time.filter(|t| is_time_name(t)) {
                *into.entry(time).or_default() += bytes.len() as u64;
            }
        }
        Ok(sizes
            .into_iter()
            .chain(procs)
            .map(|(name, bytes)| TimeDir {
                name: name.to_string(),
                kb: bytes.div_ceil(1024),
            })
            .collect())
    }

    fn free_kb(&mut self) -> Result<u64, HostError> {
        self.enter("free_kb".to_string())?;
        Ok(self.free_kb)
    }

    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError> {
        self.enter(format!("read_text {run_id} {rel}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        if !valid_rel(rel) {
            return Err(refused("invalid path"));
        }
        match self.file(run_id, rel) {
            Some(bytes) => Ok(String::from_utf8_lossy(bytes).into_owned()),
            None => Err(refused("file not found")),
        }
    }

    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError> {
        self.enter(format!("write_text {run_id} {rel}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        if !valid_rel(rel) {
            return Err(refused("invalid path"));
        }
        if text.len() > MAX_TEXT {
            return Err(refused("text too large"));
        }
        self.files
            .insert(key(run_id, rel), text.as_bytes().to_vec());
        if self.obeys_stop && rel == "system/controlDict" {
            self.obey_stop(run_id, text);
        }
        Ok(())
    }

    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError> {
        self.enter(format!("stop_container {run_id} {grace_seconds}"))?;
        if !is_owned(run_id) {
            return Err(refused("invalid run id"));
        }
        if grace_seconds > 300 {
            return Err(refused("invalid grace period"));
        }
        if self.containers.get(run_id) == Some(&ContainerState::Running) {
            self.set_container(
                run_id,
                ContainerState::Exited {
                    code: 143,
                    oom_killed: false,
                },
            );
        }
        Ok(())
    }

    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError> {
        self.enter(format!("download {run_id}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        let failed = |_| HostError::Failed { class: INFRA };
        std::fs::create_dir_all(dest).map_err(failed)?;
        for (rel, bytes) in self.run_files(run_id) {
            if rel.split('/').any(|part| part.starts_with("processor")) {
                continue;
            }
            let target = dest.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(failed)?;
            }
            std::fs::write(&target, bytes).map_err(failed)?;
        }
        Ok(())
    }

    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError> {
        self.enter(format!("remove_container {run_id}"))?;
        if !is_owned(run_id) {
            return Err(refused("invalid run id"));
        }
        self.containers.remove(run_id);
        Ok(())
    }

    fn remove_run_dir(&mut self, run_id: &str) -> Result<(), HostError> {
        self.enter(format!("remove_run_dir {run_id}"))?;
        if !is_owned(run_id) {
            return Err(refused("invalid run id"));
        }
        self.files.retain(|(id, _), _| id != run_id);
        Ok(())
    }

    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError> {
        self.enter(format!("release {run_id}"))?;
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        match &self.holder {
            None => Ok(ReleaseOutcome::Absent),
            Some(h) if h.run_id == run_id => {
                self.holder = None;
                Ok(ReleaseOutcome::Released)
            }
            Some(_) => Ok(ReleaseOutcome::NotOurs),
        }
    }
}
