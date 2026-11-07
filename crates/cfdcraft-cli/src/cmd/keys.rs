use super::{Error, with_study};
use cfdcraft_ops::{ExitClass, stage_keys};
use std::path::Path;

pub fn execute(study: &Path, json: bool) -> Result<i32, Error> {
    let report = with_study(study, "keys", stage_keys);
    if json {
        println!("{}", report.to_json());
    } else {
        if report.class() == ExitClass::Passed {
            if let Some(hash) = &report.study_hash {
                println!("study hash: {hash}");
            }
            for (kind, label) in [("mesh_key", "mesh key"), ("result_key", "result key")] {
                if let Some(artifact) = report.artifacts.iter().find(|a| a.kind == kind) {
                    println!("{label}: {}", artifact.name);
                }
            }
        }
        eprint!("{}", report.render_text());
    }
    Ok(i32::from(report.exit_class))
}
