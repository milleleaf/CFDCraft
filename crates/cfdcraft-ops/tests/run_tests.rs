//! A study run end to end against a fake ssh and docker: the classified report, busy versus failed,
//! and what a report must never contain.
#![cfg(unix)]

mod support;

use cfdcraft_ops::{ExitClass, Report, RunMode, run_study};
use serde_json::Value;
use std::path::{Path, PathBuf};
use support::*;

const GROUP: &str = "ops-run-tests";

/// Child side. Does nothing unless the parent set the root. Runs the named study and writes the
/// report and the executor's detail text to `out.json`.
#[test]
fn run_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let study = std::fs::read_to_string(std::env::var("CFC_STUDY").unwrap()).unwrap();
    let mode = match std::env::var("CFC_RUN_MODE").unwrap().as_str() {
        "validate" => RunMode::Validate,
        "smoke" => RunMode::Smoke,
        _ => RunMode::Run,
    };
    let mut request = request(&root, mode);
    if let Ok(p) = std::env::var("CFC_HOSTS") {
        request.hosts = p.into();
    }
    if let Ok(p) = std::env::var("CFC_KEY") {
        request.key = p.into();
    }
    if let Ok(p) = std::env::var("CFC_PROFILE") {
        request.profile = p;
    }
    if let Ok(p) = std::env::var("CFC_CASE_DIR") {
        request.case_dir = Some(p.into());
    }
    if let Ok(p) = std::env::var("CFC_TOLERANCE") {
        request.tolerance_override = Some(p.into());
    }
    let done = run_study(&study, &request);
    let out = serde_json::json!({ "report": done.report, "detail": done.detail });
    std::fs::write(root.join("out.json"), out.to_string()).unwrap();
}

pub struct Outcome {
    pub root: PathBuf,
    pub report: Report,
    pub detail: Option<String>,
    pub raw: String,
}

/// Run one scenario in a worker and read back what it wrote.
pub fn scenario(
    tag: &str,
    docker_mode: &str,
    run_mode: &str,
    study: &str,
    setup: impl FnOnce(&Path),
    env: &[(&str, &str)],
) -> Outcome {
    let root = new_root(GROUP, tag);
    setup(&root);
    scenario_in(root, docker_mode, run_mode, study, env)
}

/// `scenario` on a root the caller already prepared.
pub fn scenario_in(
    root: PathBuf,
    docker_mode: &str,
    run_mode: &str,
    study: &str,
    env: &[(&str, &str)],
) -> Outcome {
    std::fs::write(root.join("study.json"), study).unwrap();
    let mut c = worker(&root, "run_worker", docker_mode);
    c.env("CFC_STUDY", root.join("study.json"))
        .env("CFC_RUN_MODE", run_mode);
    for (k, v) in env {
        c.env(k, v);
    }
    assert_worker_ok(&c.output().unwrap());
    let value: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("out.json")).unwrap()).unwrap();
    Outcome {
        report: serde_json::from_value(value["report"].clone()).unwrap(),
        detail: value["detail"].as_str().map(str::to_owned),
        raw: value["report"].to_string(),
        root,
    }
}

fn no_leaks(o: &Outcome) {
    let work_dir = cfdcraft_executor::HostProfile::load(&o.root.join("hosts.toml"), "box")
        .unwrap()
        .work_dir;
    for secret in [
        o.root.join("dummy-key").to_string_lossy().into_owned(),
        SSH_TARGET.to_owned(),
        work_dir,
    ] {
        assert!(
            !o.raw.contains(&secret),
            "the report names {secret}: {}",
            o.raw
        );
    }
}

#[test]
fn an_empty_result_is_judged_from_evidence_and_not_from_the_exit_status() {
    let o = scenario("evidence", "ok", "run", CAVITY, |_| {}, &[]);
    let r = &o.report;
    assert_eq!(r.class(), ExitClass::GateFailed);
    assert_eq!(r.status, "failed");
    let evidence = r.evidence.as_ref().unwrap();
    assert_eq!(evidence.verdict, "failed");
    assert!(
        evidence.reasons.iter().any(|x| x.contains("blockMesh")),
        "{:?}",
        evidence.reasons
    );
    let run = r.run.as_ref().unwrap();
    assert_eq!(run.process_exit, Some(0), "recorded, not judged");
    let saved = o.root.join("runs").join(&run.run_id).join("report.json");
    let saved: Report = serde_json::from_str(&std::fs::read_to_string(saved).unwrap()).unwrap();
    assert_eq!(&saved, r);
    assert!(!lock_dir(&o.root).exists(), "the lock is released");
    no_leaks(&o);
}

