use cfdcraft_settle::{BalanceStatus, PatchFlux, Settings, energy_balance, mass_balance};

fn patch(name: &str, inlet: bool, sum: f64) -> PatchFlux {
    PatchFlux {
        patch: name.to_string(),
        inlet,
        sum,
        sum_mag: sum.abs(),
    }
}

#[test]
fn a_closed_domain_has_no_mass_balance_to_judge() {
    let s = Settings::default();
    let cases = [
        ("no patches", vec![]),
        ("walls only, closing", vec![patch("wall", false, 0.0)]),
        (
            "walls only, large sums",
            vec![patch("lid", false, 5.0), patch("floor", false, -1.0)],
        ),
    ];
    for (name, patches) in cases {
        let b = mass_balance(&patches, &s);
        assert_eq!(b.status, BalanceStatus::NotApplicable, "{name}");
        assert_eq!(b.imbalance, None, "{name}");
        assert_eq!(b.reference, None, "{name}");
    }
}

#[test]
fn a_closing_inlet_outlet_pair_is_ok() {
    let b = mass_balance(
        &[
            patch("inlet", true, -1.0e-3),
            patch("outlet", false, 1.0e-3 + 5e-8),
            patch("wall", false, 0.0),
        ],
        &Settings::default(),
    );
    assert_eq!(b.status, BalanceStatus::Ok);
    let imbalance = b.imbalance.unwrap();
    assert!((imbalance - 5e-5).abs() < 1e-9, "imbalance {imbalance}");
    assert!((b.reference.unwrap() - 1.0e-3).abs() < 1e-15);
}

#[test]
fn an_imbalance_over_a_tenth_of_a_percent_fails() {
    let s = Settings::default();
    let b = mass_balance(
        &[
            patch("inlet", true, -1.0e-3),
            patch("outlet", false, 1.002e-3),
        ],
        &s,
    );
    assert_eq!(b.status, BalanceStatus::Failed);
    assert!((b.imbalance.unwrap() - 2e-3).abs() < 1e-9);
    // The tolerance is a setting.
    let loose = Settings {
        mass_tol: 0.01,
        ..Settings::default()
    };
    let b = mass_balance(
        &[
            patch("inlet", true, -1.0e-3),
            patch("outlet", false, 1.002e-3),
        ],
        &loose,
    );
    assert_eq!(b.status, BalanceStatus::Ok);
    // Two inlets add up in the reference, and an imbalance in either direction counts.
    let b = mass_balance(
        &[
            patch("a", true, -1.0e-3),
            patch("b", true, -1.0e-3),
            patch("out", false, 1.0e-3),
        ],
        &s,
    );
    assert_eq!(b.status, BalanceStatus::Failed);
    assert_eq!(b.reference, Some(2.0e-3));
}

#[test]
fn an_inlet_with_zero_flux_cannot_close() {
    let b = mass_balance(
        &[patch("inlet", true, 0.0), patch("outlet", false, 0.0)],
        &Settings::default(),
    );
    assert_eq!(b.status, BalanceStatus::Failed);
    assert_eq!(b.reference, Some(0.0));
    assert_eq!(b.imbalance, None);
}

#[test]
fn a_flux_that_is_not_a_number_fails() {
    let b = mass_balance(
        &[patch("inlet", true, -1.0), patch("outlet", false, f64::NAN)],
        &Settings::default(),
    );
    assert_eq!(b.status, BalanceStatus::Failed);
    assert_eq!(b.imbalance, None);
    assert_eq!(b.reference, Some(1.0));
}

#[test]
fn energy_is_not_applicable_unless_solved() {
    let s = Settings::default();
    let none = energy_balance(None, &s);
    assert_eq!(none.status, BalanceStatus::NotApplicable);
    assert_eq!(none.imbalance, None);
    let ok = energy_balance(Some((5e-3, 1.0)), &s);
    assert_eq!(ok.status, BalanceStatus::Ok);
    assert_eq!(ok.imbalance, Some(5e-3));
    assert_eq!(ok.reference, Some(1.0));
    let failed = energy_balance(Some((0.02, 1.0)), &s);
    assert_eq!(failed.status, BalanceStatus::Failed);
    assert_eq!(failed.imbalance, Some(0.02));
    // Negative flow is judged by size, and a reference of zero cannot close.
    assert_eq!(
        energy_balance(Some((-5e-3, -1.0)), &s).status,
        BalanceStatus::Ok
    );
    let zero = energy_balance(Some((0.0, 0.0)), &s);
    assert_eq!(zero.status, BalanceStatus::Failed);
    assert_eq!(zero.reference, Some(0.0));
}

#[test]
fn a_balance_serialises_with_snake_case_words() {
    for (status, word) in [
        (BalanceStatus::Ok, "ok"),
        (BalanceStatus::Failed, "failed"),
        (BalanceStatus::NotApplicable, "not_applicable"),
    ] {
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{word}\"")
        );
    }
}
