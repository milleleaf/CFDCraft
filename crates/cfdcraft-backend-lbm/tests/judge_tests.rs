//! A finished lattice run is judged from its replies, never from an exit status, and a convergence
//! verdict comes from the change measure of a steady run or the lift of a run in time. Every file
//! here is synthetic: no solver ran.

#[path = "support/replies.rs"]
mod replies;

use cfdcraft_backend::{Backend, JudgeMode, JudgeOutcome};
use cfdcraft_backend_lbm::{
    ID, backend::preview_notice, backends, claims::CLAIMS, commands::REFERENCE_DENSITY, markers,
    obstacles,
};
use cfdcraft_model::{ResolvedStudy, resolve};
use cfdcraft_templates::{TemplateOptions, build};
use replies::{Mem, Stream, good_solve};
use serde_json::{Value, json};

fn backend() -> &'static dyn Backend {
    backends()[0]
}

fn template(id: &str) -> ResolvedStudy {
    resolve(&build(id, &TemplateOptions::default()).unwrap()).unwrap()
}

fn judged(files: &Mem, mode: JudgeMode) -> JudgeOutcome {
    backend().judge(files, &template("lid-cavity"), mode)
}

fn run_with(solve: Value) -> Mem {
    Stream::run(None, &solve).files()
}

/// The good solve answer with one member replaced.
fn solve_with(member: &str, value: Value) -> Value {
    let mut solve = good_solve();
    solve[member] = value;
    solve
}

#[test]
fn a_clean_reply_file_passes() {
    let files = Stream::good().files();
    for (mode, verdict) in [
        (JudgeMode::Run, "passed"),
        (JudgeMode::Smoke, "smoke_passed"),
        (JudgeMode::Finished, "passed"),
    ] {
        let outcome = judged(&files, mode.clone());
        assert_eq!(outcome.verdict, verdict, "{mode:?}: {:?}", outcome.reasons);
        assert!(outcome.reasons.is_empty(), "{:?}", outcome.reasons);
    }
    // A step count that the request names and the reply meets passes as well.
    let named = Stream::run(Some(1200), &good_solve()).files();
    assert_eq!(judged(&named, JudgeMode::Run).verdict, "passed");
    // A steady stop takes fewer steps than the request names.
    let steady = Stream::run(Some(5000), &solve_with("stop", json!("steady"))).files();
    assert_eq!(judged(&steady, JudgeMode::Run).verdict, "passed");
}

#[test]
fn each_piece_of_evidence_is_checked() {
    let mut cases: Vec<(&str, Mem)> = Vec::new();

    let mut stream = Stream::good();
    stream.replies.pop();
    cases.push(("a missing reply", stream.files()));

    let mut stream = Stream::good();
    stream.replies[1] =
        json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32000, "message": "refused"}});
    cases.push(("an error reply", stream.files()));

    cases.push((
        "a different step count",
        Stream::run(Some(1000), &good_solve()).files(),
    ));
    cases.push(("no step count", run_with(solve_with("steps", Value::Null))));
    cases.push(("no steps taken", run_with(solve_with("steps", json!(0)))));
    cases.push((
        "an instability warning",
        run_with(solve_with(
            "warnings",
            json!(["lattice speed too high: the flow is Unstable"]),
        )),
    ));
    cases.push((
        "a mass change above the limit",
        run_with(solve_with(
            "mass",
            json!({"relative_change": markers::MASS_CHANGE_LIMIT * 2.0}),
        )),
    ));
    cases.push(("no mass change", run_with(solve_with("mass", Value::Null))));
    cases.push((
        "an unusual stop",
        run_with(solve_with("stop", json!("diverged"))),
    ));
    cases.push(("no stop reason", run_with(solve_with("stop", Value::Null))));
    cases.push((
        "an empty log",
        Stream::good().files().with("log.fieldcraft", "  \n"),
    ));
    cases.push(("no log", Stream::good().files().without("log.fieldcraft")));
    cases.push((
        "no replies",
        Stream::good().files().without("replies.jsonl"),
    ));
    cases.push((
        "no requests",
        Stream::good().files().without("commands.jsonl"),
    ));

    let mut seen: Vec<(&str, String)> = Vec::new();
    for (name, files) in &cases {
        let outcome = judged(files, JudgeMode::Run);
        assert_eq!(outcome.verdict, "failed", "{name}");
        assert!(!outcome.reasons.is_empty(), "{name}");
        let text = outcome.reasons.join(" ");
        for (other, earlier) in &seen {
            assert_ne!(&text, earlier, "{name} and {other} share a reason");
        }
        seen.push((*name, text));
        // The same files fail in the other modes too: the evidence decides, not the mode.
        assert_eq!(judged(files, JudgeMode::Smoke).verdict, "failed", "{name}");
        assert_eq!(
            judged(files, JudgeMode::Finished).verdict,
            "failed",
            "{name}"
        );
    }
}

