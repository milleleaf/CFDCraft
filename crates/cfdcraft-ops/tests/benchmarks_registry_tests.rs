//! The benchmark registry: the files a comparison reads, the lid-driven cavity as a benchmark,
//! the choice of a benchmark by what the study is, and the validate run on top of it.

use cfdcraft_backend_esi::Dialect;
use cfdcraft_bench_ghia::{GhiaTable, linear_sample, render_table};
use cfdcraft_gate::{Benchmark, Mismatch, ResultFiles};
use cfdcraft_model::{
    BoundaryKind, InitMode, ResolvedPhysics, ResolvedStudy, ResolvedTime, ResolvedTurbulence,
    Severity, TurbulenceModel,
};
use cfdcraft_ops::{
    DirFiles, ExitClass, Finished, RunMode, RunRequest, begin,
    benchmarks::{GhiaBenchmark, not_a_benchmark},
    build,
    ghia::{compare_results, load_table},
    ids, prepare, quantities_for, select,
};
use cfdcraft_text::{Line, Text};
use serde_json::Value;
use std::path::{Path, PathBuf};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ops-benchmarks-tests")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Sampled centrelines of a 0.1 m cavity at a 0.1 m/s lid, written the way the solver writes
/// them, with every velocity moved by `shift` lid velocities from the reference.
fn write_samples(dir: &Path, shift: f64) {
    let table = GhiaTable::load().unwrap();
    let out = dir.join("postProcessing/centreline/5000");
    std::fs::create_dir_all(&out).unwrap();
    for (file, profile, component) in [
        ("vertical_U.xy", &table.u, 1),
        ("horizontal_U.xy", &table.v, 2),
    ] {
        let mut rows: Vec<_> = profile.coords.iter().zip(&profile.values).collect();
        rows.sort_by(|a, b| a.0.total_cmp(b.0));
        let text: String = rows
            .iter()
            .map(|(c, v)| {
                let mut cols = [0.0; 3];
                cols[component - 1] = (**v + shift) * 0.1;
                format!(
                    "{:.12} {:.12} {:.12} {:.12}\n",
                    **c * 0.1,
                    cols[0],
                    cols[1],
                    cols[2]
                )
            })
            .collect();
        std::fs::write(out.join(file), text).unwrap();
    }
}

fn edited(change: impl FnOnce(&mut Value)) -> String {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    change(&mut value);
    value.to_string()
}

fn resolved(text: &str) -> ResolvedStudy {
    prepare(text, Dialect::EsiV2512).unwrap().resolved
}

fn mismatch(what: &str, found: &str, expected: &str) -> Mismatch {
    Mismatch {
        what: what.to_owned(),
        found: found.to_owned(),
        expected: expected.to_owned(),
    }
}

fn whats(found: &[Mismatch]) -> Vec<&str> {
    found.iter().map(|m| m.what.as_str()).collect()
}

#[cfg(unix)]
fn links(dir: &Path, files: &DirFiles) {
    use std::os::unix::fs::symlink;
    symlink(dir.join("sub/a.txt"), dir.join("sub/link.txt")).unwrap();
    symlink(dir.join("sub"), dir.join("dirlink")).unwrap();
    assert_eq!(files.read("sub/link.txt"), None);
    assert!(!files.list("sub").contains(&"link.txt".to_owned()));
    assert_eq!(files.read("dirlink/a.txt"), None);
    assert!(files.list("dirlink").is_empty());
}

#[cfg(not(unix))]
fn links(_dir: &Path, _files: &DirFiles) {}

