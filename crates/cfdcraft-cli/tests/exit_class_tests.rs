use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cfdcraft-backend-esi/tests/golden")
        .join(name)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Every file under `dir` as (`/`-separated relative path, bytes), sorted.
fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn json(o: &std::process::Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

fn names(report: &Value) -> Vec<String> {
    report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_owned())
        .collect()
}

fn channel_without_outlet() -> String {
    let outlet = "    \"outlet\": {\n      \"faces\": [\"XMax\"],\n      \"kind\": \"PressureOutlet\"\n    },\n";
    let broken = CHANNEL.replace(outlet, "");
    assert_ne!(broken, CHANNEL, "the fixture changed shape");
    broken
}

fn write_study(dir: &Path, text: &str) -> PathBuf {
    let study = dir.join("study.json");
    std::fs::write(&study, text).unwrap();
    study
}

#[test]
fn compile_passes_with_class_0_and_writes_exactly_the_golden_case() {
    let dir = scratch("compile-ok");
    let study = write_study(&dir, CAVITY_V0);
    let out = dir.join("case");
    let o = run(&["compile", s(&study), s(&out)]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains(&format!("study hash: {CAVITY_HASH}")),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("Migrated from schema 0"));
    assert_eq!(read_tree(&out), read_tree(&golden("cavity-v2512")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compile_json_prints_one_report_and_nothing_else_on_stdout() {
    let dir = scratch("compile-json");
    let study = write_study(&dir, CAVITY_V0);
    let out = dir.join("case");
    let o = run(&["compile", "--json", s(&study), s(&out)]);
    assert_eq!(o.status.code(), Some(0));
    let report = json(&o);
    assert_eq!(report["exit_class"], 0);
    assert_eq!(report["status"], "ok");
    let hash = report["study_hash"].as_str().unwrap();
    assert!(hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(report["migration"]["from"], 0);
    assert!(names(&report).contains(&"system/controlDict".to_owned()));
    assert!(
        names(&report)
            .iter()
            .all(|n| !n.starts_with('/') && !n.contains(':'))
    );
    let v1 = write_study(&dir, CAVITY_V1);
    let out1 = dir.join("case-v1");
    let report1 = json(&run(&["compile", "--json", s(&v1), s(&out1)]));
    assert!(report1.get("migration").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_invalid_study_is_class_2_and_writes_nothing() {
    let dir = scratch("invalid");
    let study = write_study(&dir, &channel_without_outlet());
    let out = dir.join("case");
    let o = run(&["compile", s(&study), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("CFD-BC-001"));
    assert!(!out.exists());
    let o = run(&["compile", "--json", s(&study), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    let report = json(&o);
    assert_eq!(report["exit_class"], 2);
    assert_eq!(report["exit_class_name"], "study_invalid");
    assert_eq!(report["diagnostics"][0]["code"], "CFD-BC-001");
    assert!(!out.exists());

    let malformed = write_study(&dir, "{");
    let o = run(&["compile", "--json", s(&malformed), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-STUDY-001");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_unreadable_study_path_is_class_2() {
    let dir = scratch("unreadable");
    let missing = dir.join("nope.json");
    let out = dir.join("case");
    let o = run(&["compile", s(&missing), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot read study"));
    let o = run(&["compile", "--json", s(&missing), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-CLI-001");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_non_empty_output_directory_is_refused_and_left_alone() {
    let dir = scratch("non-empty");
    let study = write_study(&dir, CAVITY_V1);
    let out = dir.join("case");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("stale.txt"), "old").unwrap();
    let o = run(&["compile", "--json", s(&study), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    let report = json(&o);
    assert_eq!(report["diagnostics"][0]["code"], "CFD-CLI-001");
    assert!(report["artifacts"].as_array().unwrap().is_empty());
    assert_eq!(
        read_tree(&out),
        vec![("stale.txt".to_owned(), b"old".to_vec())]
    );
    // An empty directory is fine.
    std::fs::remove_file(out.join("stale.txt")).unwrap();
    assert_eq!(run(&["compile", s(&study), s(&out)]).status.code(), Some(0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_warning_never_changes_the_class_and_is_always_reported() {
    let dir = scratch("warning");
    let fast = CAVITY_V1.replace("\"value\": 0.0001,", "\"value\": 0.000001,");
    assert_ne!(fast, CAVITY_V1, "the fixture changed shape");
    let study = write_study(&dir, &fast);
    let out = dir.join("case");
    let o = run(&["compile", s(&study), s(&out)]);
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("warning CFD-WARN-001"));
    let out2 = dir.join("case2");
    let o = run(&["compile", "--json", s(&study), s(&out2)]);
    assert_eq!(o.status.code(), Some(0));
    let report = json(&o);
    assert_eq!(report["exit_class"], 0);
    assert_eq!(report["diagnostics"][0]["code"], "CFD-WARN-001");
    assert_eq!(report["diagnostics"][0]["severity"], "Warning");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Usage errors and invalid studies share class 2 on purpose: both mean "fix the input".
#[test]
fn a_usage_error_still_exits_2() {
    assert_eq!(run(&["compile"]).status.code(), Some(2));
    assert_eq!(
        run(&["compile", "--nonsense", "a", "b"]).status.code(),
        Some(2)
    );
}

fn copy_tree(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let dest = to.join(p.file_name().unwrap());
        if p.is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_tree(&p, &dest);
        } else {
            std::fs::copy(&p, &dest).unwrap();
        }
    }
}

#[test]
fn export_writes_the_case_plus_allrun_and_readme() {
    let dir = scratch("export-ok");
    let study = write_study(&dir, CAVITY_V1);
    let out = dir.join("case");
    let o = run(&["export", "--json", s(&study), s(&out)]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let report = json(&o);
    let listed = names(&report);
    assert!(listed.contains(&"Allrun".to_owned()) && listed.contains(&"README.md".to_owned()));
    let mut expected = read_tree(&golden("cavity-v2512"));
    expected.extend(read_tree(&golden("cavity-v2512-export")));
    expected.sort();
    assert_eq!(read_tree(&out), expected);
    let readme = std::fs::read_to_string(out.join("README.md")).unwrap();
    assert!(readme.contains(report["study_hash"].as_str().unwrap()));
    assert_eq!(report["study_hash"], CAVITY_HASH);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn export_refuses_an_invalid_study_and_a_non_empty_directory() {
    let dir = scratch("export-refuse");
    let out = dir.join("case");
    let bad = write_study(&dir, &channel_without_outlet());
    let o = run(&["export", s(&bad), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());

    let good = write_study(&dir, CAVITY_V1);
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("stale.txt"), "old").unwrap();
    let o = run(&["export", "--json", s(&good), s(&out)]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-CLI-001");
    assert_eq!(read_tree(&out).len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_case_passes_a_clean_case_and_names_file_entry_and_line_of_a_refusal() {
    let dir = scratch("check-case");
    let case = dir.join("case");
    std::fs::create_dir_all(&case).unwrap();
    copy_tree(&golden("cavity-v2512"), &case);
    let o = run(&["check-case", s(&case)]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );

    let dict = case.join("system/blockMeshDict");
    let text = std::fs::read_to_string(&dict).unwrap();
    let planted = text.replace(
        "convertToMeters 1;",
        "convertToMeters #codeStream { code \"x\"; };",
    );
    assert_ne!(planted, text, "the golden file changed shape");
    std::fs::write(&dict, planted).unwrap();
    let o = run(&["check-case", s(&case)]);
    assert_eq!(o.status.code(), Some(3));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("system/blockMeshDict:9") && err.contains("#codeStream"),
        "{err}"
    );
    let o = run(&["check-case", "--json", s(&case)]);
    assert_eq!(o.status.code(), Some(3));
    let report = json(&o);
    assert_eq!(report["exit_class"], 3);
    assert_eq!(report["exit_class_name"], "guard_refused");
    let first = &report["diagnostics"][0];
    assert_eq!(first["code"], "CFD-GUARD-001");
    assert_eq!(first["location"]["file"], "system/blockMeshDict");
    assert_eq!(first["location"]["line"], 9);
    assert_eq!(first["location"]["entry"], "#codeStream");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_case_skips_the_export_files_and_refuses_a_link() {
    let dir = scratch("check-case-export");
    let study = write_study(&dir, CAVITY_V1);
    let case = dir.join("case");
    assert_eq!(run(&["export", s(&study), s(&case)]).status.code(), Some(0));
    let o = run(&["check-case", "--json", s(&case)]);
    assert_eq!(o.status.code(), Some(0));
    let report = json(&o);
    let of_kind = |kind: &str| -> Vec<String> {
        report["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["kind"] == kind)
            .map(|a| a["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(of_kind("skipped"), ["Allrun", "README.md"]);
    assert_eq!(of_kind("scanned").len(), 8);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", case.join("system/linked")).unwrap();
        let o = run(&["check-case", s(&case)]);
        assert_eq!(o.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&o.stderr).contains("system/linked"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_case_on_something_that_is_not_a_directory_is_class_2() {
    let dir = scratch("check-case-bad");
    let o = run(&["check-case", "--json", s(&dir.join("absent"))]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(json(&o)["diagnostics"][0]["code"], "CFD-CLI-001");
    let file = write_study(&dir, "x");
    let o = run(&["check-case", s(&file)]);
    assert_eq!(o.status.code(), Some(2));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn help_lists_the_new_verbs_beside_the_old_ones() {
    let o = run(&["--help"]);
    let text = String::from_utf8_lossy(&o.stdout);
    for verb in ["compile", "export", "check-case", "run", "validate"] {
        assert!(text.contains(verb), "{verb}");
    }
}
