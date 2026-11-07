use cfdcraft_backend_esi::{
    CaseFile, CaseTree, Dialect, compile,
    guard::{GuardFile, Refusal, Rule, scan_dir, scan_files, scan_tree},
};
use cfdcraft_model::{Study, resolve};
use std::path::{Path, PathBuf};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Every file under `dir`, with `/`-separated relative paths, sorted.
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

fn one(path: &str, text: &str) -> Vec<Refusal> {
    scan_files(&[GuardFile {
        path: path.into(),
        bytes: text.as_bytes().to_vec(),
    }])
}

fn codes(r: &[Refusal]) -> Vec<(usize, &str, &'static str)> {
    r.iter()
        .map(|r| (r.line, r.entry.as_str(), r.rule.code()))
        .collect()
}

#[test]
fn golden_trees_pass_as_files_and_as_compiled_trees() {
    for (dir, json) in [("cavity-v2512", CAVITY_V0), ("channel-v2512", CHANNEL_V1)] {
        let files: Vec<GuardFile> = read_tree(&golden(dir))
            .into_iter()
            .map(|(path, bytes)| GuardFile { path, bytes })
            .collect();
        assert_eq!(files.len(), 8, "{dir}");
        assert_eq!(scan_files(&files), vec![], "{dir} from disk");
        let resolved = resolve(&Study::from_json(json).unwrap()).unwrap();
        let tree = compile(&resolved, Dialect::EsiV2512).unwrap();
        assert_eq!(scan_tree(&tree), vec![], "{dir} compiled");
    }
}

#[test]
fn code_stream_is_refused_with_file_entry_and_line() {
    let text = "FoamFile\n{\n}\nconvertToMeters #codeStream { };\n";
    let r = one("system/blockMeshDict", text);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].file, "system/blockMeshDict");
    assert_eq!(
        (r[0].line, r[0].entry.as_str(), r[0].rule),
        (4, "#codeStream", Rule::Directive)
    );
    assert_eq!(r[0].rule.code(), "CFD-GUARD-001");

    let text = "a 1;\nb 2;\nconvertToMeters #codeStream { code \"x\"; };\n";
    let r = one("system/blockMeshDict", text);
    assert_eq!(
        codes(&r),
        vec![
            (3, "#codeStream", "CFD-GUARD-001"),
            (3, "code", "CFD-GUARD-002")
        ]
    );
}

#[test]
fn compiled_tree_with_a_planted_directive_is_refused() {
    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: PathBuf::from("system/controlDict"),
            bytes: b"x #calc{1+1};\n".to_vec(),
        }],
    };
    let r = scan_tree(&tree);
    assert_eq!(codes(&r), vec![(1, "#calc", "CFD-GUARD-001")]);
    assert_eq!(r[0].file, "system/controlDict");
}

