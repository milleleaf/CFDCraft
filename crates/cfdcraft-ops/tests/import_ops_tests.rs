//! Importing an existing case as a report: what is copied, what is refused, what is named, and
//! which backend could run the result. Every case here is written by the test from the dictionary
//! syntax alone.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cfdcraft_ops::repro::ImportData;
use cfdcraft_ops::{
    ExitClass, Report, Repro, check_runnable, import_case, import_case_path, verify_case,
};
use cfdcraft_text::{Line, Text};

/// A workspace folder and, beside it, a folder outside the workspace. Both are removed on drop,
/// after the read-only marks of the stored originals are cleared.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("ops-import-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ws")).unwrap();
        fs::create_dir_all(root.join("outside")).unwrap();
        Scratch { root }
    }

    /// The workspace root.
    fn ws(&self) -> PathBuf {
        self.root.join("ws")
    }

    /// A folder outside the workspace.
    fn outside(&self) -> PathBuf {
        self.root.join("outside")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_writable_below(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o644));
}

#[cfg(not(unix))]
#[allow(clippy::permissions_set_readonly_false)]
fn make_writable(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

fn make_writable_below(dir: &Path) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => make_writable_below(&path),
            Ok(m) if m.is_file() => make_writable(&path),
            _ => {}
        }
    }
}

/// Write `text` to `root/rel`, creating the folders above it.
fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A dictionary file: the `FoamFile` header for `object`, then `body`.
fn dict(object: &str, body: &str) -> String {
    format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class dictionary;\n    object {object};\n}}\n\n{body}\n"
    )
}

const CONTROL_DICT: &str = "solver incompressibleFluid;
startFrom startTime;
startTime 0;
stopAt endTime;
endTime 0.5;
deltaT 0.005;
writeControl timeStep;
writeInterval 20;

functions
{
    probes
    {
        type probes;
        libs (sampling);
        fields (p U);
        probeLocations ((0.5 0.5 0.05));
    }
}";

/// A small clean case in the layout of a current Foundation release.
fn write_clean_case(root: &Path) {
    put(
        root,
        "system/controlDict",
        &dict("controlDict", CONTROL_DICT),
    );
    put(
        root,
        "system/fvSchemes",
        &dict("fvSchemes", "ddtSchemes\n{\n    default steadyState;\n}"),
    );
    put(
        root,
        "constant/physicalProperties",
        &dict("physicalProperties", "viscosityModel constant;\nnu 0.01;"),
    );
    put(
        root,
        "constant/momentumTransport",
        &dict(
            "momentumTransport",
            "simulationType RAS;\nRAS\n{\n    model kEpsilon;\n    turbulence on;\n}",
        ),
    );
    put(
        root,
        "0/U",
        &dict(
            "U",
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{\n    movingWall\n    {\n        type fixedValue;\n        value uniform (1 0 0);\n    }\n    frontAndBack\n    {\n        type empty;\n    }\n}",
        ),
    );
    put(
        root,
        "0/p",
        &dict(
            "p",
            "dimensions [0 2 -2 0 0 0 0];\ninternalField uniform 0;\nboundaryField\n{\n    movingWall\n    {\n        type zeroGradient;\n    }\n    frontAndBack\n    {\n        type empty;\n    }\n}",
        ),
    );
}

/// A case with only a control dictionary and the named constant files, for dialect detection.
fn write_dialect_case(root: &Path, solver_key: &str, constant_files: &[&str]) {
    put(
        root,
        "system/controlDict",
        &dict("controlDict", &format!("{solver_key} someSolver;")),
    );
    for name in constant_files {
        put(root, &format!("constant/{name}"), &dict(name, "a 1;"));
    }
}

fn say(line: &Line) -> String {
    Text::english().render(line)
}

fn codes(report: &Report) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn import_data(report: &Report) -> &ImportData {
    match &report.repro {
        Some(Repro::Import(data)) => data,
        other => panic!("expected the data of an import, found {other:?}"),
    }
}

/// Everything a report shows a person, as one text.
fn shown(report: &Report) -> String {
    let mut text = report.render_text();
    for artifact in &report.artifacts {
        text.push_str(&format!("{} {}\n", artifact.kind, artifact.name));
    }
    if let Some(Repro::Import(data)) = &report.repro {
        for line in data.lines() {
            text.push_str(&say(&line));
            text.push('\n');
        }
    }
    text
}

