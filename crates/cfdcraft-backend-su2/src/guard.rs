//! The safety scan of a written SU2 case.
//!
//! A configuration key can name a file to read or write or a program to run, and the solver runs
//! with the case folder as its working directory. So the scan is an allowlist, not a search for bad
//! words: a key that is not listed is refused (`CFD-GUARD-010`), a value that holds anything but
//! plain words and numbers is refused (`CFD-GUARD-011`), and a case holds exactly three files.
//! The scan reads every line of every file and passes nothing it does not recognise.

use crate::{
    inlet::{COLUMNS, COLUMNS_TURBULENCE},
    markers::{CONFIG_FILE, INLET_FILE, MESH_FILE},
};
use cfdcraft_backend::{CaseTree, Refusal};

/// Every key the writer can emit. A key outside this list is refused.
pub const ALLOWED_KEYS: &[&str] = &[
    "SOLVER",
    "KIND_TURB_MODEL",
    "MATH_PROBLEM",
    "RESTART_SOL",
    "KIND_INCOMP_SYSTEM",
    "INC_NONDIM",
    "INC_DENSITY_MODEL",
    "INC_DENSITY_INIT",
    "INC_VELOCITY_INIT",
    "INC_TEMPERATURE_INIT",
    "INC_ENERGY_EQUATION",
    "VISCOSITY_MODEL",
    "MU_CONSTANT",
    "THERMAL_EXPANSION_COEFF",
    "PRANDTL_LAM",
    "PRANDTL_TURB",
    "FREESTREAM_TURBULENCEINTENSITY",
    "FREESTREAM_TURB2LAMVISCRATIO",
    "INC_INLET_TYPE",
    "MARKER_INLET",
    "INC_OUTLET_TYPE",
    "MARKER_OUTLET",
    "SPECIFIED_INLET_PROFILE",
    "INLET_FILENAME",
    "SURFACE_MOVEMENT",
    "MARKER_MOVING",
    "SURFACE_TRANSLATION_RATE",
    "MARKER_HEATFLUX",
    "MARKER_ISOTHERMAL",
    "MARKER_EULER",
    "MARKER_SYM",
    "NUM_METHOD_GRAD",
    "CFL_NUMBER",
    "CFL_ADAPT",
    "CONV_NUM_METHOD_FLOW",
    "MUSCL_FLOW",
    "SLOPE_LIMITER_FLOW",
    "TIME_DISCRE_FLOW",
    "LINEAR_SOLVER",
    "LINEAR_SOLVER_PREC",
    "LINEAR_SOLVER_ERROR",
    "LINEAR_SOLVER_ITER",
    "ITER",
    "CONV_FIELD",
    "CONV_RESIDUAL_MINVAL",
    "CONV_STARTITER",
    "TIME_DOMAIN",
    "TIME_MARCHING",
    "TIME_STEP",
    "MAX_TIME",
    "TIME_ITER",
    "INNER_ITER",
    "OUTPUT_WRT_FREQ",
    "MESH_FILENAME",
    "MESH_FORMAT",
    "CONV_FILENAME",
    "TABULAR_FORMAT",
    "VOLUME_FILENAME",
    "OUTPUT_FILES",
    "OUTPUT_PRECISION",
    "HISTORY_OUTPUT",
    "SCREEN_OUTPUT",
];

/// The only file names a case may hold.
pub const ALLOWED_FILES: [&str; 3] = [CONFIG_FILE, MESH_FILE, INLET_FILE];

/// The keywords of the mesh file.
const MESH_KEYWORDS: [&str; 6] = [
    "NDIME",
    "NELEM",
    "NPOIN",
    "NMARK",
    "MARKER_TAG",
    "MARKER_ELEMS",
];
/// The keywords of the profile file.
const PROFILE_KEYWORDS: [&str; 4] = ["NMARK", "MARKER_TAG", "NROW", "NCOL"];

fn short(text: &str) -> String {
    text.chars().take(60).collect()
}

