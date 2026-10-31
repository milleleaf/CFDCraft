use super::{Error, read_study, validate};
use cfdcraft_backend_esi::evidence::{EvidenceExpectations, RunEvidence, Verdict, judge_run};
use cfdcraft_backend_esi::{Dialect, compile, write_to_disk};
use cfdcraft_executor::{HostProfile, run::run_id, run_case};
use std::path::Path;

pub fn execute(
    study: &Path,
    key: &Path,
    hosts: &Path,
    profile: &str,
    with_oracle: bool,
    tolerance: Option<&Path>,
) -> Result<i32, Error> {
    let r = read_study(study)?;
    let oracle = if with_oracle {
        let re = r.lid_velocity * r.domain.length_y / r.nu;
        if (re - 100.0).abs() > 1e-8 || (r.domain.length_x / r.domain.length_y - 1.0).abs() > 1e-10
        {
            return Err(format!(
                "Ghia Re=100 gate requires a square cavity at Re=100; found Re={re}"
            )
            .into());
        }
        let t = cfdcraft_bench_ghia::GhiaTable::load()?;
        Some(if let Some(path) = tolerance {
            t.with_tolerance_file(path)?
        } else {
            t
        })
    } else {
        None
    };
    let p = HostProfile::load(hosts, profile)?;
    let tree = compile(&r, Dialect::EsiV2512)?;
    let work = std::env::current_dir()?
        .join("target/cfdcraft-runs")
        .join(run_id("cavity")?);
    let case = work.join("case");
    let results = work.join("results");
    write_to_disk(&tree, &case)?;
    println!("study hash: {}", cfdcraft_model::canonical_hash(&r));
    let o = run_case(&p, key, &case, "cavity", &results)?;
    println!(
        "remote work: {}\nlocal results: {}\ncontainer: {}\npreflight: {}\nrun duration: {:.3}s\nprocess exit: {:?}",
        o.remote_dir,
        o.results_dir.display(),
        o.container_name,
        o.preflight_uptime.trim(),
        o.duration_seconds,
        o.exit_code
    );
    let block = std::fs::read_to_string(results.join("log.blockMesh")).unwrap_or_default();
    let mesh = std::fs::read_to_string(results.join("log.checkMesh")).unwrap_or_default();
    let solver = std::fs::read_to_string(results.join("log.simpleFoam")).unwrap_or_default();
    let times: Vec<f64> = std::fs::read_dir(&results)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_string_lossy().parse::<f64>().ok())
        .collect();
    let cells = r
        .mesh
        .cells_x
        .checked_mul(r.mesh.cells_y)
        .and_then(|x| x.checked_mul(r.mesh.cells_z))
        .ok_or("cell count overflow")?;
    let verdict = judge_run(
        &RunEvidence {
            block_mesh_log: &block,
            check_mesh_log: &mesh,
            solver_log: &solver,
            time_directories: &times,
        },
        &EvidenceExpectations {
            end_time: r.solver.end_time,
            cell_count: cells,
            domain_volume: r.domain.length_x * r.domain.length_y * r.domain.length_z,
            p_threshold: r.solver.convergence.p,
            u_threshold: r.solver.convergence.u,
        },
    );
    let final_fields = times
        .iter()
        .find(|&&t| (t - r.solver.end_time).abs() < 1e-6)
        .and_then(|t| {
            std::fs::read_dir(&results)
                .ok()?
                .filter_map(Result::ok)
                .find(|e| e.file_name().to_string_lossy().parse::<f64>().ok() == Some(*t))
        })
        .is_some_and(|dir| {
            ["U", "p"].iter().all(|name| {
                std::fs::metadata(dir.path().join(name)).is_ok_and(|m| m.is_file() && m.len() > 0)
            })
        });
    if !final_fields {
        println!("evidence verdict: FAILED\n  missing or empty final-time U/p field files");
        return Ok(1);
    }
    match verdict {
        Verdict::Failed(reasons) => {
            println!("evidence verdict: FAILED");
            for reason in reasons {
                println!("  {reason}");
            }
            return Ok(1);
        }
        Verdict::Passed => println!("evidence verdict: PASSED"),
    }
    if let Some(table) = oracle {
        return match validate::compare_results(&results, &r, &table) {
            Ok(code) => Ok(code),
            Err(error) => {
                println!("Ghia verdict: FAILED\n  invalid or incomplete sampled output: {error}");
                Ok(1)
            }
        };
    }
    Ok(0)
}
