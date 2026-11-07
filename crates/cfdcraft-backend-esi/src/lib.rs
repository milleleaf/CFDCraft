pub mod block_mesh;
pub mod case;
pub mod control_dict;
pub mod dialect;
pub mod evidence;
pub mod export;
pub mod guard;
pub mod reader;
pub mod transport;

pub use case::{CaseFile, CaseTree, CompileError, compile, write_to_disk};
pub use dialect::Dialect;
pub use evidence::{
    EvidenceExpectations, RunEvidence, SmokeExpectations, Verdict, judge, judge_run, judge_smoke,
};

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
