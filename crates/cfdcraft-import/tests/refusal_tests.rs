//! Cases that hold something unsafe are refused whole; caps and scripts are held.

mod support;

use cfdcraft_import::{ImportError, ImportOptions, ImportOutcome, ImportRefusal, import_case};
use std::fs;
use support::{Scratch, dict, file_names, put, write_clean_case};

fn refused(s: &Scratch) -> Vec<ImportRefusal> {
    match import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap() {
        ImportOutcome::Refused { refusals, .. } => refusals,
        other => panic!("the case was not refused: {other:?}"),
    }
}

/// A clean case plus one file with the given text, and what the guard must say about it.
struct Class {
    label: &'static str,
    rel: &'static str,
    text: &'static str,
    code: &'static str,
    line: u32,
}

const CLASSES: [Class; 6] = [
    Class {
        label: "codeStream",
        rel: "system/extraDict",
        text: "a 1;\nb\n#codeStream\n{\n}\n",
        code: "CFD-GUARD-001",
        line: 3,
    },
    Class {
        label: "coded type",
        rel: "0/extraDict",
        text: "a 1;\ntype coded;\n",
        code: "CFD-GUARD-002",
        line: 2,
    },
    Class {
        label: "calc",
        rel: "system/extraDict",
        text: "a 1;\nb #calc \"1+1\";\n",
        code: "CFD-GUARD-001",
        line: 2,
    },
    Class {
        label: "systemCall",
        rel: "system/extraDict",
        text: "a 1;\ntype systemCall;\n",
        code: "CFD-GUARD-003",
        line: 2,
    },
    Class {
        label: "unlisted library",
        rel: "system/extraDict",
        text: "a 1;\nlibs (\"libfoo.so\");\n",
        code: "CFD-GUARD-004",
        line: 2,
    },
    Class {
        label: "etc include",
        rel: "system/extraDict",
        text: "a 1;\n#includeEtc \"caseDicts/setConstraintTypes\"\n",
        code: "CFD-GUARD-001",
        line: 2,
    },
];

#[test]
fn each_refusal_class_refuses_the_whole_import() {
    for class in &CLASSES {
        let s = Scratch::new("class");
        write_clean_case(&s.source());
        put(&s.source(), class.rel, class.text);
        let refusals = refused(&s);
        assert_eq!(refusals.len(), 1, "{}: {refusals:?}", class.label);
        assert_eq!(refusals[0].code, class.code, "{}", class.label);
        assert_eq!(refusals[0].file, class.rel, "{}", class.label);
        assert_eq!(refusals[0].line, class.line, "{}", class.label);
        assert!(!s.target().exists(), "{} wrote a target", class.label);
    }

    let s = Scratch::new("library file");
    write_clean_case(&s.source());
    put(&s.source(), "libs/libfoo.so", "not a library\n");
    let refusals = refused(&s);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, "CFD-GUARD-007");
    assert_eq!(refusals[0].file, "libs/libfoo.so");
    assert!(!s.target().exists());
}

#[cfg(unix)]
#[test]
fn a_link_and_a_special_file_refuse_the_whole_import() {
    let s = Scratch::new("link");
    write_clean_case(&s.source());
    std::os::unix::fs::symlink("system/controlDict", s.source().join("link")).unwrap();
    let refusals = refused(&s);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, "CFD-GUARD-007");
    assert_eq!(refusals[0].file, "link");
    assert!(!s.target().exists());

    let s = Scratch::new("pipe");
    write_clean_case(&s.source());
    let made = std::process::Command::new("mkfifo")
        .arg(s.source().join("pipe"))
        .status()
        .unwrap();
    assert!(made.success());
    let refusals = refused(&s);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, "CFD-GUARD-007");
    assert_eq!(refusals[0].file, "pipe");
    assert!(!s.target().exists());

    // A link in place of the control dictionary is a refusal, not "not a case".
    let s = Scratch::new("controllink");
    put(&s.source(), "elsewhere", "a 1;\n");
    fs::create_dir_all(s.source().join("system")).unwrap();
    std::os::unix::fs::symlink("../elsewhere", s.source().join("system/controlDict")).unwrap();
    let refusals = refused(&s);
    assert_eq!(refusals[0].file, "system/controlDict");
}