#[test]
fn a_stopped_run_is_not_judged_as_a_good_one() {
    let outcome = judged(
        &Stream::good().files(),
        JudgeMode::Stopped {
            time: "0.5".to_owned(),
        },
    );
    assert_eq!(outcome.verdict, "failed");
}

#[test]
fn a_file_past_the_read_cap_is_said_to_be_cut() {
    let long = markers::READ_CAP_BYTES + 1;
    // The text is whole, only its size says it was long: the verdict stays what the text gives.
    let passing = Stream::good().files().with_size("replies.jsonl", long);
    let failing = Stream::good()
        .files()
        .with_size("log.fieldcraft", long)
        .with("log.fieldcraft", " ");
    for files in [passing, failing] {
        let outcome = judged(&files, JudgeMode::Run);
        let sentence = outcome
            .reasons
            .iter()
            .find(|reason| reason.contains("read only up to"))
            .unwrap_or_else(|| panic!("{:?}", outcome.reasons));
        assert!(
            !sentence.contains('/') && !sentence.contains('\\'),
            "{sentence}"
        );
        assert!(
            !sentence.contains("jsonl") && !sentence.contains("fieldcraft"),
            "{sentence}"
        );
    }
    // Without a long file the sentence is not there.
    let outcome = judged(&Stream::good().files(), JudgeMode::Run);
    assert!(outcome.reasons.is_empty());
}

#[test]
fn the_report_text_says_preview() {
    assert!(backend().preview());
    assert_eq!(backend().id(), ID);
    assert_eq!(preview_notice(), "CFD-BACKEND-009");
    assert!(CLAIMS.is_empty());
    let words = ["gate", "claim", "parity", "pass", "gated", "reproduce"];
    let mut stream = Stream::good();
    stream.replies.pop();
    let outcome = judged(&stream.files(), JudgeMode::Run);
    let cut = judged(
        &Stream::good()
            .files()
            .with_size("replies.jsonl", markers::READ_CAP_BYTES + 1),
        JudgeMode::Run,
    );
    for text in outcome.reasons.iter().chain(&cut.reasons) {
        let lower = text.to_lowercase();
        for word in words {
            assert!(!lower.contains(word), "{text}");
        }
    }
}

// Convergence.

fn steady() -> ResolvedStudy {
    let study = template("lid-cavity");
    assert!(!backend().run_plan(&study, 1).transient);
    study
}

fn limit(study: &ResolvedStudy) -> f64 {
    study.solver.convergence.p.min(study.solver.convergence.u)
}

fn change_run(change: Vec<f64>) -> Mem {
    run_with(solve_with("change", json!(change)))
}

fn converged(study: &ResolvedStudy, files: &Mem) -> (bool, String) {
    let outcome = backend().convergence(files, study).expect("a verdict");
    (outcome.converged, outcome.reasons.join(" "))
}

#[test]
fn a_steady_run_within_the_limits_is_converged() {
    let study = steady();
    let small = limit(&study) / 10.0;
    let files = change_run(vec![1e-2, 1e-3, 1e-4, small * 3.0, small * 2.0, small]);
    assert_eq!(converged(&study, &files), (true, String::new()));
    // One value is a series of one.
    assert_eq!(
        converged(&study, &change_run(vec![small])),
        (true, String::new())
    );
}

