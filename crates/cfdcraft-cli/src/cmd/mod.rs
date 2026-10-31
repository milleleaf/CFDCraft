pub mod compile;
pub mod run;
pub mod validate;
pub type Error = Box<dyn std::error::Error>;

pub fn read_study(path: &std::path::Path) -> Result<cfdcraft_model::ResolvedStudy, Error> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read study {}: {e}", path.display()))?;
    let study = cfdcraft_model::Study::from_json(&raw)?;
    Ok(cfdcraft_model::resolve(&study)?)
}