#[test]
fn several_refused_files_are_all_listed() {
    let s = Scratch::new("several");
    write_clean_case(&s.source());
    put(&s.source(), "0/a", "#calc x;\n");
    put(&s.source(), "constant/b", "type coded;\n");
    put(&s.source(), "system/c", "libs (\"x.so\");\n");
    let refusals = refused(&s);
    let seen: Vec<(&str, &str)> = refusals
        .iter()
        .map(|r| (r.file.as_str(), r.code.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("0/a", "CFD-GUARD-001"),
            ("constant/b", "CFD-GUARD-002"),
            ("system/c", "CFD-GUARD-004"),
        ]
    );
    assert!(!s.target().exists());
}

#[test]
fn a_refused_case_still_reports_its_inventory() {
    let s = Scratch::new("inventory");
    write_clean_case(&s.source());
    put(
        &s.source(),
        "system/controlDict",
        &dict(
            "controlDict",
            "solver incompressibleFluid;\nendTime 1;\nfunctions\n{\n    #includeFunc streamlinesLine\n    probes\n    {\n        type probes;\n    }\n}",
        ),
    );
    let outcome = import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap();
    let ImportOutcome::Refused {
        refusals,
        inventory,
    } = outcome
    else {
        panic!("the case was not refused");
    };
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].code, "CFD-GUARD-001");
    assert_eq!(refusals[0].file, "system/controlDict");
    assert_eq!(inventory.dialect, "foundation-14");
    assert_eq!(
        inventory.application_or_solver.as_deref(),
        Some("incompressibleFluid")
    );
    assert_eq!(inventory.function_objects, ["probes", "streamlinesLine"]);
    assert_eq!(inventory.mesh_kind, "blockMesh");
    assert!(!s.target().exists());
}

#[test]
fn caps_stop_the_import() {
    let s = Scratch::new("cap-files");
    write_clean_case(&s.source());
    let few = ImportOptions {
        max_files: 3,
        ..ImportOptions::default()
    };
    let err = import_case(&s.source(), &s.target(), &few).unwrap_err();
    assert!(
        matches!(err, ImportError::Cap { what: "files", .. }),
        "{err}"
    );
    assert_eq!(err.code(), Some("CFD-IMPORT-006"));
    assert!(!s.target().exists());

    let small = ImportOptions {
        max_file_bytes: 60,
        ..ImportOptions::default()
    };
    let err = import_case(&s.source(), &s.target(), &small).unwrap_err();
    assert!(
        matches!(
            err,
            ImportError::Cap {
                what: "file_bytes",
                ..
            }
        ),
        "{err}"
    );
    assert!(!s.target().exists());

    let tight = ImportOptions {
        max_total_bytes: 100,
        ..ImportOptions::default()
    };
    let err = import_case(&s.source(), &s.target(), &tight).unwrap_err();
    assert!(
        matches!(
            err,
            ImportError::Cap {
                what: "total_bytes",
                ..
            }
        ),
        "{err}"
    );
    assert!(!s.target().exists());

    put(&s.source(), "constant/a/b/c", "x\n");
    let shallow = ImportOptions {
        max_depth: 2,
        ..ImportOptions::default()
    };
    let err = import_case(&s.source(), &s.target(), &shallow).unwrap_err();
    assert!(
        matches!(err, ImportError::Cap { what: "depth", .. }),
        "{err}"
    );
    assert!(!s.target().exists());
}

#[test]
fn scripts_are_copied_inert_and_left_out_of_the_working_copy() {
    let s = Scratch::new("scripts");
    write_clean_case(&s.source());
    let body = "#!/bin/sh\ntouch marker\n";
    put(&s.source(), "Allrun", body);
    put(&s.source(), "Allclean", body);
    put(&s.source(), "run.sh", body);

    let outcome = import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap();
    let ImportOutcome::Imported(record) = outcome else {
        panic!("the case was not imported");
    };
    let original = file_names(&s.target().join("original"));
    let work = file_names(&s.target().join("work"));
    for name in ["Allrun", "Allclean", "run.sh"] {
        assert!(original.contains(name), "{name} is not in the original");
        assert!(!work.contains(name), "{name} is in the working copy");
        let meta = fs::metadata(s.target().join("original").join(name)).unwrap();
        assert!(meta.permissions().readonly());
        assert!(record.files.iter().any(|f| f.rel_path == name));
    }
    assert_eq!(record.scripts, ["Allclean", "Allrun", "run.sh"]);
    assert_eq!(original.len(), work.len() + 3);
    assert!(!s.source().join("marker").exists());
    assert!(!s.target().join("marker").exists());
    assert!(!s.target().join("original/marker").exists());
}
