use cfdcraft_backend::{Backend, CaseFile, CaseTree, CompileOptions};
use cfdcraft_backend_codesaturne::backends;
use cfdcraft_model::{ResolvedStudy, Study, resolve};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

fn clean_tree() -> CaseTree {
    let mut study: ResolvedStudy = resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap();
    study.mesh.cells_x = 2;
    study.mesh.cells_y = 2;
    backend()
        .compile(&study, &CompileOptions::default())
        .unwrap()
}

fn with_file(name: &str, bytes: &str) -> CaseTree {
    let mut tree = clean_tree();
    tree.files.push(CaseFile {
        rel_path: name.into(),
        bytes: bytes.as_bytes().to_vec(),
    });
    tree
}

/// The tree with `from` replaced by `to` in the setup.
fn with_setup_edit(from: &str, to: &str) -> CaseTree {
    let mut tree = clean_tree();
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path.ends_with("setup.xml"))
        .unwrap();
    let text = String::from_utf8(file.bytes.clone()).unwrap();
    assert!(text.contains(from), "the setup has no {from}");
    file.bytes = text.replacen(from, to, 1).into_bytes();
    tree
}

fn codes(tree: &CaseTree) -> Vec<&'static str> {
    backend().scan(tree).iter().map(|r| r.code).collect()
}

#[test]
fn the_guard_refuses_forbidden_files_and_formulas() {
    assert!(backend().scan(&clean_tree()).is_empty());

    for name in [
        "SRC/user.c",
        "DATA/user_scripts.py",
        "DATA/cs_user_scripts.py",
        "lib/solver.so",
        "lib/solver.dylib",
        "notes.txt",
        "DATA/run.cfg",
    ] {
        let found = backend().scan(&with_file(name, "x"));
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert_eq!(found[0].code, "CFD-GUARD-013", "{name}");
        assert_eq!(found[0].file, name);
    }

    let formula = with_setup_edit(
        "<calculation_management/>",
        "<calculation_management><my_formula>x</my_formula></calculation_management>",
    );
    assert_eq!(codes(&formula), ["CFD-GUARD-012"]);

    let compute = with_setup_edit(
        "<additional_scalars/>",
        "<additional_scalars compute=\"1\"/>",
    );
    assert_eq!(codes(&compute), ["CFD-GUARD-012"]);

    let function = with_setup_edit("choice=\"off\"", "choice=\"user_function\"");
    assert_eq!(codes(&function), ["CFD-GUARD-012"]);

    let expression = with_setup_edit(
        "<iterations>5000</iterations>",
        "<iterations>n=5000</iterations>",
    );
    assert_eq!(codes(&expression), ["CFD-GUARD-012"]);

    let entity = with_setup_edit(
        "<calculation_management/>",
        "<calculation_management>&#65;</calculation_management>",
    );
    assert_eq!(codes(&entity), ["CFD-GUARD-012"]);

    let doctype = with_setup_edit("<Code_Saturne_GUI", "<!DOCTYPE a><Code_Saturne_GUI");
    assert_eq!(codes(&doctype), ["CFD-GUARD-012"]);
}

#[test]
fn the_exported_files_pass_the_scan() {
    let study: ResolvedStudy = {
        let mut s = resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap();
        s.mesh.cells_x = 2;
        s.mesh.cells_y = 2;
        s
    };
    let options = CompileOptions::default();
    let tree = backend().compile(&study, &options).unwrap();
    let exported = backend().export(&study, &tree, "abc123", &options);
    assert_eq!(exported.files.len(), tree.files.len() + 2);
    assert!(backend().scan(&exported).is_empty());
}