#[test]
fn every_refused_construct_has_the_right_code_and_line() {
    let rows: &[(&str, &str, &str)] = &[
        ("x #calc {1};", "#calc", "CFD-GUARD-001"),
        (
            "#includeEtc \"caseDicts/x\"",
            "#includeEtc",
            "CFD-GUARD-001",
        ),
        ("#includeFunc streamlines", "#includeFunc", "CFD-GUARD-001"),
        ("v #eval{1+1};", "#eval", "CFD-GUARD-001"),
        ("# calc", "#", "CFD-GUARD-001"),
        ("v #{ anything\nat all #};", "#{", "CFD-GUARD-001"),
        ("type codedFixedValue;", "codedFixedValue", "CFD-GUARD-002"),
        ("type codedMixed;", "codedMixed", "CFD-GUARD-002"),
        ("type coded;", "coded", "CFD-GUARD-002"),
        (
            "type scalarCodedSource;",
            "scalarCodedSource",
            "CFD-GUARD-002",
        ),
        (
            "type vectorCodedSource;",
            "vectorCodedSource",
            "CFD-GUARD-002",
        ),
        (
            "type sphericalTensorCodedSource;",
            "sphericalTensorCodedSource",
            "CFD-GUARD-002",
        ),
        (
            "type symmTensorCodedSource;",
            "symmTensorCodedSource",
            "CFD-GUARD-002",
        ),
        (
            "type tensorCodedSource;",
            "tensorCodedSource",
            "CFD-GUARD-002",
        ),
        ("codeContext x;", "codeContext", "CFD-GUARD-002"),
        ("codeData x;", "codeData", "CFD-GUARD-002"),
        ("codeRead x;", "codeRead", "CFD-GUARD-002"),
        ("codeExecute x;", "codeExecute", "CFD-GUARD-002"),
        ("codeWrite x;", "codeWrite", "CFD-GUARD-002"),
        ("codeEnd x;", "codeEnd", "CFD-GUARD-002"),
        ("codeCorrect x;", "codeCorrect", "CFD-GUARD-002"),
        ("codeConstrain x;", "codeConstrain", "CFD-GUARD-002"),
        ("codeAddSup x;", "codeAddSup", "CFD-GUARD-002"),
        ("codeAddSupRho x;", "codeAddSupRho", "CFD-GUARD-002"),
        ("codeSetValue x;", "codeSetValue", "CFD-GUARD-002"),
        ("code x;", "code", "CFD-GUARD-002"),
        ("codeInclude x;", "codeInclude", "CFD-GUARD-002"),
        ("codeOptions x;", "codeOptions", "CFD-GUARD-002"),
        ("codeLibs x;", "codeLibs", "CFD-GUARD-002"),
        ("redirectType x;", "redirectType", "CFD-GUARD-002"),
        ("type CODEDFIXEDVALUE;", "CODEDFIXEDVALUE", "CFD-GUARD-002"),
        ("type \"coded\";", "coded", "CFD-GUARD-002"),
        ("type \"co ded\";", "co ded", "CFD-GUARD-002"),
        ("type systemCall;", "systemCall", "CFD-GUARD-003"),
        ("executeCalls (x);", "executeCalls", "CFD-GUARD-003"),
        ("writeCalls (x);", "writeCalls", "CFD-GUARD-003"),
        ("endCalls (x);", "endCalls", "CFD-GUARD-003"),
        ("type \"systemCall\";", "systemCall", "CFD-GUARD-003"),
    ];
    for (text, entry, code) in rows {
        let src = format!("a 1;\nb 2;\n{text}\n");
        let r = one("system/x", &src);
        assert!(!r.is_empty(), "{text}");
        assert_eq!(
            (r[0].line, r[0].entry.as_str(), r[0].rule.code()),
            (3, *entry, *code),
            "{text}"
        );
    }
}

#[test]
fn comments_and_strings_hide_nothing_and_invent_nothing() {
    let quiet = "// type codedFixedValue; #codeStream systemCall\n\
                 /* type coded;\n #calc executeCalls */\n\
                 note \"uses codeStream and #calc and systemCall\";\n\
                 more \"a // b /* c\";\n\
                 a 1;\n";
    assert_eq!(one("system/x", quiet), vec![]);
    // Lines stay right after multi-line comments and strings.
    let text = "/* one\ntwo\nthree */\nnote \"x\ny\";\ntype coded;\n";
    assert_eq!(codes(&one("f", text)), vec![(6, "coded", "CFD-GUARD-002")]);
    // A comment marker inside a string does not start a comment.
    assert_eq!(
        codes(&one("f", "n \"//\"; type coded;\n")),
        vec![(1, "coded", "CFD-GUARD-002")]
    );
    // An escaped quote does not end a string.
    assert_eq!(
        codes(&one("f", "n \"a\\\"b\"; type coded;\n")),
        vec![(1, "coded", "CFD-GUARD-002")]
    );
}

#[test]
fn unterminated_constructs_fail_closed() {
    for (text, entry, line) in [
        ("a 1;\nn \"never closed;\n", "\"", 2),
        ("a 1;\n/* never closed\n", "/*", 2),
        ("a 1;\nv #{ never closed\n", "#{", 2),
    ] {
        let r = one("f", text);
        assert_eq!(codes(&r), vec![(line, entry, "CFD-GUARD-008")], "{text}");
    }
}

#[test]
fn nul_and_invalid_utf8_are_refused() {
    let r = scan_files(&[GuardFile {
        path: "f".into(),
        bytes: b"a 1;\nb\0 2;\n".to_vec(),
    }]);
    assert_eq!(codes(&r), vec![(2, "NUL byte", "CFD-GUARD-007")]);
    let r = scan_files(&[GuardFile {
        path: "f".into(),
        bytes: b"a 1;\nb \xff 2;\n".to_vec(),
    }]);
    assert_eq!(codes(&r), vec![(2, "invalid UTF-8", "CFD-GUARD-007")]);
}

