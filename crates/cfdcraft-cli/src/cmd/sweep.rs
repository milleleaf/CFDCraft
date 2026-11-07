//! The verbs for sweeps: plan one, create its variants, and check every point before anything
//! is solved. A sweep is given as a file holding it as JSON.
//!
//! `plan` and `preflight` answer with data: their text output is the JSON data of the report on
//! standard output, and a point that fails is named on standard error. `create` prints a
//! sentence. Nothing here needs a compute host.

use std::io::Read;
use std::path::Path;

use cfdcraft_ops::sweep_ops::{sweep_create, sweep_plan, sweep_preflight};
use cfdcraft_sweep::Sweep;

use super::{Error, emit_doc, print_data};

/// The largest sweep file that is read.
const MAX_SPEC_BYTES: u64 = 1 << 20;

/// Reads a sweep from a JSON file. A file that is over 1 MiB, is not JSON, or holds anything but
/// a sweep is refused with a sentence that names the file as it was typed.
pub fn read_spec(path: &Path) -> Result<Sweep, Error> {
    let shown = path.display();
    let file = std::fs::File::open(path)
        .map_err(|e| format!("cannot read the sweep file {shown}: {e}"))?;
    let mut text = String::new();
    file.take(MAX_SPEC_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the sweep file {shown}: {e}"))?;
    if text.len() as u64 > MAX_SPEC_BYTES {
        return Err(format!("the sweep file {shown} is over 1 MiB").into());
    }
    let sweep = serde_json::from_str(&text)
        .map_err(|e| format!("the sweep file {shown} does not hold a sweep: {e}"))?;
    Ok(sweep)
}

/// The points a sweep would make; nothing is stored.
pub fn plan(doc: &Path, spec: &Path, json: bool) -> Result<i32, Error> {
    print_data(&sweep_plan(doc, &read_spec(spec)?), json)
}

/// Store the points of a sweep as variants.
pub fn create(doc: &Path, spec: &Path, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&sweep_create(doc, &read_spec(spec)?), json))
}

/// Check every point of a stored sweep without solving anything.
pub fn preflight(doc: &Path, sweep: &str, json: bool) -> Result<i32, Error> {
    print_data(&sweep_preflight(doc, sweep), json)
}