#[test]
fn a_folder_is_a_result_files() {
    let dir = scratch("files");
    std::fs::create_dir_all(dir.join("sub/inner")).unwrap();
    std::fs::write(dir.join("sub/a.txt"), "hello").unwrap();
    std::fs::write(dir.join("sub/b.txt"), [0xff, b'a']).unwrap();
    let big = std::fs::File::create(dir.join("sub/big.bin")).unwrap();
    big.set_len(64 * 1024 * 1024 + 1).unwrap();
    let files = DirFiles { root: dir.clone() };

    assert_eq!(files.read("sub/a.txt").as_deref(), Some("hello"));
    assert_eq!(files.read("sub/b.txt").as_deref(), Some("\u{fffd}a"));
    assert_eq!(files.read("sub/missing.txt"), None);
    assert_eq!(files.read("sub"), None, "a folder is not a file");
    assert_eq!(files.read("sub/big.bin"), None, "over the size limit");
    assert_eq!(files.read("../sub/a.txt"), None);
    assert_eq!(files.read("sub/../sub/a.txt"), None);
    let absolute = dir.join("sub/a.txt").to_string_lossy().into_owned();
    assert_eq!(files.read(&absolute), None);

    assert_eq!(
        files.list("sub"),
        ["a.txt", "b.txt", "big.bin", "inner"].map(str::to_owned)
    );
    assert!(files.list("nothing").is_empty());
    assert!(files.list("sub/a.txt").is_empty(), "a file has no entries");
    assert!(files.list("..").is_empty());
    links(&dir, &files);
}

#[test]
fn the_cavity_matches_and_a_changed_one_says_what_differs() {
    let ghia = GhiaBenchmark::new(None).unwrap();
    let cavity = resolved(CAVITY);
    assert_eq!(ghia.id(), "ghia");
    assert!(ghia.matches(&cavity).is_ok());

    let slow = resolved(&edited(|v| {
        v["fluid"]["kinematic_viscosity"]["value"] = 0.0002.into();
    }));
    assert_eq!(
        ghia.matches(&slow).unwrap_err(),
        [mismatch("reynolds number", "50", "100")]
    );

    let oblong = resolved(&edited(|v| v["domain"]["length_x"]["value"] = 0.12.into()));
    let found = ghia.matches(&oblong).unwrap_err();
    assert_eq!(whats(&found), ["shape"]);
    assert_eq!(found[0].expected, "a square");

    let mut two_lids = cavity.clone();
    let walls = two_lids.boundary_conditions.get_mut("walls").unwrap();
    walls.kind = BoundaryKind::MovingWall;
    walls.velocity = Some(0.1);
    assert_eq!(whats(&ghia.matches(&two_lids).unwrap_err()), ["lid"]);

    let channel = resolved(CHANNEL);
    assert!(whats(&ghia.matches(&channel).unwrap_err()).contains(&"lid"));

    let laminar = ResolvedTurbulence {
        model: TurbulenceModel::Laminar,
        intensity: None,
        viscosity_ratio: None,
        reference_velocity: None,
    };
    let mut changed = cavity.clone();
    changed.physics = Some(ResolvedPhysics {
        time: ResolvedTime::Transient { delta_t: 0.5 },
        turbulence: laminar.clone(),
        buoyancy: None,
        init: InitMode::Uniform,
    });
    assert_eq!(
        ghia.matches(&changed).unwrap_err(),
        [mismatch("time", "transient", "steady")]
    );
    changed.physics = Some(ResolvedPhysics {
        time: ResolvedTime::Steady,
        turbulence: ResolvedTurbulence {
            model: TurbulenceModel::KOmegaSst,
            ..laminar
        },
        buoyancy: None,
        init: InitMode::Uniform,
    });
    assert_eq!(
        whats(&ghia.matches(&changed).unwrap_err()),
        ["turbulence model"]
    );
}

#[test]
fn the_trait_comparison_equals_the_old_function() {
    let cavity = resolved(CAVITY);
    let table = load_table(None).unwrap();
    let ghia = GhiaBenchmark::new(None).unwrap();
    for (tag, shift, passes) in [("good", 0.0, true), ("off", 0.5, false)] {
        let dir = scratch(tag);
        write_samples(&dir, shift);
        let old = compare_results(&dir, &cavity, &table);
        let new = ghia.compare(&DirFiles { root: dir }, &cavity);
        assert_eq!(new.passed, passes, "{tag}");
        assert_eq!(
            serde_json::to_string(&new).unwrap(),
            serde_json::to_string(&old).unwrap(),
            "{tag}"
        );
        assert_eq!(new.render_text(), old.render_text(), "{tag}");
    }
    let empty = scratch("nothing");
    let new = ghia.compare(
        &DirFiles {
            root: empty.clone(),
        },
        &cavity,
    );
    let old = compare_results(&empty, &cavity, &table);
    assert!(!new.passed && new.reason.is_some());
    assert_eq!(new, old);
}

