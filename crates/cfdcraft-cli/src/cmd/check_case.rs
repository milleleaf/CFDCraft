use super::{Error, emit};
use cfdcraft_ops::check_case;
use std::path::Path;

pub fn execute(dir: &Path, json: bool) -> Result<i32, Error> {
    Ok(emit(&check_case(dir), json, "checked case", dir))
}
