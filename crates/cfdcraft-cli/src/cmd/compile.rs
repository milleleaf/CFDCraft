use super::{Error, emit, with_study};
use cfdcraft_ops::compile_study;
use std::path::Path;

pub fn execute(study: &Path, out: &Path, json: bool) -> Result<i32, Error> {
    let report = with_study(study, "compile", |text| compile_study(text, out));
    Ok(emit(&report, json, "compiled case", out))
}