#[test]
fn the_quantity_is_the_centre_velocity() {
    let cavity = resolved(CAVITY);
    let ghia = GhiaBenchmark::new(None).unwrap();
    let dir = scratch("quantity");
    write_samples(&dir, 0.0);
    let files = DirFiles { root: dir };
    let found = ghia.quantities(&files, &cavity);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "bench.ghia.u_centre");
    assert!((found[0].1 + 0.20581).abs() < 1e-9, "{}", found[0].1);
    let table = GhiaTable::load().unwrap();
    assert!((found[0].1 - linear_sample(&table.u, 0.5).unwrap()).abs() < 1e-9);
    assert_eq!(quantities_for(&cavity, &files), found);

    let bare = DirFiles {
        root: scratch("no-centreline"),
    };
    assert!(ghia.quantities(&bare, &cavity).is_empty());
    assert!(quantities_for(&cavity, &bare).is_empty());
    let channel = resolved(CHANNEL);
    assert!(quantities_for(&channel, &files).is_empty());
}

#[test]
fn the_registry_selects_by_what_the_study_is() {
    assert_eq!(
        ids().to_vec(),
        ["ghia", "step", "armaly", "cylinder", "dvd"]
    );
    let chosen = select(&resolved(CAVITY), None).ok().unwrap();
    assert_eq!(chosen.id(), "ghia");

    let slow = resolved(&edited(|v| {
        v["fluid"]["kinematic_viscosity"]["value"] = 0.0002.into();
    }));
    let rejected = select(&slow, None).err().unwrap();
    // Every benchmark of the registry is tried, in order, and says what differs.
    let tried: Vec<&str> = rejected.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(tried, ["ghia", "step", "armaly", "cylinder", "dvd"]);
    assert!(rejected.iter().all(|(_, found)| !found.is_empty()));
    assert_eq!(rejected[0].0, "ghia");
    assert_eq!(rejected[0].1, [mismatch("reynolds number", "50", "100")]);

    assert!(build("nope", None).is_err());
    assert_eq!(build("ghia", None).unwrap().id(), "ghia");
}

#[test]
fn a_study_checked_against_the_wrong_benchmark_gets_the_code() {
    let found = [
        mismatch("reynolds number", "50", "100"),
        mismatch("shape", "0.12 m by 0.1 m", "a square"),
    ];
    let diagnostic = not_a_benchmark("ghia", &found);
    assert_eq!(diagnostic.code, "CFD-BENCH-001");
    assert_eq!(diagnostic.severity, Severity::Error);
    let expected = Text::english().render(&Line::BenchmarkNotMatching {
        benchmark: "ghia".to_owned(),
        differences:
            "reynolds number is 50, expected 100; shape is 0.12 m by 0.1 m, expected a square"
                .to_owned(),
    });
    assert_eq!(diagnostic.message, expected);
}

/// A validate request whose host files do not exist: a run that gets past the checks of the
/// study ends as a failure of the host, not a refusal of the study.
fn validate(study: &str, tolerance: Option<PathBuf>) -> Finished {
    let mut request = RunRequest::new(
        RunMode::Validate,
        "/nonexistent/key",
        "/nonexistent/hosts.toml",
        "box",
        "/nonexistent/runs",
        "/nonexistent/sockets",
    );
    request.tolerance_override = tolerance;
    match begin(study, &request) {
        Ok(_) => panic!("a run cannot begin without a host"),
        Err(done) => done,
    }
}

fn diagnostic_of<'a>(done: &'a Finished, code: &str) -> &'a cfdcraft_model::Diagnostic {
    done.report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} in {:?}", done.report.diagnostics))
}

