//! Linked files: relative links that survive moving the project folder, and errors that say what
//! is missing and where it was expected.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::{
    ArtifactIndex, Document, Link, LinkError, check_links, open, relative_path, resolve, save,
    store_dir, verify,
};
use cfdcraft_model::Study;
use cfdcraft_store::{ContentHash, Store};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn document() -> Document {
    Document::new(Study::from_json(CAVITY).unwrap())
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("links-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn step_bytes() -> Vec<u8> {
    (0..4096u32).map(|i| (i % 251) as u8).collect()
}

/// A project folder `proj` with a study folder and a geometry folder, inside a fresh base folder.
/// Returns the base folder, the study file path and the linked file path.
fn project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = fresh(name);
    let proj = base.join("proj");
    fs::create_dir_all(proj.join("studies")).unwrap();
    fs::create_dir_all(proj.join("geometry")).unwrap();
    let step = proj.join("geometry").join("room.step");
    fs::write(&step, step_bytes()).unwrap();
    (base, proj.join("studies").join("cavity.cfdcraft"), step)
}

/// Move the whole `proj` folder to `moved/proj` and return the study file's new path.
fn move_project(base: &Path) -> PathBuf {
    let moved = base.join("moved");
    fs::create_dir_all(&moved).unwrap();
    fs::rename(base.join("proj"), moved.join("proj")).unwrap();
    moved.join("proj").join("studies").join("cavity.cfdcraft")
}

fn root() -> PathBuf {
    PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" })
}

#[test]
fn a_link_still_resolves_after_the_project_folder_moves() {
    let (base, doc_path, step) = project("move");
    let link = Link::to_file(&doc_path, &step, "room").unwrap();
    assert_eq!(link.id, "room");
    assert_eq!(link.path, "../geometry/room.step");
    assert_eq!(link.name, "room.step");
    assert_eq!(link.hash, ContentHash::of(&step_bytes()));

    let mut doc = document();
    doc.add_link(link).unwrap();
    save(&doc, &doc_path).unwrap();

    let moved_doc = move_project(&base);
    let opened = open(&moved_doc).unwrap();
    let moved_link = &opened.document.artifacts.links[0];
    let expected = base
        .join("moved")
        .join("proj")
        .join("geometry")
        .join("room.step");
    assert_eq!(resolve(&moved_doc, moved_link).unwrap(), expected);
    verify(&moved_doc, moved_link).unwrap();
}

#[test]
fn a_link_survives_save_and_open_with_a_plain_stored_path() {
    let (_base, doc_path, step) = project("roundtrip");
    let link = Link::to_file(&doc_path, &step, "room").unwrap();
    assert!(!link.path.contains('\\'));
    assert!(!link.path.contains(':'));

    let mut doc = document();
    doc.add_link(link.clone()).unwrap();
    save(&doc, &doc_path).unwrap();
    let opened = open(&doc_path).unwrap();
    assert_eq!(
        opened.document.artifacts.links.as_slice(),
        std::slice::from_ref(&link)
    );
}

#[test]
fn relative_paths_climb_and_descend_with_forward_slashes() {
    let q = root().join("p").join("q");
    assert_eq!(relative_path(&q, &q.join("f")).as_deref(), Some("f"));
    assert_eq!(
        relative_path(&q, &q.join("sub").join("f")).as_deref(),
        Some("sub/f")
    );
    assert_eq!(
        relative_path(&q, &root().join("p").join("sibling").join("f")).as_deref(),
        Some("../sibling/f")
    );
    assert_eq!(
        relative_path(&q, &root().join("f")).as_deref(),
        Some("../../f")
    );
    assert_eq!(relative_path(&q, &q).as_deref(), Some("."));
    // `..` inside the inputs is folded without asking the file system.
    let odd = q.join("..").join("z");
    assert_eq!(
        relative_path(&odd, &root().join("p").join("z").join("f")).as_deref(),
        Some("f")
    );
    // Relative inputs have no answer.
    assert_eq!(relative_path(Path::new("p/q"), &q.join("f")), None);
    assert_eq!(relative_path(&q, Path::new("f")), None);
}

#[cfg(windows)]
#[test]
fn files_on_different_drives_have_no_relative_path() {
    assert_eq!(
        relative_path(Path::new(r"C:\a"), Path::new(r"D:\b\f")),
        None
    );
}

#[test]
fn the_store_beside_the_document_moves_with_the_project_folder() {
    let (base, doc_path, _step) = project("store");
    let store = Store::create(store_dir(&doc_path)).unwrap();
    let hash = store.put_bytes(b"a stored result").unwrap();
    assert!(store.contains(&hash));

    let moved_doc = move_project(&base);
    let moved_store = Store::open(store_dir(&moved_doc)).unwrap();
    assert!(moved_store.contains(&hash));
}

/// A link written by hand, as another tool might write it.
fn hand_link(id: &str, path: &str, name: &str) -> Link {
    Link {
        id: id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        hash: ContentHash::of(&step_bytes()),
    }
}

