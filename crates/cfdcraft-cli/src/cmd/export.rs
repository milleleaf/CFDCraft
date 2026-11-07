use super::{Error, emit, with_study};
use cfdcraft_ops::export_study;
use std::path::Path;

pub fn execute(study: &Path, out: &Path, json: bool) -> Result<i32, Error> {
    let report = with_study(study, "export", |text| export_study(text, out));
    Ok(emit(&report, json, "exported case", out))
}
