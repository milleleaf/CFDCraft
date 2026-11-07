pub mod block_mesh;
pub mod case;
pub mod control_dict;
pub mod dialect;
pub mod evidence;
pub mod export;
pub mod guard;
pub mod raw;
pub mod reader;
pub mod transport;

pub use case::{CaseFile, CaseTree, CompileError, compile, write_to_disk};
pub use dialect::Dialect;
pub use evidence::{
    EvidenceExpectations, RunEvidence, SmokeExpectations, Verdict, judge, judge_run, judge_smoke,
};

/// The source files that decide what is written into a case and how it is run: the case files, the
/// patch table, the `Allrun` script, the two helpers every dictionary goes through, the step that
/// applies raw overrides and the dictionary editor it uses (a file of another crate, named by its
/// path from `src`). Their text is part of the tool identity of a stage key, so a change to what
/// the writer emits cannot leave results made before it looking up to date. A file that does not
/// shape the written case (the safety scan, the reader, the judging of a run) is deliberately not
/// in the list.
pub const WRITER_SOURCES: [(&str, &str); 8] = [
    ("block_mesh.rs", include_str!("block_mesh.rs")),
    ("case.rs", include_str!("case.rs")),
    ("control_dict.rs", include_str!("control_dict.rs")),
    ("export.rs", include_str!("export.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("raw.rs", include_str!("raw.rs")),
    ("transport.rs", include_str!("transport.rs")),
    (
        "../../cfdcraft-dictedit/src/lib.rs",
        include_str!("../../cfdcraft-dictedit/src/lib.rs"),
    ),
];

/// Locale-independent, shortest-round-trip finite scalar representation.
pub(crate) fn scalar(value: f64) -> String {
    ryu::Buffer::new()
        .format_finite(if value == 0.0 { 0.0 } else { value })
        .into()
}

pub(crate) fn dictionary(object: &str, class: &str, body: &str) -> String {
    format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class {class};\n    object {object};\n}}\n\n{body}"
    )
}