#[test]
fn a_held_lock_is_busy_class_4_and_nothing_is_started() {
    let o = scenario(
        "busy",
        "ok",
        "run",
        CAVITY,
        |root| hold_lock(root, now_epoch(), 3600),
        &[],
    );
    assert_eq!(o.report.class(), ExitClass::Infrastructure);
    let infra = o.report.infra.as_ref().unwrap();
    assert_eq!(infra.code, "CFD-INFRA-001");
    assert_eq!(infra.holder.as_ref().unwrap().run_id, "other-run-1");
    assert!((30..=600).contains(&infra.retry_after_seconds.unwrap()));
    assert!(!infra.stale);
    assert!(
        events(&o.root).iter().all(|e| e != "run"),
        "no container was started: {:?}",
        events(&o.root)
    );
    assert!(
        lock_dir(&o.root).exists(),
        "someone else's lock is left alone"
    );
    no_leaks(&o);
}

#[test]
fn ssh_and_profile_failures_are_class_4_with_stable_codes() {
    let o = scenario(
        "ssh255",
        "ok",
        "run",
        CAVITY,
        |_| {},
        &[("CFC_TEST_FAIL_255", "uptime")],
    );
    assert_eq!(o.report.class(), ExitClass::Infrastructure);
    assert_eq!(o.report.infra.as_ref().unwrap().code, "CFD-INFRA-002");
    assert!(
        o.report
            .infra
            .as_ref()
            .unwrap()
            .retry_after_seconds
            .is_none()
    );
    no_leaks(&o);

    let o = scenario(
        "noprofile",
        "ok",
        "run",
        CAVITY,
        |_| {},
        &[("CFC_PROFILE", "nope")],
    );
    assert_eq!(o.report.class(), ExitClass::Infrastructure);
    assert_eq!(o.report.infra.as_ref().unwrap().code, "CFD-INFRA-005");
    assert!(ssh_log(&o.root).is_empty(), "no host was contacted");
    no_leaks(&o);
}

/// The detail text keeps what a report must not: prove the check above can fail.
#[test]
fn the_operator_detail_names_the_key_and_the_report_does_not() {
    let missing = "/nonexistent-dir/secret-key-file";
    let o = scenario(
        "nokey",
        "ok",
        "run",
        CAVITY,
        |_| {},
        &[("CFC_KEY", missing)],
    );
    assert_eq!(o.report.class(), ExitClass::Infrastructure);
    assert!(o.detail.as_deref().unwrap().contains(missing));
    assert!(!o.raw.contains(missing), "{}", o.raw);
    assert!(!o.report.render_text().contains(missing));
}

#[test]
fn an_invalid_study_starts_nothing() {
    let o = scenario(
        "invalid",
        "ok",
        "run",
        "{\"schema_version\": 1}",
        |_| {},
        &[],
    );
    assert_eq!(o.report.class(), ExitClass::StudyInvalid);
    assert_eq!(o.report.operation, "run");
    assert!(o.report.infra.is_none());
    assert!(ssh_log(&o.root).is_empty(), "{:?}", ssh_log(&o.root));
    assert!(events(&o.root).is_empty());
}

// ---- modes: validate, smoke, and an exported case directory ----

const GOOD_SOLVER: &str = include_str!("../../cfdcraft-backend-esi/tests/fixtures/cavity-good.log");

/// The cavity study with an edit applied to its JSON.
fn edit(study: &str, change: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(study).unwrap();
    change(&mut v);
    v.to_string()
}

/// The cavity run for `end` iterations, judged against the shipped logs.
fn cavity_for(end: u64) -> String {
    edit(CAVITY, |v| {
        v["solver"]["end_time"]["value"] = end.into();
        v["solver"]["write_interval"]["value"] = end.into();
    })
}