/// Why a value is refused, or `None` when it holds plain words and numbers only: letters, digits
/// and `_ . - + , ( ) =` and spaces, with no `..`.
fn value_problem(value: &str) -> Option<&'static str> {
    if value.contains("..") {
        return Some("a value holds a parent folder reference");
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || " _.-+,()=".contains(c);
    if value.chars().all(plain) {
        None
    } else {
        Some("a value holds a path or a character that is not a plain word or number")
    }
}

fn refusal(code: &'static str, file: &str, line: usize, entry: &str, message: &str) -> Refusal {
    Refusal {
        code,
        file: file.to_owned(),
        line: u32::try_from(line).unwrap_or(u32::MAX),
        entry: short(entry),
        message: message.to_owned(),
    }
}

fn bad_value(file: &str, line: usize, entry: &str, why: &str) -> Refusal {
    refusal("CFD-GUARD-011", file, line, entry, why)
}

fn unknown(file: &str, line: usize, entry: &str, message: &str) -> Refusal {
    refusal("CFD-GUARD-010", file, line, entry, message)
}

/// The lines of a text, each with its 1-based number; the empty piece after the last newline is
/// not a line.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split('\n')
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.is_empty())
}

fn split_entry(line: &str) -> Option<(&str, &str)> {
    // Only spaces are trimmed: a carriage return or a tab at the end of a value is refused with it.
    line.split_once('=')
        .map(|(k, v)| (k.trim_matches(' '), v.trim_matches(' ')))
}

fn scan_config(text: &str, file: &str, found: &mut Vec<Refusal>) {
    for (n, line) in lines(text) {
        let Some((key, value)) = split_entry(line) else {
            found.push(unknown(
                file,
                n,
                line,
                "a line of the configuration is not a key and a value",
            ));
            continue;
        };
        if !ALLOWED_KEYS.contains(&key) {
            found.push(unknown(
                file,
                n,
                key,
                "a configuration key is not on the allowlist",
            ));
        }
        if let Some(why) = value_problem(value) {
            found.push(bad_value(file, n, line, why));
        }
    }
}

fn numeric(line: &str) -> bool {
    line.chars()
        .all(|c| c.is_ascii_digit() || " .-+eE".contains(c))
}

/// A file made of keyword lines and number lines: the mesh and the inlet profile.
fn scan_data(
    text: &str,
    file: &str,
    keywords: &[&str],
    header: Option<&str>,
    found: &mut Vec<Refusal>,
) {
    for (n, line) in lines(text) {
        if header.is_some_and(|h| line == h || line == format!("{h}{COLUMNS_TURBULENCE}")) {
            continue;
        }
        if let Some((key, value)) = split_entry(line) {
            if !keywords.contains(&key) {
                found.push(unknown(
                    file,
                    n,
                    key,
                    "a keyword of the file is not allowed",
                ));
            }
            if let Some(why) = value_problem(value) {
                found.push(bad_value(file, n, line, why));
            }
        } else if !numeric(line) {
            found.push(unknown(
                file,
                n,
                line,
                "a line of the file is not a keyword or numbers",
            ));
        }
    }
}

/// Every construct of the case the scan refuses; empty when the case is clean.
pub fn scan_tree(tree: &CaseTree) -> Vec<Refusal> {
    let mut found = Vec::new();
    for case_file in &tree.files {
        let name = case_file.rel_path.to_string_lossy().replace('\\', "/");
        let text = String::from_utf8_lossy(&case_file.bytes);
        match name.as_str() {
            CONFIG_FILE => scan_config(&text, &name, &mut found),
            MESH_FILE => scan_data(&text, &name, &MESH_KEYWORDS, None, &mut found),
            INLET_FILE => scan_data(&text, &name, &PROFILE_KEYWORDS, Some(COLUMNS), &mut found),
            _ => found.push(unknown(
                &name,
                0,
                &name,
                "a case holds the configuration, the mesh and the inlet profile only",
            )),
        }
    }
    found
}
