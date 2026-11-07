use super::{Error, emit, with_study};
use cfdcraft_ops::compile_study_on;
use std::path::Path;

/// Compile the study into `out`, for the backend with id `backend` (the default when none).
pub fn execute(study: &Path, out: &Path, backend: Option<&str>, json: bool) -> Result<i32, Error> {
    let report = with_study(study, "compile", |text| {
        compile_study_on(text, out, backend)
    });
    Ok(emit(&report, json, "compiled case", out))
}
