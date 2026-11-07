//! The five benchmarks in the registry: each template's study selects its own benchmark, a
//! modified study selects none and says why, the numbers each benchmark records come out of its
//! own result tree, and a tolerance file reaches the benchmark it is meant for.

use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_bench_dvd::fixture::{Tweak, good_files as dvd_files};
use cfdcraft_bench_ghia::GhiaTable;
use cfdcraft_bench_step::fixture::good_files as step_files;
use cfdcraft_capture::Bundle;
use cfdcraft_gate::Benchmark;
use cfdcraft_model::{ResolvedStudy, Study, TurbulenceModel, resolve};
use cfdcraft_ops::{BundleFiles, build, ids, quantities_for, select};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build as template};
use serde_json::Value;
use std::path::{Path, PathBuf};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// Each template and the benchmark that judges its runs.
const TEMPLATES: [(&str, &str); 5] = [
    ("lid-cavity", "ghia"),
    ("backward-step", "step"),
    ("backward-step-laminar", "armaly"),
    ("cylinder", "cylinder"),
    ("heated-cavity", "dvd"),
];

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ops-benchmarks-all-tests")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn resolved_template(id: &str, options: &TemplateOptions) -> ResolvedStudy {
    let study = template(id, options).unwrap_or_else(|e| panic!("{id}: {e}"));
    resolve(&study).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn id_of(benchmark: &dyn Benchmark) -> &'static str {
    benchmark.id()
}

fn files_of(entries: Vec<(String, String)>) -> BundleFiles {
    let entries = entries
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    BundleFiles::new(Bundle::from_entries(entries).unwrap().to_bytes().unwrap())
}

fn names(found: &[(String, f64)]) -> Vec<&str> {
    found.iter().map(|(name, _)| name.as_str()).collect()
}

#[test]
fn the_registry_holds_the_five_benchmarks_in_order() {
    assert_eq!(
        ids().to_vec(),
        vec!["ghia", "step", "armaly", "cylinder", "dvd"]
    );
    for id in ids() {
        let benchmark = build(id, None).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(id_of(benchmark.as_ref()), *id);
    }
    assert!(
        build("nope", None)
            .err()
            .is_some_and(|e| e.contains("not a known benchmark"))
    );
}

#[test]
fn each_template_selects_its_benchmark() {
    for (template_id, benchmark) in TEMPLATES {
        for mesh in MeshPreset::ALL {
            let options = TemplateOptions {
                mesh,
                ..TemplateOptions::default()
            };
            let r = resolved_template(template_id, &options);
            let chosen = select(&r, None)
                .unwrap_or_else(|rejected| panic!("{template_id} at {mesh:?}: {rejected:?}"));
            assert_eq!(
                id_of(chosen.as_ref()),
                benchmark,
                "{template_id} at {mesh:?}"
            );
        }
    }
}

#[test]
fn the_turbulence_model_decides_only_where_the_benchmark_says() {
    let models = [
        TurbulenceModel::Laminar,
        TurbulenceModel::KOmegaSst,
        TurbulenceModel::RealizableKe,
        TurbulenceModel::RngKEpsilon,
    ];
    for model in models {
        let options = TemplateOptions {
            turbulence: Some(model),
            ..TemplateOptions::default()
        };
        // The turbulent step takes any model.
        let step = resolved_template("backward-step", &options);
        let chosen = select(&step, None).map(|b| id_of(b.as_ref())).ok();
        assert_eq!(chosen, Some("step"), "{model:?}");
        // The two laminar benchmarks take the laminar model only.
        for (id, benchmark) in [("lid-cavity", "ghia"), ("backward-step-laminar", "armaly")] {
            let r = resolved_template(id, &options);
            let chosen = select(&r, None).map(|b| id_of(b.as_ref())).ok();
            let expected = (model == TurbulenceModel::Laminar).then_some(benchmark);
            assert_eq!(chosen, expected, "{id} with {model:?}");
        }
    }
}