#[test]
fn several_offences_are_all_reported_sorted_by_file_then_line() {
    let a = "x #calc{1};\ny 1;\ntype systemCall;\n";
    let b = "type coded;\n";
    let r = scan_files(&[
        GuardFile {
            path: "system/b".into(),
            bytes: b.as_bytes().to_vec(),
        },
        GuardFile {
            path: "system/a".into(),
            bytes: a.as_bytes().to_vec(),
        },
    ]);
    let got: Vec<(&str, usize)> = r.iter().map(|r| (r.file.as_str(), r.line)).collect();
    assert_eq!(got, vec![("system/a", 1), ("system/a", 3), ("system/b", 1)]);
}

#[test]
fn an_entry_is_cut_to_eighty_characters() {
    let long = format!("type {};", "coded".repeat(40));
    let r = one("f", &long);
    assert_eq!(r[0].entry.chars().count(), 80);
}

fn libs_rows(rows: &[(&str, Option<&str>)]) {
    for (text, expect) in rows {
        let src = format!("a 1;\nb 2;\n{text}\n");
        let r = one("system/controlDict", &src);
        match expect {
            None => assert_eq!(r, vec![], "{text}"),
            Some(entry) => assert_eq!(
                (r[0].line, r[0].entry.as_str(), r[0].rule.code()),
                (3, *entry, "CFD-GUARD-004"),
                "{text}"
            ),
        }
    }
}

#[test]
fn only_the_listed_libraries_may_be_loaded() {
    libs_rows(&[
        ("libs (sampling);", None),
        ("libs (\"libsampling.so\");", None),
        ("libs ( );", None),
        ("libs ();", None),
        ("functionObjectLibs (sampling);", None),
        ("libs (foo);", Some("foo")),
        ("libs (\"libfoo.so\");", Some("libfoo.so")),
        (
            "libs (\"/abs/libsampling.so\");",
            Some("/abs/libsampling.so"),
        ),
        ("libs (\"../libsampling.so\");", Some("../libsampling.so")),
        ("libs (\"sampling\" \"other\");", Some("other")),
        ("libs other;", Some("other")),
        ("functionObjectLibs (foo);", Some("foo")),
        ("libs (sampling $x);", Some("$x")),
    ]);
}

#[test]
fn includes_must_be_literal_relative_paths_inside_the_case() {
    for ok in ["#include \"defs/common\"", "#include defs"] {
        assert_eq!(one("system/x", &format!("{ok}\n")), vec![], "{ok}");
    }
    for bad in [
        "#include \"../outside\"",
        "#include \"a/../../b\"",
        "#include \"/etc/passwd\"",
        "#include \"$FOAM_CASE/x\"",
        "#include \"~/x\"",
        "#include \"C:/x\"",
        "#include \"a\\\\b\"",
        "#include \"\"",
        "#include ;",
        "#Include \"x\"",
    ] {
        let r = one("system/x", &format!("a 1;\n{bad}\n"));
        assert!(!r.is_empty(), "{bad}");
        assert_eq!(r[0].line, 2, "{bad}");
        let want = if bad.starts_with("#Include") {
            "CFD-GUARD-001"
        } else {
            "CFD-GUARD-006"
        };
        assert_eq!(r[0].rule.code(), want, "{bad}");
    }
}

#[test]
fn switches_that_enable_code_are_refused() {
    for (text, entry) in [
        ("allowSystemOperations 1;", "allowSystemOperations"),
        ("InfoSwitches { a 1; }", "InfoSwitches"),
        ("DebugSwitches { a 1; }", "DebugSwitches"),
        ("OptimisationSwitches { a 1; }", "OptimisationSwitches"),
        ("allowsystemoperations 1;", "allowsystemoperations"),
    ] {
        let r = one("system/controlDict", &format!("a 1;\n{text}\n"));
        assert_eq!(
            (r[0].line, r[0].entry.as_str(), r[0].rule.code()),
            (2, entry, "CFD-GUARD-005"),
            "{text}"
        );
    }
}

fn tree_row(path: &str, bytes: &[u8]) -> Vec<Refusal> {
    scan_files(&[GuardFile {
        path: path.into(),
        bytes: bytes.to_vec(),
    }])
}

