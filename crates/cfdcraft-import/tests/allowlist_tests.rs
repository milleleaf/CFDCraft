//! The per-image lists: they ship empty, load strictly, and the import picks one by dialect.

mod support;

use cfdcraft_backend_esi::dialect;
use cfdcraft_import::{
    Allowlist, GUARD_POLICY, ImportOptions, ImportOutcome, Record, TABLE, TableError,
    allowlist_for, image_id_for_dialect, import_case, parse_table,
};
use support::{Scratch, dict, put, write_dialect_case};

const ETC: &str = "caseDicts/setConstraintTypes";

fn listing(etc: &[&str]) -> Allowlist {
    Allowlist::new(etc.iter().copied(), Vec::<&str>::new()).unwrap()
}

/// An esi-style case whose `0/U` includes one etc file.
fn esi_case(s: &Scratch) {
    write_dialect_case(
        &s.source(),
        "application",
        &["transportProperties", "turbulenceProperties"],
    );
    put(
        &s.source(),
        "0/U",
        &dict("U", &format!("#includeEtc \"{ETC}\"")),
    );
}

fn imported(s: &Scratch, options: &ImportOptions) -> Record {
    match import_case(&s.source(), &s.target(), options).unwrap() {
        ImportOutcome::Imported(record) => record,
        other => panic!("the case was not imported: {other:?}"),
    }
}

#[test]
fn the_tables_load_and_are_empty_until_reviewed() {
    for id in ["esi-v2512", "esi-v2606", "of14-20261008"] {
        let list = allowlist_for(id).unwrap_or_else(|| panic!("no list for {id}"));
        assert_eq!(list.len(), 0, "{id}");
    }
    assert!(allowlist_for("su2-8.6.0").is_none());
    assert!(allowlist_for("").is_none());

    let rows: Vec<(&str, &str)> = TABLE.iter().map(|r| (r.id, r.image)).collect();
    assert_eq!(
        rows,
        [
            (
                "esi-v2512",
                "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319"
            ),
            (
                "esi-v2606",
                "opencfd/openfoam-run@sha256:4229997e74defb81548222d511b8e3b95b98305e5df41b8e88b031813fe47eeb"
            ),
            ("of14-20261008", "none"),
        ]
    );
    assert_eq!(TABLE[0].image, dialect::IMAGE);
    assert_eq!(TABLE[1].image, dialect::IMAGE_V2606);
    for row in TABLE {
        assert!(
            !row.text
                .lines()
                .any(|l| l.starts_with("etc ") || l.starts_with("func ")),
            "{} already lists an entry",
            row.id
        );
    }
}

#[test]
fn a_table_text_is_read_strictly() {
    let list =
        parse_table("# comment\n\netc caseDicts/a\nfunc streamlinesLine\n  etc caseDicts/b  \n")
            .unwrap();
    assert_eq!(
        list.etc_includes().collect::<Vec<_>>(),
        ["caseDicts/a", "caseDicts/b"]
    );
    assert_eq!(list.functions().collect::<Vec<_>>(), ["streamlinesLine"]);

    match parse_table("etc caseDicts/a\nlibs foo\n") {
        Err(TableError::Line { line }) => assert_eq!(line, 2),
        other => panic!("{other:?}"),
    }
    for bad in [
        "etc ../x", "etc /abs", "etc a b", "func $x", "etc a;b", "etc ",
    ] {
        let text = format!("# first\n{bad}\n");
        match parse_table(&text) {
            Err(TableError::Entry { line, .. }) | Err(TableError::Line { line }) => {
                assert_eq!(line, 2, "{bad}")
            }
            other => panic!("{bad:?} was accepted: {other:?}"),
        }
    }
}

#[test]
fn a_dialect_maps_to_one_image_list_or_none() {
    assert_eq!(image_id_for_dialect("foundation-14"), Some("of14-20261008"));
    assert_eq!(image_id_for_dialect("esi"), Some("esi-v2512"));
    assert_eq!(image_id_for_dialect("foundation-legacy"), None);
    assert_eq!(image_id_for_dialect("unknown"), None);
}

#[test]
fn import_uses_the_allowlist_for_the_detected_image() {
    // No entry is pinned, so the include is refused and nothing is written.
    let s = Scratch::new("allow-refused");
    esi_case(&s);
    match import_case(&s.source(), &s.target(), &ImportOptions::default()).unwrap() {
        ImportOutcome::Refused { refusals, .. } => {
            assert_eq!(refusals.len(), 1, "{refusals:?}");
            assert_eq!(refusals[0].code, "CFD-GUARD-001");
            assert_eq!(refusals[0].file, "0/U");
        }
        other => panic!("the case was not refused: {other:?}"),
    }
    assert!(!s.target().exists());

    // A list handed through the options is used for the detected dialect.
    let options = ImportOptions {
        allowlist: Some(listing(&[ETC])),
        ..ImportOptions::default()
    };
    let s = Scratch::new("allow-esi");
    esi_case(&s);
    let record = imported(&s, &options);
    assert_eq!(record.inventory.dialect, "esi");
    assert_eq!(record.guard_policy, "allowlist:esi-v2512:1");
    assert!(s.target().join("original/0/U").is_file());

    // A list that does not hold the path still refuses it.
    let other = ImportOptions {
        allowlist: Some(listing(&["caseDicts/other"])),
        ..ImportOptions::default()
    };
    let s = Scratch::new("allow-other");
    esi_case(&s);
    assert!(matches!(
        import_case(&s.source(), &s.target(), &other).unwrap(),
        ImportOutcome::Refused { .. }
    ));
}

#[test]
fn the_policy_names_the_image_of_the_dialect() {
    let options = ImportOptions {
        allowlist: Some(listing(&[ETC, "caseDicts/other"])),
        ..ImportOptions::default()
    };

    let s = Scratch::new("policy-of14");
    write_dialect_case(
        &s.source(),
        "solver",
        &["physicalProperties", "momentumTransport"],
    );
    let record = imported(&s, &options);
    assert_eq!(record.inventory.dialect, "foundation-14");
    assert_eq!(record.guard_policy, "allowlist:of14-20261008:2");

    // A dialect without an image keeps a handed list under a neutral name.
    let s = Scratch::new("policy-custom");
    write_dialect_case(
        &s.source(),
        "application",
        &["transportProperties", "momentumTransport"],
    );
    let record = imported(&s, &options);
    assert_eq!(record.inventory.dialect, "foundation-legacy");
    assert_eq!(record.guard_policy, "allowlist:custom:2");

    // With no list the policy is the strict one, whatever the dialect.
    let s = Scratch::new("policy-strict");
    esi_case(&s);
    put(&s.source(), "0/U", &dict("U", "a 1;"));
    let record = imported(&s, &ImportOptions::default());
    assert_eq!(record.guard_policy, GUARD_POLICY);
    let s = Scratch::new("policy-empty");
    esi_case(&s);
    put(&s.source(), "0/U", &dict("U", "a 1;"));
    let empty = ImportOptions {
        allowlist: Some(Allowlist::empty()),
        ..ImportOptions::default()
    };
    assert_eq!(imported(&s, &empty).guard_policy, GUARD_POLICY);
}
