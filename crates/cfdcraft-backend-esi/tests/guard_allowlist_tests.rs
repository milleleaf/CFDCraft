//! The import scan accepts exactly listed `etc` includes and function names; every strict scan is
//! unchanged and every other construct stays refused.

use cfdcraft_backend_esi::{
    CaseFile, CaseTree,
    guard::{
        Allowlist, GuardFile, Refusal, scan_dir, scan_dir_leaving_out, scan_dir_with, scan_files,
        scan_files_with, scan_tree,
    },
};
use std::path::{Path, PathBuf};

const ETC: &str = "caseDicts/setConstraintTypes";

fn one(path: &str, text: &str) -> GuardFile {
    GuardFile {
        path: path.into(),
        bytes: text.as_bytes().to_vec(),
    }
}

fn rules(r: &[Refusal]) -> Vec<&'static str> {
    r.iter().map(|r| r.rule.code()).collect()
}

fn listing(etc: &[&str], functions: &[&str]) -> Allowlist {
    Allowlist::new(etc.iter().copied(), functions.iter().copied()).unwrap()
}

fn include_text(path: &str) -> String {
    format!("a 1;\n#includeEtc \"{path}\"\nb 2;\n")
}

fn function_text(call: &str) -> String {
    format!("functions\n{{\n    #includeFunc {call}\n}}\n")
}

fn scratch(label: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("guard-allow-{label}-{}", std::process::id()));
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
fn a_listed_etc_include_passes_in_an_import_scan() {
    let text = include_text(ETC);
    let file = one("system/extraDict", &text);

    let listed = listing(&[ETC], &[]);
    assert_eq!(
        scan_files_with(std::slice::from_ref(&file), &listed),
        vec![]
    );

    let other = listing(&["caseDicts/other"], &[]);
    assert_eq!(
        rules(&scan_files_with(std::slice::from_ref(&file), &other)),
        ["CFD-GUARD-001"]
    );
    assert_eq!(
        rules(&scan_files_with(
            std::slice::from_ref(&file),
            &Allowlist::empty()
        )),
        ["CFD-GUARD-001"]
    );

    // The bare form is read the same way.
    let bare = one(
        "system/extraDict",
        "#includeEtc caseDicts/setConstraintTypes\n",
    );
    assert_eq!(scan_files_with(&[bare], &listed), vec![]);
}