#[test]
fn paths_and_files_a_case_may_not_contain_are_refused() {
    for path in [
        "../system/controlDict",
        "system/../x",
        "/etc/controlDict",
        "\\x",
        "C:/x",
        "system\\controlDict",
        "constant/polyMesh/points.gz",
        "libs/libfoo.so",
        "libs/libfoo.SO",
        "libs/libfoo.dylib",
        "libs/foo.DLL",
        "dynamicCode/x/Make/options",
        "system/codeTemplates/a",
        "system/DYNAMICCODE/a",
    ] {
        let r = tree_row(path, b"a 1;\n");
        assert_eq!(r.len(), 1, "{path}: {r:?}");
        assert_eq!(r[0].rule.code(), "CFD-GUARD-007", "{path}");
        assert_eq!(r[0].file, path);
    }
    let big = vec![b' '; 16 * 1024 * 1024 + 1];
    assert_eq!(codes(&tree_row("system/x", &big))[0].2, "CFD-GUARD-007");
    let edge = vec![b' '; 16 * 1024 * 1024];
    assert_eq!(tree_row("system/x", &edge), vec![]);
}

#[test]
fn a_compiled_tree_with_a_parent_path_is_refused() {
    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: PathBuf::from("../x"),
            bytes: b"a 1;\n".to_vec(),
        }],
    };
    assert_eq!(codes(&scan_tree(&tree))[0].2, "CFD-GUARD-007");
    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: PathBuf::from("/x"),
            bytes: b"a 1;\n".to_vec(),
        }],
    };
    assert_eq!(codes(&scan_tree(&tree))[0].2, "CFD-GUARD-007");
}

fn scratch(label: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("guard-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn the_directory_scanner_checks_dictionary_trees_and_lists_the_rest() {
    let root = scratch("layout");
    put(&root, "0/U", "a 1;\n");
    put(&root, "0.orig/U", "a 1;\n");
    put(&root, "constant/transportProperties", "a 1;\n");
    put(&root, "system/controlDict", "a 1;\n");
    put(&root, "Allrun", "#!/bin/sh\n#calc is not scanned here\n");
    put(&root, "README.md", "text\n");
    put(&root, "log/x", "type coded;\n");
    let s = scan_dir(&root).unwrap();
    assert_eq!(s.refusals, vec![]);
    assert_eq!(
        s.scanned,
        [
            "0.orig/U",
            "0/U",
            "constant/transportProperties",
            "system/controlDict"
        ]
    );
    assert_eq!(s.skipped, ["Allrun", "README.md", "log"]);

    put(
        &root,
        "system/controlDict",
        "a 1;\nfunctions { x #codeStream{ }; }\n",
    );
    let r = scan_dir(&root).unwrap().refusals;
    assert_eq!(codes(&r), vec![(2, "#codeStream", "CFD-GUARD-001")]);
    assert_eq!(r[0].file, "system/controlDict");
    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn the_directory_scanner_refuses_links_and_hidden_code_directories() {
    use std::os::unix::fs::symlink;
    for place in ["system/link", "link", "log/link", "0/deep/link"] {
        let root = scratch("link");
        put(&root, "system/controlDict", "a 1;\n");
        put(&root, "0/deep/U", "a 1;\n");
        put(&root, "log/x", "a 1;\n");
        symlink("/etc/hostname", root.join(place)).unwrap();
        let r = scan_dir(&root).unwrap().refusals;
        assert_eq!(r.len(), 1, "{place}: {r:?}");
        assert_eq!(
            (r[0].file.as_str(), r[0].rule.code()),
            (place, "CFD-GUARD-007")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    let root = scratch("dyn");
    put(&root, "system/controlDict", "a 1;\n");
    std::fs::create_dir_all(root.join("dynamicCode")).unwrap();
    put(&root, "system/lib.so", "x");
    let r = scan_dir(&root).unwrap().refusals;
    let files: Vec<&str> = r.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(files, ["dynamicCode", "system/lib.so"]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_directory_scanner_caps_the_depth() {
    let root = scratch("deep");
    let rel = (0..20)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/");
    put(&root, &format!("system/{rel}/f"), "a 1;\n");
    let r = scan_dir(&root).unwrap().refusals;
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].rule.code(), "CFD-GUARD-007");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_directory_scanner_passes_both_golden_trees() {
    for dir in ["cavity-v2512", "channel-v2512"] {
        let s = scan_dir(&golden(dir)).unwrap();
        assert_eq!(s.refusals, vec![], "{dir}");
        assert_eq!(s.scanned.len(), 8, "{dir}");
        assert_eq!(s.skipped, Vec::<String>::new(), "{dir}");
    }
}
