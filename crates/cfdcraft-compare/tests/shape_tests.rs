use cfdcraft_compare::{Comparison, KeyNumbers, Tolerance, Verdict, compare};
use serde_json::{Value, json};

fn nums(pairs: &[(&str, f64)]) -> KeyNumbers {
    pairs.iter().map(|(n, v)| ((*n).to_owned(), *v)).collect()
}

/// One comparison per verdict, all with finite numbers so each can be written as JSON.
fn three() -> [Comparison; 3] {
    let base = nums(&[("a", 1.0), ("b", 2.0)]);
    let near = nums(&[("a", 1.0), ("b", 2.0 + 1e-13)]);
    let far = nums(&[("a", 1.0), ("b", 3.0)]);
    let t = Tolerance::same_image();
    [
        compare(&base, &base, t),
        compare(&base, &near, t),
        compare(&base, &far, t),
    ]
}

#[test]
fn a_bad_tolerance_is_refused_with_its_value_named() {
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        for (abs, rel) in [(bad, 0.0), (0.0, bad)] {
            let err = Tolerance::new(abs, rel).unwrap_err();
            assert!(
                err.to_string().contains(&format!("{bad}")),
                "message {err} should name {bad}"
            );
        }
    }
    assert!(Tolerance::new(0.0, 0.0).is_ok());
}

#[test]
fn the_default_tolerance_is_the_stated_one() {
    assert_eq!(
        Tolerance::same_image(),
        Tolerance {
            abs: 1e-12,
            rel: 1e-9
        }
    );
}

#[test]
fn a_comparison_serialises_with_stable_names() {
    let [_, _, far] = three();
    let v = serde_json::to_value(&far).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "compared",
            "differing",
            "max_abs_diff",
            "tolerance",
            "verdict"
        ]
    );
    assert_eq!(v["verdict"], "differs");
    assert_eq!(v["tolerance"], json!({"abs": 1e-12, "rel": 1e-9}));
    assert_eq!(v["compared"], 2);
    let entry = v["differing"][0].as_object().unwrap();
    let mut entry_keys: Vec<&str> = entry.keys().map(String::as_str).collect();
    entry_keys.sort_unstable();
    assert_eq!(
        entry_keys,
        ["abs_diff", "name", "original", "rerun", "within"]
    );
    assert_eq!(entry["name"], "b");
    assert_eq!(entry["within"], json!(false));

    let strings: Vec<Value> = [
        Verdict::Reproduced,
        Verdict::WithinTolerance,
        Verdict::Differs,
    ]
    .iter()
    .map(|v| serde_json::to_value(v).unwrap())
    .collect();
    assert_eq!(strings, ["reproduced", "within_tolerance", "differs"]);
    for v in [
        Verdict::Reproduced,
        Verdict::WithinTolerance,
        Verdict::Differs,
    ] {
        assert_eq!(serde_json::to_value(v).unwrap(), v.as_str());
    }
}

#[test]
fn a_comparison_survives_a_json_round_trip() {
    let verdicts: Vec<Verdict> = three().iter().map(|c| c.verdict).collect();
    assert_eq!(
        verdicts,
        [
            Verdict::Reproduced,
            Verdict::WithinTolerance,
            Verdict::Differs
        ]
    );
    for c in three() {
        let text = serde_json::to_string(&c).unwrap();
        let back: Comparison = serde_json::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}

#[test]
fn a_one_sided_name_is_written_with_null_not_skipped() {
    let c = compare(
        &nums(&[("a", 1.0)]),
        &KeyNumbers::new(),
        Tolerance::same_image(),
    );
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["differing"][0]["rerun"], Value::Null);
    assert_eq!(v["differing"][0]["abs_diff"], Value::Null);
    let back: Comparison = serde_json::from_value(v).unwrap();
    assert_eq!(back, c);
}

#[test]
fn the_same_inputs_always_give_the_same_comparison() {
    let a = nums(&[("a", 1.0), ("b", 2.0), ("c", 3.0)]);
    let b = nums(&[("a", 1.0), ("b", 2.5), ("c", 3.0 + 1e-13)]);
    let t = Tolerance::same_image();
    let runs = [compare(&a, &b, t), compare(&a, &b, t), compare(&a, &b, t)];
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
    let texts: Vec<String> = runs
        .iter()
        .map(|c| serde_json::to_string(c).unwrap())
        .collect();
    assert_eq!(texts[0], texts[1]);
    assert_eq!(texts[1], texts[2]);
}
