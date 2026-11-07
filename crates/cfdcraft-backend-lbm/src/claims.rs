//! The benchmarks this backend is claimed for. The list is empty: a result of this backend is a
//! preview until a recorded gate pass says otherwise for a named case.

/// One case the backend is claimed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub benchmark: &'static str,
    pub case: &'static str,
    /// Where the recorded gate pass is written down. An entry without it does not count.
    pub evidence: &'static str,
}

// An entry is added only together with a recorded gate pass for that benchmark and case, as a
// reviewed change of this constant.
pub const CLAIMS: [Claim; 0] = [];

/// Whether the backend is claimed for this benchmark and case.
pub fn claimed(benchmark: &str, case: &str) -> bool {
    claimed_in(&CLAIMS, benchmark, case)
}

/// [`claimed`] over any list: an entry counts only when it names its evidence.
pub fn claimed_in(claims: &[Claim], benchmark: &str, case: &str) -> bool {
    claims.iter().any(|claim| {
        claim.benchmark == benchmark && claim.case == case && !claim.evidence.trim().is_empty()
    })
}
