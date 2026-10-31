use cfdcraft_backend_esi::{Dialect, compile, write_to_disk};
use cfdcraft_model::{Study, resolve};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let study = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v0.json"
    ))?;
    let tree = compile(&resolve(&study)?, Dialect::EsiV2512)?;
    write_to_disk(
        &tree,
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/cavity-v2512")
            .as_path(),
    )?;
    Ok(())
}
