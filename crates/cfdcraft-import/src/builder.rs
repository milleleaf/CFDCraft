//! A candidate list built from what an image's `etc/caseDicts` probe printed. The result is only a
//! starting point for a review: nothing built here is used until its entries are copied into the
//! image's table by hand.
//!
//! The probe prints one `LIST <path>` line for every file under `etc/caseDicts` and one
//! `HIT <path> <construct>` line for every file holding a construct that runs code or loads a
//! library. A file with any hit is never a candidate, and neither is anything under
//! `functionTemplates`, whose files are coded or are filled in with coded text.

use cfdcraft_backend_esi::guard::Allowlist;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

const ETC_ROOT: &str = "etc/caseDicts/";
/// Folders whose files are function objects, by the path below the folder.
const FUNCTION_FOLDERS: [&str; 2] = ["functions", "postProcessing"];

/// One file left out of the candidate and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    /// The path as the probe printed it.
    pub path: String,
    /// The constructs the probe found, or the rule that left the file out.
    pub reasons: Vec<String>,
}

/// The candidate lists. Every member is sorted and without repeats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    /// Include paths below `etc`, for example `caseDicts/setConstraintTypes`.
    pub etc: Vec<String>,
    /// Function names: the path of a file below `functions` or `postProcessing`.
    // UNPROBED: the exact name OpenFOAM looks up for a function file (with or without a folder
    // part, and whether the folder name belongs to it) has not been seen on a real run. A
    // reviewer checks every name against a case that runs before pinning it.
    pub functions: Vec<String>,
    pub excluded: Vec<Excluded>,
}

fn under_function_folder(rest: &str) -> Option<&str> {
    FUNCTION_FOLDERS.iter().find_map(|folder| {
        rest.strip_prefix(folder)
            .and_then(|r| r.strip_prefix('/'))
            .filter(|name| !name.is_empty())
    })
}

/// Reads the `LIST` and `HIT` lines of the probe; every other line is ignored.
pub fn build_candidate(probe_text: &str) -> Candidate {
    let mut listed = BTreeSet::new();
    let mut hits: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for line in probe_text.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("LIST ") {
            listed.insert(path.trim());
        } else if let Some(rest) = line.strip_prefix("HIT ") {
            let (path, construct) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            hits.entry(path).or_default().insert(construct.trim());
        }
    }
    let mut out = Candidate::default();
    for path in listed {
        let Some(rest) = path.strip_prefix(ETC_ROOT) else {
            continue;
        };
        let mut reasons: Vec<String> = hits
            .get(path)
            .map(|set| {
                set.iter()
                    .map(|c| {
                        if c.is_empty() {
                            "construct".to_string()
                        } else {
                            (*c).to_string()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if rest.split('/').any(|c| c == "functionTemplates") {
            reasons.push("under functionTemplates".to_string());
        }
        let below_etc = &path["etc/".len()..];
        let (entry, is_function) = match under_function_folder(rest) {
            Some(name) => (name, true),
            None => (below_etc, false),
        };
        // The same check the table loader applies, so a rendered candidate always loads.
        if Allowlist::new([entry], Vec::<&str>::new()).is_err() {
            reasons.push("name the list cannot hold".to_string());
        }
        if !reasons.is_empty() {
            out.excluded.push(Excluded {
                path: path.to_string(),
                reasons,
            });
        } else if is_function {
            out.functions.push(entry.to_string());
        } else {
            out.etc.push(entry.to_string());
        }
    }
    out.etc.sort();
    out.etc.dedup();
    out.functions.sort();
    out.functions.dedup();
    out
}

impl Candidate {
    /// The data-file text: the entries, then the excluded files as comment lines, so the whole
    /// text still loads as a table.
    pub fn render(&self) -> String {
        let mut text = String::from(
            "# Candidate list from the etc probe. Review every entry before copying it into a table.\n\
             # The function names are UNPROBED: check each against a case that runs.\n",
        );
        for path in &self.etc {
            let _ = writeln!(text, "etc {path}");
        }
        for name in &self.functions {
            let _ = writeln!(text, "func {name}");
        }
        text.push_str("# excluded\n");
        for e in &self.excluded {
            let _ = writeln!(text, "# {}: {}", e.path, e.reasons.join(", "));
        }
        text
    }
}
