use cfdcraft_backend_esi::{Dialect, compile, export::export_tree, write_to_disk};
use cfdcraft_model::{Study, canonical_hash, resolve};
use std::path::Path;

/// `regenerate_golden -- cavity|channel` rewrites that case's golden tree; `export` rewrites only the
/// `Allrun` and `README.md` of the cavity export tree. A diff in the cavity
/// tree is a failing review, never a regeneration.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args().nth(1);
    if target.as_deref() == Some("export") {
        return write_export();
    }
    let (json, dir) = match target.as_deref() {
        Some("cavity") => (
            include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json"),
            "cavity-v2512",
        ),
        Some("channel") => (
            include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json"),
            "channel-v2512",
        ),
        _ => return Err("usage: regenerate_golden cavity|channel|export".into()),
    };
    let tree = compile(&resolve(&Study::from_json(json)?)?, Dialect::EsiV2512)?;
    write_to_disk(
        &tree,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(dir),
    )?;
    Ok(())
}

fn write_export() -> Result<(), Box<dyn std::error::Error>> {
    let study = resolve(&Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v0.json"
    ))?)?;
    let tree = compile(&study, Dialect::EsiV2512)?;
    let exported = export_tree(&tree, &canonical_hash(&study));
    let dest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cavity-v2512-export");
    std::fs::create_dir_all(&dest)?;
    for file in &exported.files[tree.files.len()..] {
        std::fs::write(dest.join(&file.rel_path), &file.bytes)?;
    }
    Ok(())
}