#[test]
fn a_clean_case_imports_and_the_report_says_read_only() {
    let s = Scratch::new("clean");
    write_clean_case(&s.ws().join("case-dir"));
    let report = import_case(&s.ws(), "case-dir", "mycase");
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    assert!(report.diagnostics.is_empty());

    let target = s.ws().join("mycase.cfdcase");
    assert!(target.join("original/system/controlDict").is_file());
    assert!(target.join("work/system/controlDict").is_file());
    assert!(target.join("import.json").is_file());

    let data = import_data(&report);
    assert_eq!(data.case, "mycase.cfdcase");
    assert_eq!(data.dialect, "foundation-14");
    assert_eq!(data.refused, 0);
    assert!(data.files >= 6, "{}", data.files);
    assert!(data.runnable);
    let lines = data.lines();
    assert!(lines.contains(&Line::ImportDone {
        name: "mycase.cfdcase".to_owned(),
        files: data.files.to_string(),
        refused: "0".to_owned(),
    }));
    assert!(lines.contains(&Line::ImportDialect {
        dialect: "foundation-14".to_owned(),
    }));
    assert!(lines.contains(&Line::ImportReadOnly {
        name: "mycase.cfdcase".to_owned(),
    }));
    assert!(lines.contains(&Line::ImportInventory {
        key: "solver".to_owned(),
        value: "incompressibleFluid".to_owned(),
    }));
    assert!(
        !lines
            .iter()
            .any(|l| matches!(l, Line::ImportNoMatchingBackend { .. }))
    );

    let artifact = |kind: &str| {
        report
            .artifacts
            .iter()
            .find(|a| a.kind == kind)
            .map(|a| a.name.as_str())
    };
    assert_eq!(artifact("case"), Some("mycase.cfdcase"));
    assert_eq!(artifact("dialect"), Some("foundation-14"));
    assert_eq!(artifact("files"), Some(data.files.to_string().as_str()));

    // Only workspace-relative names appear.
    let text = shown(&report);
    let root = s.root.to_string_lossy().into_owned();
    assert!(!text.contains(&root), "{text}");
    assert!(
        !text.contains(&s.ws().to_string_lossy().into_owned()),
        "{text}"
    );
}

#[test]
fn the_command_line_form_takes_a_folder_and_never_prints_it() {
    let s = Scratch::new("pathform");
    write_clean_case(&s.outside().join("my-case-folder"));
    let report = import_case_path(&s.outside().join("my-case-folder"), &s.ws(), "pathcase");
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    assert!(s.ws().join("pathcase.cfdcase/import.json").is_file());
    let text = shown(&report);
    assert!(
        !text.contains(&s.root.to_string_lossy().into_owned()),
        "{text}"
    );

    let missing = import_case_path(&s.outside().join("not-here"), &s.ws(), "othercase");
    assert_eq!(codes(&missing), ["CFD-IMPORT-001"]);
    let text = shown(&missing);
    assert!(text.contains("not-here"), "{text}");
    assert!(
        !text.contains(&s.root.to_string_lossy().into_owned()),
        "{text}"
    );
}

#[test]
fn a_refused_case_lists_every_entry_and_writes_nothing() {
    let s = Scratch::new("refused");
    let source = s.ws().join("bad-case");
    write_clean_case(&source);
    put(&source, "system/extraDict", "a 1;\nb\n#codeStream\n{\n}\n");
    put(&source, "0/otherDict", "a 1;\ntype coded;\n");
    let report = import_case(&s.ws(), "bad-case", "badcase");

    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert_eq!(codes(&report), ["CFD-IMPORT-002", "CFD-IMPORT-002"]);
    let first = &report.diagnostics[0].message;
    assert!(first.contains("0/otherDict"), "{first}");
    assert!(first.contains("line 2"), "{first}");
    assert!(first.contains("CFD-GUARD-"), "{first}");
    let second = &report.diagnostics[1].message;
    assert!(second.contains("system/extraDict"), "{second}");
    assert!(second.contains("line 3"), "{second}");
    assert!(second.contains("CFD-GUARD-001"), "{second}");

    assert!(!s.ws().join("badcase.cfdcase").exists());
    assert!(report.repro.is_none());
}

#[test]
fn the_error_codes_are_mapped() {
    let s = Scratch::new("codes");

    // The source cannot be read.
    let unreadable = import_case(&s.ws(), "nowhere", "one");
    assert_eq!(codes(&unreadable), ["CFD-IMPORT-001"]);

    // The target exists: the second import of one case under one name.
    write_clean_case(&s.ws().join("case-dir"));
    let first = import_case(&s.ws(), "case-dir", "twice");
    assert_eq!(first.class(), ExitClass::Passed, "{}", first.render_text());
    let second = import_case(&s.ws(), "case-dir", "twice");
    assert_eq!(codes(&second), ["CFD-IMPORT-003"]);

    // A folder that is not a case.
    put(&s.ws().join("notes"), "readme.txt", "just some text\n");
    let not_a_case = import_case(&s.ws(), "notes", "notacase");
    assert_eq!(codes(&not_a_case), ["CFD-IMPORT-004"]);
    assert!(!s.ws().join("notacase.cfdcase").exists());

    // A cap: a folder nested deeper than the import goes.
    let mut deep = String::new();
    for _ in 0..24 {
        deep.push_str("d/");
    }
    put(&s.ws().join("deep-case"), &format!("{deep}file.txt"), "x\n");
    let capped = import_case(&s.ws(), "deep-case", "capped");
    assert_eq!(codes(&capped), ["CFD-IMPORT-006"]);
    assert!(!s.ws().join("capped.cfdcase").exists());

    // A name that is not a plain name.
    let bad_name = import_case(&s.ws(), "case-dir", "Bad Name");
    assert_eq!(bad_name.class(), ExitClass::StudyInvalid);
    assert_eq!(bad_name.diagnostics.len(), 1);
    assert!(!s.ws().join("Bad Name.cfdcase").exists());
}