/// The template with a different viscosity.
fn scaled_viscosity(id: &str, factor: f64) -> ResolvedStudy {
    let study = template(id, &TemplateOptions::default()).unwrap();
    let mut value = serde_json::to_value(&study).unwrap();
    let nu = &mut value["fluid"]["kinematic_viscosity"]["value"];
    *nu = Value::from(nu.as_f64().unwrap() * factor);
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

#[test]
fn a_modified_study_selects_none_and_says_why() {
    let changes = [
        ("lid-cavity", "ghia", 2.0),
        ("backward-step", "step", 2.0),
        ("backward-step-laminar", "armaly", 10.0),
        ("cylinder", "cylinder", 2.0),
        ("heated-cavity", "dvd", 2.0),
    ];
    for (id, benchmark, factor) in changes {
        let r = scaled_viscosity(id, factor);
        let Err(rejected) = select(&r, None) else {
            panic!("{id} with another viscosity still selects a benchmark");
        };
        let tried: Vec<&str> = rejected.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(tried, ids().to_vec(), "{id}");
        for (name, found) in &rejected {
            assert!(!found.is_empty(), "{id}: {name} lists no difference");
        }
        let (_, own) = rejected.iter().find(|(name, _)| name == benchmark).unwrap();
        let words = ["reynolds", "viscosity", "rayleigh"];
        assert!(
            own.iter().any(|m| {
                let what = m.what.to_lowercase();
                words.iter().any(|word| what.contains(word))
            }),
            "{id}: {own:?}"
        );
        assert!(quantities_for(&r, &BundleFiles::new(Vec::new())).is_empty());
    }
}

/// The sampled centrelines of the cavity of `cavity_v1.json` at its lid speed, on the reference.
fn ghia_entries(r: &ResolvedStudy) -> Vec<(String, String)> {
    let table = GhiaTable::load().unwrap();
    let mut out = Vec::new();
    for (file, profile, component) in [
        ("vertical_U.xy", &table.u, 1_usize),
        ("horizontal_U.xy", &table.v, 2),
    ] {
        let mut rows: Vec<_> = profile.coords.iter().zip(&profile.values).collect();
        rows.sort_by(|a, b| a.0.total_cmp(b.0));
        let text: String = rows
            .iter()
            .map(|(c, v)| {
                let mut cols = [0.0; 3];
                cols[component - 1] = **v * 0.1;
                format!(
                    "{:.12} {:.12} {:.12} {:.12}\n",
                    **c * 0.1,
                    cols[0],
                    cols[1],
                    cols[2]
                )
            })
            .collect();
        let folder = r.solver.end_time;
        out.push((format!("postProcessing/centreline/{folder}/{file}"), text));
    }
    out
}

#[test]
fn quantities_for_a_good_result_tree() {
    let defaults = TemplateOptions::default();
    let finite = |found: &[(String, f64)]| {
        for (name, value) in found {
            assert!(value.is_finite(), "{name} is {value}");
        }
    };

    let cavity = resolve(&Study::from_json(CAVITY).unwrap()).unwrap();
    let found = quantities_for(&cavity, &files_of(ghia_entries(&cavity)));
    assert_eq!(names(&found), ["bench.ghia.u_centre"]);
    finite(&found);

    let step = resolved_template("backward-step", &defaults);
    let found = quantities_for(&step, &files_of(step_files(&step).unwrap()));
    assert_eq!(names(&found), ["bench.step.reattachment"]);
    finite(&found);
    assert!((found[0].1 - 6.26).abs() < 1e-3, "{found:?}");

    let armaly = resolved_template("backward-step-laminar", &defaults);
    let found = quantities_for(&armaly, &files_of(step_files(&armaly).unwrap()));
    assert_eq!(names(&found), ["bench.armaly.reattachment"]);
    finite(&found);
    assert!(found[0].1 > 0.0, "{found:?}");

    let cylinder = resolved_template("cylinder", &defaults);
    let found = quantities_for(&cylinder, &files_of(cylinder_tree(&cylinder, &Run::good())));
    assert_eq!(
        names(&found),
        [
            "bench.cylinder.st",
            "bench.cylinder.cd_max",
            "bench.cylinder.cl_max",
            "bench.cylinder.delta_p",
            "bench.cylinder.cycles"
        ]
    );
    finite(&found);

    let heated = resolved_template("heated-cavity", &defaults);
    let found = quantities_for(
        &heated,
        &files_of(dvd_files(&heated, &Tweak::default()).unwrap()),
    );
    assert_eq!(
        names(&found),
        [
            "bench.dvd.nu_hot",
            "bench.dvd.u_max",
            "bench.dvd.w_max",
            "bench.dvd.u_max_y",
            "bench.dvd.w_max_x",
            "bench.dvd.energy_imbalance"
        ]
    );
    finite(&found);
}

fn file(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn refusal(id: &str, path: &Path) -> String {
    build(id, Some(path))
        .err()
        .unwrap_or_else(|| panic!("{id} accepted {}", path.display()))
}

#[test]
fn a_tolerance_override_reaches_the_right_benchmark() {
    let dir = scratch("override");

    let wider = file(
        &dir,
        "cylinder.toml",
        "[tolerance]\ncd_max = 0.04\njustification = \"the mesh is coarse\"\n",
    );
    assert!(build("cylinder", Some(&wider)).is_ok());
    let too_wide = file(
        &dir,
        "cylinder-wide.toml",
        "[tolerance]\ncd_max = 0.07\njustification = \"the mesh is coarse\"\n",
    );
    assert!(refusal("cylinder", &too_wide).contains("cd_max"));
    let unjustified = file(&dir, "cylinder-bare.toml", "[tolerance]\ncd_max = 0.03\n");
    assert!(refusal("cylinder", &unjustified).contains("justification"));

    let step = file(
        &dir,
        "step.toml",
        "[tolerance]\nx_over_h = 0.8\njustification = \"a coarse mesh\"\n",
    );
    assert!(build("step", Some(&step)).is_ok());
    let step_wide = file(
        &dir,
        "step-wide.toml",
        "[tolerance]\nx_over_h = 2.0\njustification = \"a coarse mesh\"\n",
    );
    assert!(refusal("step", &step_wide).contains("x_over_h"));

    let dvd = file(
        &dir,
        "dvd.toml",
        "[tolerance]\nnu_rel = 0.03\njustification = \"a coarse mesh\"\n",
    );
    assert!(build("dvd", Some(&dvd)).is_ok());

    // The laminar step has no tolerance a file can change.
    assert!(refusal("armaly", &step).contains("armaly"));

    // The cavity keeps its own loader and its own file layout.
    let ghia = file(
        &dir,
        "ghia.toml",
        "[tolerance]\nu_max_abs = 0.03\nv_max_abs = 0.03\nunits = \"lid_velocities\"\njustification = \"a coarse mesh\"\n",
    );
    assert!(build("ghia", Some(&ghia)).is_ok());
    assert!(build("ghia", Some(&dir.join("missing.toml"))).is_err());

    // A file that is too large, missing or not text is refused, and named as typed.
    let big = file(&dir, "big.toml", &"#".repeat(64 * 1024 + 1));
    let message = refusal("cylinder", &big);
    assert!(message.contains(&big.display().to_string()), "{message}");
    assert!(message.contains("64 KiB"), "{message}");
    let missing = dir.join("absent.toml");
    let message = refusal("step", &missing);
    assert!(
        message.contains(&missing.display().to_string()),
        "{message}"
    );
    let binary = dir.join("binary.toml");
    std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    assert!(refusal("dvd", &binary).contains("not text"));
}
