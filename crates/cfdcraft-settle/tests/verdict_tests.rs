use cfdcraft_settle::{
    Balance, BalanceStatus, NOT_FINISHED, QuantityState, SOLVER_FAILED, Settings, Status, Verdict,
    VerdictState, verdict,
};

fn q(name: &str, status: Status) -> QuantityState {
    QuantityState {
        name: name.to_string(),
        last: 1.0,
        mean: 1.0,
        spread: 0.0,
        drift: 0.0,
        status,
        samples: 50,
    }
}

fn balance(status: BalanceStatus) -> Balance {
    Balance {
        status,
        imbalance: None,
        reference: None,
    }
}

const NA: BalanceStatus = BalanceStatus::NotApplicable;
const OK: BalanceStatus = BalanceStatus::Ok;
const FAILED: BalanceStatus = BalanceStatus::Failed;

fn decide(
    quantities: Vec<QuantityState>,
    mass: BalanceStatus,
    energy: BalanceStatus,
    r: bool,
) -> Verdict {
    verdict(
        quantities,
        balance(mass),
        balance(energy),
        r,
        &Settings::default(),
    )
}

struct Row {
    name: &'static str,
    quantities: Vec<QuantityState>,
    mass: BalanceStatus,
    energy: BalanceStatus,
    residuals_ok: bool,
    state: VerdictState,
    reasons: Vec<&'static str>,
}

#[test]
fn the_verdict_matrix() {
    use VerdictState::{Converged, NotConverged, NotJudged};
    let rows = vec![
        Row {
            name: "no quantity, residuals ok",
            quantities: vec![],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: NotJudged,
            reasons: vec!["no_monitored_quantity"],
        },
        Row {
            name: "no quantity, nothing else ok",
            quantities: vec![],
            mass: FAILED,
            energy: FAILED,
            residuals_ok: false,
            state: NotJudged,
            reasons: vec!["no_monitored_quantity"],
        },
        Row {
            name: "all settled, balances not applicable, residuals ok",
            quantities: vec![q("a", Status::Settled), q("b", Status::Settled)],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: Converged,
            reasons: vec![],
        },
        Row {
            name: "all settled, balances ok",
            quantities: vec![q("a", Status::Settled)],
            mass: OK,
            energy: OK,
            residuals_ok: true,
            state: Converged,
            reasons: vec![],
        },
        Row {
            name: "all settled, residuals not ok",
            quantities: vec![q("a", Status::Settled)],
            mass: NA,
            energy: NA,
            residuals_ok: false,
            state: NotConverged,
            reasons: vec!["residuals_above_limit"],
        },
        Row {
            name: "one drifting",
            quantities: vec![q("a", Status::Settled), q("u", Status::Drifting)],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["not_settled:u"],
        },
        Row {
            name: "one oscillating",
            quantities: vec![q("p", Status::Oscillating)],
            mass: OK,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["not_settled:p"],
        },
        Row {
            name: "mass failed",
            quantities: vec![q("a", Status::Settled)],
            mass: FAILED,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["mass_imbalance"],
        },
        Row {
            name: "energy failed",
            quantities: vec![q("a", Status::Settled)],
            mass: OK,
            energy: FAILED,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["energy_imbalance"],
        },
        Row {
            name: "too early is not settled",
            quantities: vec![q("a", Status::TooEarly)],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["not_settled:a"],
        },
        Row {
            name: "too few samples is not settled",
            quantities: vec![q("line.mean_Ux", Status::TooFewSamples)],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["not_settled:line.mean_Ux"],
        },
        Row {
            name: "an error is not settled",
            quantities: vec![q("a", Status::Error)],
            mass: NA,
            energy: NA,
            residuals_ok: true,
            state: NotConverged,
            reasons: vec!["not_settled:a"],
        },
        Row {
            name: "every reason, in order",
            quantities: vec![
                q("b", Status::Drifting),
                q("c", Status::Settled),
                q("a", Status::Oscillating),
            ],
            mass: FAILED,
            energy: FAILED,
            residuals_ok: false,
            state: NotConverged,
            reasons: vec![
                "not_settled:a",
                "not_settled:b",
                "mass_imbalance",
                "energy_imbalance",
                "residuals_above_limit",
            ],
        },
    ];
    assert!(rows.len() >= 8);
    for row in rows {
        let kept = row.quantities.clone();
        let v = decide(row.quantities, row.mass, row.energy, row.residuals_ok);
        assert_eq!(v.state, row.state, "{}", row.name);
        assert_eq!(v.reasons, row.reasons, "{}", row.name);
        assert_eq!(
            v.quantities, kept,
            "{}: the quantities are carried",
            row.name
        );
        assert_eq!(v.mass.status, row.mass, "{}", row.name);
        assert_eq!(v.energy.status, row.energy, "{}", row.name);
        assert_eq!(v.residuals_ok, row.residuals_ok, "{}", row.name);
    }
}

#[test]
fn residuals_alone_never_converge() {
    for residuals_ok in [true, false] {
        for mass in [OK, FAILED, NA] {
            for energy in [OK, FAILED, NA] {
                let v = decide(vec![], mass, energy, residuals_ok);
                assert_ne!(
                    v.state,
                    VerdictState::Converged,
                    "residuals_ok {residuals_ok}, mass {mass:?}, energy {energy:?}"
                );
                assert_eq!(v.state, VerdictState::NotJudged);
            }
        }
    }
}

#[test]
fn a_verdict_round_trips_through_json() {
    let v = decide(
        vec![
            q("probe_1.Ux", Status::Settled),
            q("probe_2.p", Status::Drifting),
        ],
        OK,
        NA,
        true,
    );
    let text = serde_json::to_string(&v).unwrap();
    assert!(text.contains("\"state\":\"not_converged\""), "{text}");
    assert!(text.contains("\"not_settled:probe_2.p\""), "{text}");
    assert!(text.contains("\"status\":\"not_applicable\""), "{text}");
    let back: Verdict = serde_json::from_str(&text).unwrap();
    assert_eq!(back, v);
    for (state, word) in [
        (VerdictState::Converged, "converged"),
        (VerdictState::NotConverged, "not_converged"),
        (VerdictState::NotJudged, "not_judged"),
    ] {
        assert_eq!(
            serde_json::to_string(&state).unwrap(),
            format!("\"{word}\"")
        );
    }
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["surprise"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Verdict>(value).is_err());
}

#[test]
fn a_blocked_verdict_is_never_converged_and_keeps_what_it_was_made_from() {
    let settled = decide(vec![q("a", Status::Settled)], NA, NA, true);
    assert_eq!(settled.state, VerdictState::Converged);

    let blocked = settled.clone().blocked_by(SOLVER_FAILED);
    assert_eq!(blocked.state, VerdictState::NotConverged);
    assert_eq!(blocked.reasons, [SOLVER_FAILED]);
    assert_eq!(blocked.quantities, settled.quantities);

    // The reasons already there stay first, and a reason is added once.
    let open = decide(vec![q("a", Status::Drifting)], NA, NA, true);
    let twice = open.blocked_by(NOT_FINISHED).blocked_by(NOT_FINISHED);
    assert_eq!(twice.state, VerdictState::NotConverged);
    assert_eq!(twice.reasons, ["not_settled:a", NOT_FINISHED]);

    // A run that cannot be judged stays so, with the reason noted.
    let none = decide(vec![], NA, NA, true).blocked_by(SOLVER_FAILED);
    assert_eq!(none.state, VerdictState::NotJudged);
    assert_eq!(none.reasons, ["no_monitored_quantity", SOLVER_FAILED]);
}