#[test]
fn a_steady_run_with_a_rising_tail_is_not_converged() {
    let study = steady();
    let small = limit(&study) / 10.0;
    let above = converged(&study, &change_run(vec![1e-2, 1e-3, limit(&study) * 2.0]));
    assert!(!above.0);
    assert!(above.1.contains("above the limit"), "{}", above.1);

    // Twenty values: the last tenth is the last two, and the last one is higher than the one before.
    let mut rising: Vec<f64> = (0..18).map(|i| small * 100.0 / f64::from(i + 1)).collect();
    rising.extend([small / 10.0, small]);
    let rise = converged(&study, &change_run(rising));
    assert!(!rise.0);
    assert!(rise.1.contains("rising"), "{}", rise.1);
    assert!(!rise.1.contains("above the limit"), "{}", rise.1);
    assert_ne!(above.1, rise.1);
}

/// A force answer for the cylinder: Cd and Cl scaled back to newtons for the body's walls, split
/// over two walls of the body, every `step` seconds up to `end`. The lift is a sine of `hertz`
/// whose amplitude grows by `growth` per second.
fn cylinder_forces(study: &ResolvedStudy, end: f64, step: f64, hertz: f64, growth: f64) -> Value {
    let lattice = obstacles::lattice(study).unwrap();
    let force = &study.monitor_plan().forces[0];
    let scale = 0.5
        * REFERENCE_DENSITY
        * force.reference_velocity
        * force.reference_velocity
        * force.reference_length
        * lattice.slab();
    let count = (end / step).round() as usize;
    let times: Vec<f64> = (1..=count).map(|i| i as f64 * step).collect();
    let rows = |share: f64| -> Vec<[f64; 3]> {
        times
            .iter()
            .map(|t| {
                let cd = 3.23 + 0.01 * (2.0 * std::f64::consts::PI * 2.0 * hertz * t).sin();
                let cl = (1.0 + growth * t) * (2.0 * std::f64::consts::PI * hertz * t).sin();
                [share * cd * scale, share * cl * scale, 0.0]
            })
            .collect()
    };
    let body = lattice.obstacle.as_ref().unwrap().name;
    json!({
        "times": times,
        "forces": {(format!("{body}/F1:side")): rows(0.25), (format!("{body}/F2:side")): rows(0.75)},
    })
}

fn transient_run(study: &ResolvedStudy, answer: Option<Value>) -> Mem {
    let mut stream = Stream::good();
    if let Some(answer) = answer {
        let force = &study.monitor_plan().forces[0];
        stream.probe(json!({"step": "Run", "forces": [force.boundary]}), &answer);
    }
    stream.files()
}

#[test]
fn a_settled_transient_run_is_converged() {
    let study = template("cylinder");
    assert!(backend().run_plan(&study, 1).transient);
    let settled = transient_run(&study, Some(cylinder_forces(&study, 8.0, 0.002, 3.0, 0.0)));
    let (ok, reason) = converged(&study, &settled);
    assert!(ok, "{reason}");

    // The same view serves the series the verdict read.
    let view = backend().results(Box::new(settled), &study);
    let fo = &study.monitor_plan().forces[0].fo;
    assert!(
        view.read(&format!("postProcessing/{fo}/0/coefficient.dat"))
            .is_some()
    );

    // An amplitude that keeps growing is not settled.
    let growing = transient_run(&study, Some(cylinder_forces(&study, 8.0, 0.002, 3.0, 1.0)));
    let (ok, reason) = converged(&study, &growing);
    assert!(!ok);
    assert!(reason.contains("still changes"), "{reason}");

    // Too short to hold ten cycles.
    let short = transient_run(&study, Some(cylinder_forces(&study, 1.0, 0.002, 3.0, 0.0)));
    let (ok, reason) = converged(&study, &short);
    assert!(!ok);
    assert!(reason.contains("too few"), "{reason}");
}

#[test]
fn a_missing_series_is_not_converged_with_its_reason() {
    let study = template("cylinder");
    let (ok, reason) = converged(&study, &transient_run(&study, None));
    assert!(!ok);
    assert!(reason.contains("coefficient series"), "{reason}");

    let cavity = steady();
    let mut solve = good_solve();
    solve.as_object_mut().unwrap().remove("change");
    let (ok, reason) = converged(&cavity, &run_with(solve));
    assert!(!ok);
    assert!(reason.contains("no change measure"), "{reason}");

    let (ok, reason) = converged(&cavity, &Mem::default());
    assert!(!ok);
    assert!(reason.contains("solve reply is missing"), "{reason}");
}
