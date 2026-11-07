/// One construct a backend's safety scan refused in a written case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The stable code of the rule that refused it.
    pub code: &'static str,
    /// The file, as a `/`-separated path from the case root.
    pub file: String,
    /// 1-based.
    pub line: u32,
    /// The offending token, shortened by the scan.
    pub entry: String,
    pub message: String,
}