#[test]
fn validate_refuses_a_non_cavity_with_the_old_words() {
    const HINT: &str = "Use a study of the square lid-driven cavity at Reynolds number 100.";
    const START: &str = "The reference comparison is defined for a square cavity at Reynolds number 100; this study has Reynolds number ";

    let slow = validate(
        &edited(|v| v["fluid"]["kinematic_viscosity"]["value"] = 0.0002.into()),
        None,
    );
    assert_eq!(slow.report.class(), ExitClass::StudyInvalid);
    let found = diagnostic_of(&slow, "CFD-RUN-002");
    assert!(
        found.message.starts_with(&format!("{START}50")),
        "{}",
        found.message
    );
    assert!(found.message.ends_with('.') && !found.message.contains("not square"));
    assert_eq!(found.hint.as_deref(), Some(HINT));

    let oblong = validate(
        &edited(|v| v["domain"]["length_x"]["value"] = 0.12.into()),
        None,
    );
    assert_eq!(oblong.report.class(), ExitClass::StudyInvalid);
    let found = diagnostic_of(&oblong, "CFD-RUN-002");
    assert!(
        found.message.starts_with(&format!("{START}100")),
        "{}",
        found.message
    );
    assert!(found.message.ends_with(" and is not square."));
    assert_eq!(found.hint.as_deref(), Some(HINT));

    let channel = validate(CHANNEL, None);
    assert_eq!(channel.report.class(), ExitClass::StudyInvalid);
    let found = diagnostic_of(&channel, "CFD-RUN-002");
    assert_eq!(
        found.message,
        "The reference comparison needs a lid-driven cavity: exactly one moving wall on the top face."
    );
    assert_eq!(found.hint.as_deref(), Some(HINT));
}

#[test]
fn validate_still_loads_the_override_for_ghia() {
    let dir = scratch("override");
    let bad = dir.join("bad.toml");
    std::fs::write(&bad, "this is [not toml").unwrap();
    let refused = validate(CAVITY, Some(bad));
    assert_eq!(refused.report.class(), ExitClass::StudyInvalid);
    let found = diagnostic_of(&refused, "CFD-CLI-001");
    assert!(
        found
            .message
            .starts_with("The tolerance file cannot be used:"),
        "{}",
        found.message
    );

    let good = dir.join("good.toml");
    std::fs::write(
        &good,
        "[tolerance]\nu_max_abs = 0.01\nv_max_abs = 0.01\nunits = \"lid_velocities\"\njustification = \"a tighter limit for a test\"\n",
    )
    .unwrap();
    let past = validate(CAVITY, Some(good.clone()));
    assert_eq!(past.report.class(), ExitClass::Infrastructure);
    assert!(
        past.report
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("tolerance file"))
    );

    // The file's tolerances are the ones the benchmark compares with.
    let cavity = resolved(CAVITY);
    let samples = scratch("override-samples");
    write_samples(&samples, 0.015);
    let files = DirFiles { root: samples };
    assert!(
        GhiaBenchmark::new(None)
            .unwrap()
            .compare(&files, &cavity)
            .passed
    );
    let tighter = GhiaBenchmark::new(Some(&good)).unwrap();
    assert!(!tighter.compare(&files, &cavity).passed);
}

#[test]
fn the_gate_report_is_unchanged_for_the_cavity() {
    let cavity = resolved(CAVITY);
    let dir = scratch("unchanged");
    write_samples(&dir, 0.004);
    let chosen = select(&cavity, None).ok().unwrap();
    let gate = chosen.compare(&DirFiles { root: dir.clone() }, &cavity);
    let table = load_table(None).unwrap();
    let direct = compare_results(&dir, &cavity, &table);
    assert_eq!(gate, direct);
    assert!(gate.passed);
    let sample = |file: &str, length: f64, component: usize| {
        let raw =
            std::fs::read_to_string(dir.join("postProcessing/centreline/5000").join(file)).unwrap();
        cfdcraft_ops::ghia::profile_from_raw(&raw, length, 0.1, component).unwrap()
    };
    let u = table.compare_u(&sample("vertical_U.xy", 0.1, 1)).unwrap();
    let v = table.compare_v(&sample("horizontal_U.xy", 0.1, 2)).unwrap();
    assert_eq!(gate.render_text(), render_table(&u, &v));
}
