use super::{Error, read_study};
use cfdcraft_backend_esi::{Dialect, compile, write_to_disk};
use std::path::Path;
pub fn execute(study: &Path, out: &Path) -> Result<i32, Error> {
    let r = read_study(study)?;
    let tree = compile(&r, Dialect::EsiV2512)?;
    write_to_disk(&tree, out)?;
    println!("study hash: {}", cfdcraft_model::canonical_hash(&r));
    println!("compiled case: {}", out.display());
    Ok(0)
}