#[test]
fn a_missing_link_names_the_file_and_where_it_was_expected() {
    let (base, doc_path, step) = project("missing");
    let link = Link::to_file(&doc_path, &step, "room").unwrap();
    let moved_doc = move_project(&base);
    let expected = base
        .join("moved")
        .join("proj")
        .join("geometry")
        .join("room.step");
    fs::remove_file(&expected).unwrap();

    let Err(error) = resolve(&moved_doc, &link) else {
        panic!("a deleted file must not resolve");
    };
    let text = error.to_string();
    let LinkError::Missing { name, expected: at } = error else {
        panic!("expected a missing-file error, got: {text}");
    };
    assert_eq!(name, "room.step");
    assert_eq!(at, expected);
    assert!(text.contains("room.step"));
    assert!(text.contains(&expected.display().to_string()));
    assert!(text.contains("is missing; it was expected at"));
}

#[test]
fn links_to_the_web_other_computers_devices_or_above_the_root_are_refused() {
    let base = fresh("refused");
    let doc_path = base.join("cavity.cfdcraft");
    let climbing = format!("{}x.step", "../".repeat(64));
    let forms = [
        "https://example.com/a.step",
        "//server/share/a.step",
        "file://host/x.step",
        "NUL",
        r"\\.\pipe\x",
        climbing.as_str(),
    ];
    for form in forms {
        let link = hand_link("hostile", form, "a.step");
        match resolve(&doc_path, &link) {
            Err(LinkError::Refused { name, reason }) => {
                assert_eq!(name, "a.step", "{form}");
                assert!(!reason.is_empty(), "{form}");
            }
            other => panic!("{form} should be refused, got {other:?}"),
        }
        let text = resolve(&doc_path, &link).unwrap_err().to_string();
        assert!(text.contains("a.step"), "{text}");
        assert!(text.contains("cannot be followed"), "{text}");
    }
}

#[test]
fn a_changed_file_still_resolves_but_verify_reports_it() {
    let (_base, doc_path, step) = project("changed");
    let link = Link::to_file(&doc_path, &step, "room").unwrap();
    verify(&doc_path, &link).unwrap();

    fs::write(&step, b"different bytes").unwrap();
    assert_eq!(resolve(&doc_path, &link).unwrap(), step);
    match verify(&doc_path, &link) {
        Err(LinkError::Changed { name }) => assert_eq!(name, "room.step"),
        other => panic!("expected a changed-file error, got {other:?}"),
    }
}

#[test]
fn check_links_reports_each_link_in_order_without_reading_files() {
    let (_base, doc_path, step) = project("check");
    let good = Link::to_file(&doc_path, &step, "good").unwrap();
    let gone = hand_link("gone", "../geometry/gone.step", "gone.step");
    let web = hand_link("web", "https://example.com/a.step", "a.step");
    let mut index = ArtifactIndex::default();
    index.links.extend([good, gone, web]);

    // A changed file is not noticed here: resolving does not read the content.
    fs::write(&step, b"different bytes").unwrap();

    let checks = check_links(&doc_path, &index);
    let ids: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["good", "gone", "web"]);
    let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["room.step", "gone.step", "a.step"]);
    assert_eq!(checks[0].result.as_ref().unwrap(), &step);
    assert!(matches!(checks[1].result, Err(LinkError::Missing { .. })));
    assert!(matches!(checks[2].result, Err(LinkError::Refused { .. })));
}

#[test]
fn ids_are_validated_and_duplicates_are_refused() {
    let (_base, doc_path, step) = project("ids");
    let too_long = "a".repeat(41);
    for bad in ["", "Room", too_long.as_str(), "a/b", "-room"] {
        match Link::to_file(&doc_path, &step, bad) {
            Err(LinkError::BadId(id)) => assert_eq!(id, bad),
            other => panic!("{bad:?} should be refused, got {other:?}"),
        }
    }
    assert!(Link::to_file(&doc_path, &step, &"a".repeat(40)).is_ok());

    let mut doc = document();
    let link = Link::to_file(&doc_path, &step, "room").unwrap();
    doc.add_link(link.clone()).unwrap();
    assert!(matches!(
        doc.add_link(link.clone()),
        Err(LinkError::DuplicateId(id)) if id == "room"
    ));
    assert_eq!(doc.artifacts.links.as_slice(), std::slice::from_ref(&link));

    let mut doc = document();
    assert!(matches!(
        doc.add_link(hand_link("Bad Id", "f", "f")),
        Err(LinkError::BadId(_))
    ));
    assert!(doc.artifacts.links.is_empty());
}

#[test]
fn a_target_that_is_a_folder_or_does_not_exist_is_refused_by_name() {
    let (base, doc_path, _step) = project("targets");
    for target in [
        base.join("proj").join("geometry"),
        base.join("nothing.step"),
    ] {
        let Err(error) = Link::to_file(&doc_path, &target, "room") else {
            panic!("{} should not be linkable", target.display());
        };
        assert!(matches!(error, LinkError::NotAFile { .. }));
        assert!(error.to_string().contains(&target.display().to_string()));
    }
}

#[test]
fn a_link_written_with_backslashes_resolves_to_the_same_file() {
    let (_base, doc_path, step) = project("backslash");
    let slash = hand_link("slash", "../geometry/room.step", "room.step");
    let back = hand_link("back", r"..\geometry\room.step", "room.step");
    assert_eq!(resolve(&doc_path, &slash).unwrap(), step);
    assert_eq!(resolve(&doc_path, &back).unwrap(), step);
}
