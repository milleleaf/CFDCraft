pub mod adapter;
pub mod block_mesh;
pub mod case;
pub mod control_dict;
pub mod dialect;
pub mod evidence;
pub mod export;
pub mod fields;
pub mod guard;
pub mod monitors;
pub mod patches;
pub mod raw;
pub mod reader;
pub mod rows;
pub mod solver_plan;
pub mod transport;

pub use case::{CaseFile, CaseTree, CompileError, compile, write_to_disk};
pub use dialect::Dialect;
pub use evidence::{
    EvidenceExpectations, EvidencePlan, RunEvidence, SmokeExpectations, Verdict, judge,
    judge_partial_with, judge_run, judge_run_with, judge_smoke, judge_smoke_with,
};
pub use reader::expected::shaped_expected_facts;
pub use solver_plan::{ResidualLimit, SolverPlan};

/// The source files that decide what is written into a case and how it is run: the case files, the
/// patch table, the solver plan, the turbulence, buoyancy and numerics files, the `Allrun` script, the two helpers every dictionary goes
/// through, the step that applies raw overrides and the dictionary editor it uses (a file of
/// another crate, named by its path from `src`). Their text is part of the tool identity of a
/// stage key, so a change to what the writer emits cannot leave results made before it looking up
/// to date. A file that does not shape the written case (the safety scan, the reader, the judging
/// of a run) is deliberately not in the list.
pub const WRITER_SOURCES: [(&str, &str); 12] = [
    ("block_mesh.rs", include_str!("block_mesh.rs")),
    ("case.rs", include_str!("case.rs")),
    ("control_dict.rs", include_str!("control_dict.rs")),
    ("export.rs", include_str!("export.rs")),
    ("fields.rs", include_str!("fields.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("monitors.rs", include_str!("monitors.rs")),
    ("patches.rs", include_str!("patches.rs")),
    ("raw.rs", include_str!("raw.rs")),
    ("solver_plan.rs", include_str!("solver_plan.rs")),
    ("transport.rs", include_str!("transport.rs")),
    (
        "../../cfdcraft-dictedit/src/lib.rs",
        include_str!("../../cfdcraft-dictedit/src/lib.rs"),
    ),
];

/// Locale-independent, shortest-round-trip finite scalar representation.
pub fn scalar(value: f64) -> String {
    ryu::Buffer::new()
        .format_finite(if value == 0.0 { 0.0 } else { value })
        .into()
}

pub fn dictionary(object: &str, class: &str, body: &str) -> String {
    format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class {class};\n    object {object};\n}}\n\n{body}"
    )
}