#[test]
fn the_strict_scans_are_unchanged() {
    let text = include_text(ETC);
    let file = one("system/extraDict", &text);
    assert_eq!(
        rules(&scan_files(std::slice::from_ref(&file))),
        ["CFD-GUARD-001"]
    );

    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: PathBuf::from("system/extraDict"),
            bytes: text.as_bytes().to_vec(),
        }],
    };
    assert_eq!(rules(&scan_tree(&tree)), ["CFD-GUARD-001"]);

    let root = scratch("strict");
    put(&root, "system/controlDict", "a 1;\n");
    put(&root, "system/extraDict", &text);
    assert_eq!(rules(&scan_dir(&root).unwrap().refusals), ["CFD-GUARD-001"]);
    assert_eq!(
        scan_dir(&root).unwrap(),
        scan_dir_with(&root, &Allowlist::empty()).unwrap()
    );
    // With the path listed the directory scan accepts it.
    assert_eq!(
        scan_dir_with(&root, &listing(&[ETC], &[]))
            .unwrap()
            .refusals,
        vec![]
    );
    // A function include is refused by the strict scans too.
    put(&root, "system/extraDict", &function_text("streamlinesLine"));
    assert_eq!(rules(&scan_dir(&root).unwrap().refusals), ["CFD-GUARD-001"]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_allowlist_entry_is_exact_and_safe() {
    for bad in [
        "",
        "../x",
        "caseDicts/../x",
        "a..b",
        "/etc/passwd",
        "caseDicts/`id`",
        "caseDicts/$x",
        "caseDicts/a b",
        "caseDicts/a\tb",
        "caseDicts/a;b",
        "caseDicts\\x",
        "~/x",
    ] {
        let e = Allowlist::new([bad], Vec::<&str>::new()).unwrap_err();
        assert_eq!(e.entry, bad, "{bad:?}");
        assert!(e.to_string().contains("allowlist entry"), "{bad:?}");
        let e = Allowlist::new(Vec::<&str>::new(), [bad]).unwrap_err();
        assert_eq!(e.entry, bad, "{bad:?}");
    }
    let ok = listing(&["caseDicts/a:b-c_d.e", ETC], &["streamlinesLine"]);
    assert_eq!(ok.len(), 3);
    assert!(!ok.is_empty());
    assert!(Allowlist::empty().is_empty());

    // A path that only starts like a listed one, or is a part of it, is refused.
    let listed = listing(&[ETC], &[]);
    for path in [
        "caseDicts/setConstraintTypes2",
        "caseDicts/setConstraintType",
        "caseDicts/setConstraintTypes/",
        "xcaseDicts/setConstraintTypes",
        "caseDicts",
        "CaseDicts/setConstraintTypes",
    ] {
        let text = include_text(path);
        let r = scan_files_with(&[one("system/x", &text)], &listed);
        assert_eq!(rules(&r), ["CFD-GUARD-001"], "{path}");
    }
}

#[test]
fn other_directives_stay_refused_with_a_list() {
    let listed = listing(&[ETC, "caseDicts/other"], &["streamlinesLine"]);
    let cases: [(&str, &str, &str); 8] = [
        ("codeStream", "a\n#codeStream\n{\n}\n", "CFD-GUARD-001"),
        ("calc", "a #calc \"1+1\";\n", "CFD-GUARD-001"),
        (
            "unlisted etc",
            "#includeEtc \"caseDicts/mesh/generation/x\"\n",
            "CFD-GUARD-001",
        ),
        (
            "outside include",
            "#include \"/etc/passwd\"\n",
            "CFD-GUARD-006",
        ),
        ("parent include", "#include \"../x\"\n", "CFD-GUARD-006"),
        (
            "variable in etc path",
            "#includeEtc \"$FOAM_ETC/caseDicts/x\"\n",
            "CFD-GUARD-001",
        ),
        ("verbatim", "a #{ code #};\n", "CFD-GUARD-001"),
        (
            "glued directive",
            "x 1#includeEtc \"caseDicts/setConstraintTypes\"\n",
            "CFD-GUARD-001",
        ),
    ];
    for (label, text, code) in cases {
        let r = scan_files_with(&[one("system/x", text)], &listed);
        assert!(!r.is_empty(), "{label} was accepted");
        assert_eq!(r[0].rule.code(), code, "{label}");
    }
    // A listed include does not make the lines around it safe.
    let text = format!("{}c #calc \"2\";\n", include_text(ETC));
    let r = scan_files_with(&[one("system/x", &text)], &listed);
    assert_eq!(rules(&r), ["CFD-GUARD-001"]);
    assert_eq!(r[0].line, 4);
    // Coded content stays refused even in a file that also holds a listed include.
    let text = format!("{}type coded;\n", include_text(ETC));
    let r = scan_files_with(&[one("system/x", &text)], &listed);
    assert_eq!(rules(&r), ["CFD-GUARD-002"]);
    // The write-side scans take no list.
    assert_eq!(
        rules(&scan_files(&[one("system/x", &include_text(ETC))])).len(),
        1
    );
}

#[test]
fn a_listed_function_passes_and_arguments_are_checked() {
    let listed = listing(&[], &["streamlinesLine", "writeObjects"]);
    for call in [
        "streamlinesLine",
        "writeObjects(kEpsilon:G)",
        "writeObjects(kEpsilon:G, p)",
        "\"streamlinesLine\"",
        "\"writeObjects(kEpsilon:G)\"",
    ] {
        let text = function_text(call);
        let r = scan_files_with(&[one("system/controlDict", &text)], &listed);
        assert_eq!(r, vec![], "{call}");
    }
    for call in [
        "otherFunction",
        "otherFunction(a)",
        "streamlinesLine2",
        "streamlinesLin",
        "\"other\"",
    ] {
        let text = function_text(call);
        let r = scan_files_with(&[one("system/controlDict", &text)], &listed);
        assert_eq!(rules(&r), ["CFD-GUARD-001"], "{call}");
    }
    for args in [
        "$x", "a$b", "`id`", "#calc", "a;b", "\"q\"", "a(b)", "{x}", "[x]", "a=b", "a/b",
    ] {
        let call = format!("writeObjects({args})");
        let text = function_text(&call);
        let r = scan_files_with(&[one("system/controlDict", &text)], &listed);
        assert!(!r.is_empty(), "{call} was accepted");
        let quoted = format!("\"writeObjects({args})\"");
        let text = function_text(&quoted);
        let r = scan_files_with(&[one("system/controlDict", &text)], &listed);
        assert!(!r.is_empty(), "{quoted} was accepted");
    }
    // An argument list that never closes is refused.
    let text = "functions\n{\n    #includeFunc writeObjects(a\n}\n";
    assert!(!scan_files_with(&[one("system/controlDict", text)], &listed).is_empty());
    // With no list, the strict scan refuses the same text.
    let text = function_text("streamlinesLine");
    assert_eq!(
        rules(&scan_files(&[one("system/controlDict", &text)])),
        ["CFD-GUARD-001"]
    );
}

fn left_out(name: &str, is_folder: bool) -> bool {
    if is_folder {
        name == "100" || name.starts_with("processor") || name == "postProcessing"
    } else {
        name.starts_with("log.")
    }
}

#[test]
fn a_left_out_folder_is_not_read_and_keeps_its_safety_rules() {
    let root = scratch("leftout");
    put(&root, "system/controlDict", "a 1;\n");
    put(&root, "0/U", "a 1;\n");
    put(&root, "100/U.gz", "not text");
    put(&root, "processor0/100/p.gz", "not text");
    put(&root, "log.simpleFoam.gz", "not text");
    // Never read: it would be refused as coded, and it is over the file size limit.
    put(&root, "100/p", "type coded;\n");
    std::fs::File::create(root.join("100/big"))
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();

    // The strict scan reads the time folder and refuses compressed files.
    let strict = scan_dir(&root).unwrap();
    assert!(
        strict.refusals.iter().any(|r| r.file == "100/U.gz"),
        "{:?}",
        strict.refusals
    );
    assert!(strict.refusals.iter().any(|r| r.file == "100/big"));
    assert!(strict.refusals.iter().any(|r| r.file == "100/p"));

    let none = Allowlist::empty();
    let s = scan_dir_leaving_out(&root, &none, &left_out).unwrap();
    assert_eq!(s.refusals, vec![]);
    assert_eq!(s.scanned, ["0/U", "system/controlDict"]);
    assert_eq!(s.skipped, ["100", "log.simpleFoam.gz", "processor0"]);

    // The same name in a folder that is read is still refused.
    put(&root, "constant/points.gz", "not text");
    let r = scan_dir_leaving_out(&root, &none, &left_out)
        .unwrap()
        .refusals;
    assert_eq!(
        r.iter().map(|r| r.file.as_str()).collect::<Vec<_>>(),
        ["constant/points.gz"]
    );
    assert_eq!(r[0].rule.code(), "CFD-GUARD-007");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_left_out_folder_still_refuses_libraries_and_unsafe_entries() {
    let root = scratch("leftout-unsafe");
    put(&root, "system/controlDict", "a 1;\n");
    put(&root, "100/lib.so", "x");
    std::fs::create_dir_all(root.join("100/dynamicCode")).unwrap();
    put(&root, "processor0/lib.dylib", "x");
    let none = Allowlist::empty();
    let r = scan_dir_leaving_out(&root, &none, &left_out)
        .unwrap()
        .refusals;
    let files: Vec<&str> = r.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(
        files,
        ["100/dynamicCode", "100/lib.so", "processor0/lib.dylib"]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_left_out_folder_still_refuses_links() {
    use std::os::unix::fs::symlink;
    for place in ["100/link", "100/deep/link", "processor0/link", "log.link"] {
        let root = scratch("leftout-link");
        put(&root, "system/controlDict", "a 1;\n");
        put(&root, "100/deep/U", "a 1;\n");
        put(&root, "processor0/x", "a 1;\n");
        symlink("/etc/hostname", root.join(place)).unwrap();
        let r = scan_dir_leaving_out(&root, &Allowlist::empty(), &left_out)
            .unwrap()
            .refusals;
        assert_eq!(r.len(), 1, "{place}: {r:?}");
        assert_eq!(
            (r[0].file.as_str(), r[0].rule.code()),
            (place, "CFD-GUARD-007")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