/// A results archive a good run would download: the three logs, the final fields and the sampled
/// centrelines. `shift` moves every sampled velocity by that many lid velocities.
fn results_tar(root: &Path, end: u64, shift: f64, with_samples: bool) -> PathBuf {
    let src = root.join("results-src");
    let time = src.join(end.to_string());
    let samples = src.join(format!("postProcessing/centreline/{end}"));
    std::fs::create_dir_all(&time).unwrap();
    std::fs::create_dir_all(&samples).unwrap();
    let w = |p: PathBuf, text: &str| std::fs::write(p, text).unwrap();
    w(src.join("log.blockMesh"), "Created 16384 cells\nEnd\n");
    w(
        src.join("log.checkMesh"),
        "cells: 16384\nTotal volume = 0.0005\nMesh OK.\nEnd\n",
    );
    w(
        src.join("log.simpleFoam"),
        &GOOD_SOLVER.replace("Time = 2000", &format!("Time = {end}")),
    );
    w(time.join("U"), "field\n");
    w(time.join("p"), "field\n");
    if with_samples {
        let table = cfdcraft_bench_ghia::GhiaTable::load().unwrap();
        let (length, lid) = (0.1, 0.1);
        let lines = |profile: &cfdcraft_bench_ghia::Profile, component: usize| {
            let mut rows: Vec<(f64, f64)> = profile
                .coords
                .iter()
                .copied()
                .zip(profile.values.iter().copied())
                .collect();
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows.iter()
                .map(|(c, v)| {
                    let mut cols = [0.0; 3];
                    cols[component - 1] = (v + shift) * lid;
                    format!(
                        "{:.12} {:.12} {:.12} {:.12}\n",
                        c * length,
                        cols[0],
                        cols[1],
                        cols[2]
                    )
                })
                .collect::<String>()
        };
        w(samples.join("vertical_U.xy"), &lines(&table.u, 1));
        w(samples.join("horizontal_U.xy"), &lines(&table.v, 2));
    }
    let tar = root.join("results.tar");
    assert!(
        std::process::Command::new("tar")
            .arg("-C")
            .arg(&src)
            .arg("-cf")
            .arg(&tar)
            .arg(".")
            .status()
            .unwrap()
            .success()
    );
    tar
}

/// Run `study` against a box that serves the archive `results_tar` builds, and return the report.
fn tar_scenario(
    tag: &str,
    run_mode: &str,
    study: &str,
    end: u64,
    shift: f64,
    samples: bool,
) -> Report {
    let root = new_root(GROUP, tag);
    let tar = results_tar(&root, end, shift, samples);
    let tar = tar.to_str().unwrap();
    scenario_in(
        root,
        "ok",
        run_mode,
        study,
        &[("CFC_TEST_RESULTS_TAR", tar)],
    )
    .report
}

fn codes(r: &Report) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn validate_is_refused_before_anything_is_read_unless_it_is_the_re_100_square_cavity() {
    // A hosts file that does not exist: reading it would be class 4, so class 2 proves the order.
    let no_hosts = [("CFC_HOSTS", "/nonexistent-dir/hosts.toml")];
    let channel = scenario("val-channel", "ok", "validate", CHANNEL, |_| {}, &no_hosts);
    let re_50 = edit(CAVITY, |v| {
        v["fluid"]["kinematic_viscosity"]["value"] = 0.0002.into()
    });
    let oblong = edit(CAVITY, |v| v["domain"]["length_x"]["value"] = 0.12.into());
    for (tag, study) in [("val-re", re_50), ("val-oblong", oblong)] {
        let o = scenario(tag, "ok", "validate", &study, |_| {}, &no_hosts);
        assert_eq!(
            o.report.class(),
            ExitClass::StudyInvalid,
            "{tag}: {}",
            o.raw
        );
        assert!(codes(&o.report).contains(&"CFD-RUN-002"), "{tag}");
        assert!(ssh_log(&o.root).is_empty());
    }
    assert_eq!(channel.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&channel.report).contains(&"CFD-RUN-002"));
    assert_eq!(channel.report.operation, "validate");
    assert!(ssh_log(&channel.root).is_empty());
}

