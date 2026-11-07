/// How a case is written beyond what the study says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileOptions {
    /// Number of processes the solver runs on; at least one.
    pub np: u32,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self { np: 1 }
    }
}
