//! The run side of the backend trait: a backend that has no judge never passes a run, the default
//! results view is the native one, and the folder-backed view of a run stays inside its folder.

use cfdcraft_backend::{
    Backend, CaseTree, CompileFailure, CompileOptions, DirRunFiles, Family, JudgeMode, Refusal,
    RunFiles, RunPlan,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy, Study, resolve};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

const SOURCES: [(&str, &str); 0] = [];

/// A backend that implements every required method and nothing of the run side.
struct NoJudge;

impl Backend for NoJudge {
    fn id(&self) -> &'static str {
        "nojudge-1"
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        None
    }
    fn preview(&self) -> bool {
        true
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::unrestricted()
    }
    fn compile(
        &self,
        _study: &ResolvedStudy,
        _options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        Ok(CaseTree { files: Vec::new() })
    }
    fn scan(&self, _tree: &CaseTree) -> Vec<Refusal> {
        Vec::new()
    }
    fn read_back(&self, _study: &ResolvedStudy, _tree: &CaseTree) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn export(
        &self,
        _study: &ResolvedStudy,
        tree: &CaseTree,
        _hash: &str,
        _options: &CompileOptions,
    ) -> CaseTree {
        tree.clone()
    }
    fn run_plan(&self, _study: &ResolvedStudy, _np: u32) -> RunPlan {
        RunPlan {
            solver: "none".into(),
            steps: Vec::new(),
            solver_log: "log.none".into(),
            judged_logs: vec!["log.none".into()],
            final_fields: Vec::new(),
            transient: false,
            buoyant: false,
            energy_field: "T".to_owned(),
            pressure_field: String::new(),
            residual_checks: Vec::new(),
            potential: false,
        }
    }
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &SOURCES
    }
}

/// The files of a run held in memory, by name.
struct MapFiles(BTreeMap<String, String>);

impl RunFiles for MapFiles {
    fn read_text(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }
    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        let mut names: Vec<String> = self
            .0
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
    fn size(&self, rel: &str) -> Option<u64> {
        self.0.get(rel).map(|t| t.len() as u64)
    }
}

fn study() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY).unwrap()).unwrap()
}

#[test]
fn a_backend_without_a_judge_never_passes() {
    let study = study();
    let empty = MapFiles(BTreeMap::new());
    // A run that looks perfect by every convention: logs that end, a final time folder with fields.
    let clean = MapFiles(
        [
            ("log.none", "Time = 5000\nEnd\n"),
            ("5000/U", "internalField uniform (0 0 0);\n"),
            ("5000/p", "internalField uniform 0;\n"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    );
    let modes = [
        JudgeMode::Run,
        JudgeMode::Smoke,
        JudgeMode::Finished,
        JudgeMode::Stopped {
            time: "5000".to_owned(),
        },
    ];
    for files in [&empty, &clean] {
        for mode in &modes {
            let outcome = NoJudge.judge(files, &study, mode.clone());
            assert_eq!(outcome.verdict, "failed", "{mode:?}");
            assert_eq!(outcome.reasons.len(), 1, "{mode:?}");
        }
    }
    assert!(NoJudge.convergence(&clean, &study).is_none());
}

/// A results view over a few named texts.
struct Named(Vec<(String, String)>);

impl ResultFiles for Named {
    fn read(&self, rel: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(k, _)| k == rel)
            .map(|(_, v)| v.clone())
    }
    fn list(&self, _rel_dir: &str) -> Vec<String> {
        self.0.iter().map(|(k, _)| k.clone()).collect()
    }
}

#[test]
fn the_results_view_defaults_to_the_native_files() {
    let input = Named(vec![
        ("a.csv".to_owned(), "1,2\n".to_owned()),
        ("b.csv".to_owned(), "3,4\n".to_owned()),
    ]);
    let view = NoJudge.results(Box::new(input), &study());
    assert_eq!(view.read("a.csv").as_deref(), Some("1,2\n"));
    assert_eq!(view.read("b.csv").as_deref(), Some("3,4\n"));
    assert_eq!(view.read("c.csv"), None);
    assert_eq!(view.list(""), ["a.csv", "b.csv"]);
}

fn fresh(tag: &str) -> PathBuf {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("judge-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(dir: &Path, name: &str, bytes: &[u8]) {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn the_folder_view_refuses_links_and_outside_names() {
    let base = fresh("refuse");
    let root = base.join("run");
    put(&root, "log.solver", b"Time = 1\nEnd\n");
    put(&root, "20/U", b"x");
    put(&base, "outside.txt", b"secret");
    let files = DirRunFiles::new(&root);

    // The files of the folder read normally, and a folder has no size.
    assert_eq!(
        files.read_text("log.solver").as_deref(),
        Some("Time = 1\nEnd\n")
    );
    assert_eq!(files.size("20/U"), Some(1));
    assert_eq!(files.size("20"), None);

    // A name that leaves the folder, or is absolute, reads nothing.
    let absolute = base.join("outside.txt").to_string_lossy().into_owned();
    assert_eq!(files.read_text("../outside.txt"), None);
    assert_eq!(files.read_text("20/../../outside.txt"), None);
    assert_eq!(files.read_text(&absolute), None);
    assert_eq!(files.size("../outside.txt"), None);
    assert_eq!(files.size(&absolute), None);
    assert!(files.list("..").is_empty());
    assert!(files.list(&base.to_string_lossy()).is_empty());

    // A missing folder lists nothing; a folder lists its names sorted.
    assert!(files.list("nothing").is_empty());
    put(&root, "b", b"");
    put(&root, "a", b"");
    put(&root, "10/U", b"y");
    assert_eq!(files.list(""), ["10", "20", "a", "b", "log.solver"]);

    // A link is refused whatever it points at, and does not appear in a listing.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(base.join("outside.txt"), root.join("log.link")).unwrap();
        symlink(&base, root.join("up")).unwrap();
        assert_eq!(files.read_text("log.link"), None);
        assert_eq!(files.size("log.link"), None);
        assert_eq!(files.read_text("up/outside.txt"), None);
        assert!(files.list("up").is_empty());
        assert!(!files.list("").iter().any(|n| n == "log.link" || n == "up"));
    }
}

#[test]
fn the_folder_view_reads_the_end_of_a_file_over_the_cap() {
    let root = fresh("cap");
    put(&root, "big.log", b"0123456789abcdef\n");
    put(&root, "small.log", b"abc");
    put(&root, "wide.log", "\u{e9}\u{e9}\u{e9}".as_bytes());
    put(&root, "binary.bin", &[0xff, 0xfe, 0x00]);
    let files = DirRunFiles::with_cap(&root, 8);
    // The last 8 bytes.
    assert_eq!(files.read_text("big.log").as_deref(), Some("9abcdef\n"));
    assert_eq!(files.read_text("small.log").as_deref(), Some("abc"));
    // A cut inside a character starts at the next whole one: 5 of 6 bytes leave two letters.
    let files = DirRunFiles::with_cap(&root, 5);
    assert_eq!(files.read_text("wide.log").as_deref(), Some("\u{e9}\u{e9}"));
    // Text that is not text is refused.
    assert_eq!(files.read_text("binary.bin"), None);
}