#[test]
fn smoke_is_limited_to_sixty_iterations_and_never_claims_convergence() {
    let no_hosts = [("CFC_HOSTS", "/nonexistent-dir/hosts.toml")];
    let o = scenario("smoke-long", "ok", "smoke", CAVITY, |_| {}, &no_hosts);
    assert_eq!(o.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&o.report).contains(&"CFD-RUN-001"));

    // The channel is 60 iterations: it proceeds, and a failing run is a failed smoke run.
    let o = scenario("smoke-fail", "ok", "smoke", CHANNEL, |_| {}, &[]);
    assert_eq!(o.report.operation, "smoke");
    assert_eq!(o.report.evidence.as_ref().unwrap().verdict, "failed");
    assert_eq!(o.report.class(), ExitClass::GateFailed);

    // A passing smoke run is `smoke_passed`, never `passed`, and says what it does not prove.
    let o = tar_scenario("smoke-pass", "smoke", &cavity_for(60), 60, 0.0, false);
    assert_eq!(o.evidence.as_ref().unwrap().verdict, "smoke_passed");
    assert_eq!(o.class(), ExitClass::Passed);
    assert!(o.render_text().contains("convergence is not claimed"));
}

#[test]
fn a_validate_run_compares_with_the_reference_after_the_evidence_passes() {
    let study = cavity_for(2000);
    let with_tar = |tag: &str, run_mode: &str, shift: f64, samples: bool| {
        tar_scenario(tag, run_mode, &study, 2000, shift, samples)
    };
    let plain = with_tar("run-pass", "run", 0.0, false);
    assert_eq!(plain.evidence.as_ref().unwrap().verdict, "passed");
    assert_eq!(plain.class(), ExitClass::Passed);
    assert!(plain.gate.is_none());

    let valid = with_tar("val-pass", "validate", 0.0, true);
    assert_eq!(valid.class(), ExitClass::Passed, "{}", valid.render_text());
    let gate = valid.gate.as_ref().unwrap();
    assert!(gate.passed && gate.metrics.len() == 2);

    let off = with_tar("val-off", "validate", 0.5, true);
    assert_eq!(off.evidence.as_ref().unwrap().verdict, "passed");
    assert_eq!(off.class(), ExitClass::GateFailed);
    assert!(!off.gate.as_ref().unwrap().passed);

    let missing = with_tar("val-nosamples", "validate", 0.0, false);
    assert_eq!(missing.class(), ExitClass::GateFailed);
    let gate = missing.gate.as_ref().unwrap();
    assert!(!gate.passed && gate.reason.is_some() && gate.metrics.is_empty());
}

#[test]
fn an_exported_case_directory_runs_only_when_it_is_exactly_the_export() {
    let export = |root: &Path| {
        let dir = root.join("exported");
        let r = cfdcraft_ops::export_study(CAVITY, &dir);
        assert_eq!(r.class(), ExitClass::Passed);
        dir
    };
    let run_in = |tag: &str, tamper: &dyn Fn(&Path)| {
        let root = new_root(GROUP, tag);
        let dir = export(&root);
        tamper(&dir);
        scenario_in(
            root,
            "ok",
            "run",
            CAVITY,
            &[("CFC_CASE_DIR", dir.to_str().unwrap())],
        )
    };

    let o = run_in("case-ok", &|_| {});
    assert!(o.report.evidence.is_some(), "the run went ahead: {}", o.raw);
    let run = o.report.run.as_ref().unwrap();
    assert!(
        !o.root.join("runs").join(&run.run_id).join("case").exists(),
        "the exported directory itself was the one uploaded"
    );

    for (tag, expect, tamper) in [
        (
            "case-byte",
            "0/U",
            Box::new(|d: &Path| {
                let p = d.join("0/U");
                let mut b = std::fs::read(&p).unwrap();
                *b.last_mut().unwrap() ^= 1;
                std::fs::write(p, b).unwrap();
            }) as Box<dyn Fn(&Path)>,
        ),
        (
            "case-extra",
            "extra.txt",
            Box::new(|d: &Path| std::fs::write(d.join("extra.txt"), "x").unwrap()),
        ),
        (
            "case-missing",
            "Allrun",
            Box::new(|d: &Path| std::fs::remove_file(d.join("Allrun")).unwrap()),
        ),
    ] {
        let o = run_in(tag, &*tamper);
        assert_eq!(
            o.report.class(),
            ExitClass::GuardRefused,
            "{tag}: {}",
            o.raw
        );
        let d = o
            .report
            .diagnostics
            .iter()
            .find(|d| d.code == "CFD-RT-003")
            .unwrap();
        assert!(d.message.contains(expect), "{tag}: {}", d.message);
        assert!(
            ssh_log(&o.root).is_empty(),
            "{tag}: nothing reached the host"
        );
    }
}