#[test]
fn a_source_outside_the_workspace_is_refused_and_never_opened() {
    let s = Scratch::new("escape");
    write_clean_case(&s.outside().join("secret-case"));
    let absolute = s.outside().join("secret-case");
    let absolute_text = absolute.to_string_lossy().into_owned();
    for source in [
        "../outside/secret-case",
        "..",
        "a/../../outside/secret-case",
        absolute_text.as_str(),
        "",
        "back\\slash",
    ] {
        let report = import_case(&s.ws(), source, "escaped");
        assert_eq!(codes(&report), ["CFD-IMPORT-001"], "{source}");
        let text = shown(&report);
        assert!(
            !text.contains(&s.root.to_string_lossy().into_owned()),
            "{text}"
        );
        assert!(!s.ws().join("escaped.cfdcase").exists(), "{source}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(s.outside().join("secret-case"), s.ws().join("linked")).unwrap();
        let report = import_case(&s.ws(), "linked", "escaped");
        assert_eq!(codes(&report), ["CFD-IMPORT-001"]);
        assert!(!s.ws().join("escaped.cfdcase").exists());
    }
}

#[test]
fn a_dialect_without_a_backend_is_inspectable_but_not_runnable() {
    let s = Scratch::new("dialect");

    // An older dialect no backend runs: the import works and says so.
    write_dialect_case(
        &s.ws().join("old-case"),
        "application",
        &["transportProperties", "momentumTransport"],
    );
    let report = import_case(&s.ws(), "old-case", "oldcase");
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    let data = import_data(&report);
    assert_eq!(data.dialect, "foundation-legacy");
    assert!(!data.runnable);
    assert!(data.lines().contains(&Line::ImportNoMatchingBackend {
        dialect: "foundation-legacy".to_owned(),
    }));
    let runnable = check_runnable(&s.ws(), "oldcase");
    assert_eq!(codes(&runnable), ["CFD-IMPORT-005"]);
    assert_eq!(runnable.class(), ExitClass::StudyInvalid);
    assert!(
        runnable.diagnostics[0]
            .message
            .contains("foundation-legacy"),
        "{}",
        runnable.diagnostics[0].message
    );

    // A current Foundation case has a backend row.
    write_dialect_case(
        &s.ws().join("new-case"),
        "solver",
        &["physicalProperties", "momentumTransport"],
    );
    let report = import_case(&s.ws(), "new-case", "newcase");
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    let runnable = check_runnable(&s.ws(), "newcase");
    assert_eq!(
        runnable.class(),
        ExitClass::Passed,
        "{}",
        runnable.render_text()
    );
    let backend = runnable
        .artifacts
        .iter()
        .find(|a| a.kind == "backend")
        .map(|a| a.name.as_str());
    assert_eq!(backend, Some("of14-20261008"));

    // A case that was never imported has no record.
    let missing = check_runnable(&s.ws(), "never-imported");
    assert_eq!(codes(&missing), ["CFD-IMPORT-007"]);
}

#[test]
fn verify_reports_a_changed_original() {
    let s = Scratch::new("verify");
    write_clean_case(&s.ws().join("case-dir"));
    let imported = import_case(&s.ws(), "case-dir", "stored");
    assert_eq!(
        imported.class(),
        ExitClass::Passed,
        "{}",
        imported.render_text()
    );

    let clean = verify_case(&s.ws(), "stored");
    assert_eq!(clean.class(), ExitClass::Passed, "{}", clean.render_text());
    assert!(clean.diagnostics.is_empty());

    // The working copy is meant to change.
    fs::write(
        s.ws().join("stored.cfdcase/work/system/controlDict"),
        "changed\n",
    )
    .unwrap();
    assert!(verify_case(&s.ws(), "stored").diagnostics.is_empty());

    // One changed byte of the original is a finding that names the file.
    let path = s.ws().join("stored.cfdcase/original/system/controlDict");
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    make_writable(&path);
    fs::write(&path, bytes).unwrap();
    let report = verify_case(&s.ws(), "stored");
    assert_eq!(codes(&report), ["CFD-IMPORT-007"]);
    assert!(
        report.diagnostics[0].message.contains("system/controlDict"),
        "{}",
        report.diagnostics[0].message
    );
    assert_eq!(report.class(), ExitClass::StudyInvalid);
}
