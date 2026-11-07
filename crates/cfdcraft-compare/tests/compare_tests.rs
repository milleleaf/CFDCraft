use cfdcraft_compare::{KeyNumbers, Tolerance, Verdict, compare};

fn nums(pairs: &[(&str, f64)]) -> KeyNumbers {
    pairs.iter().map(|(n, v)| ((*n).to_owned(), *v)).collect()
}

fn five() -> KeyNumbers {
    nums(&[
        ("solver.iterations", 120.0),
        ("residual.Ux", 1.5e-6),
        ("residual.Uy", 2.5e-6),
        ("residual.p", 8.0e-6),
        ("centreline.vertical.ux.0000", 0.25),
    ])
}

#[test]
fn identical_numbers_are_reproduced() {
    let c = compare(&five(), &five(), Tolerance::same_image());
    assert_eq!(c.verdict, Verdict::Reproduced);
    assert_eq!(c.compared, 5);
    assert_eq!(c.max_abs_diff, 0.0);
    assert!(c.differing.is_empty());
}

#[test]
fn a_difference_inside_the_tolerance_is_within_tolerance() {
    let original = five();
    let mut rerun = five();
    rerun.insert("residual.p".into(), 8.0e-6 + 1e-13);
    let c = compare(&original, &rerun, Tolerance::same_image());
    assert_eq!(c.verdict, Verdict::WithinTolerance);
    assert_eq!(c.differing.len(), 1);
    let d = &c.differing[0];
    assert_eq!(d.name, "residual.p");
    assert_eq!(d.original, Some(8.0e-6));
    assert_eq!(d.rerun, Some(8.0e-6 + 1e-13));
    assert!(d.within);
    assert!(d.abs_diff.is_some_and(|x| x > 0.0 && x < 1e-12));
}

#[test]
fn a_difference_outside_the_tolerance_differs() {
    let original = five();
    let mut rerun = five();
    rerun.insert("solver.iterations".into(), 120.0 + 1e-3);
    let c = compare(&original, &rerun, Tolerance::same_image());
    assert_eq!(c.verdict, Verdict::Differs);
    assert_eq!(c.differing.len(), 1);
    assert_eq!(c.differing[0].name, "solver.iterations");
    assert!(!c.differing[0].within);
    assert!(c.max_abs_diff > 9e-4);
}

#[test]
fn a_name_on_one_side_only_differs() {
    let mut rerun = five();
    rerun.remove("residual.Uy");
    let c = compare(&five(), &rerun, Tolerance::same_image());
    assert_eq!(c.verdict, Verdict::Differs);
    assert_eq!(c.compared, 5);
    let d = &c.differing[0];
    assert_eq!(d.name, "residual.Uy");
    assert_eq!(d.original, Some(2.5e-6));
    assert_eq!(d.rerun, None);
    assert_eq!(d.abs_diff, None);
    assert!(!d.within);

    let mut original = five();
    original.remove("residual.Uy");
    let c = compare(&original, &five(), Tolerance::same_image());
    assert_eq!(c.verdict, Verdict::Differs);
    let d = &c.differing[0];
    assert_eq!(d.original, None);
    assert_eq!(d.rerun, Some(2.5e-6));
    assert_eq!(d.abs_diff, None);
    assert!(!d.within);
}

#[test]
fn a_value_that_is_not_finite_differs() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let side = nums(&[("q", bad)]);
        let finite = nums(&[("q", 1.0)]);
        for (a, b) in [(&side, &finite), (&finite, &side), (&side, &side)] {
            let c = compare(a, b, Tolerance::same_image());
            assert_eq!(c.verdict, Verdict::Differs);
            assert_eq!(c.differing.len(), 1);
            assert!(!c.differing[0].within);
            assert_eq!(c.differing[0].abs_diff, None);
        }
    }
}

#[test]
fn the_relative_part_scales_with_the_size() {
    let t = Tolerance::new(0.0, 1e-6).unwrap();
    let c = compare(
        &nums(&[("q", 1_000_000.0)]),
        &nums(&[("q", 1_000_000.5)]),
        t,
    );
    assert_eq!(c.verdict, Verdict::WithinTolerance);
    let c = compare(&nums(&[("q", 1.0)]), &nums(&[("q", 1.5)]), t);
    assert_eq!(c.verdict, Verdict::Differs);
}

#[test]
fn the_differing_list_is_sorted_by_name() {
    let original = nums(&[("z", 1.0), ("a", 1.0), ("m", 1.0), ("same", 1.0)]);
    let rerun = nums(&[("m", 2.0), ("z", 2.0), ("same", 1.0), ("a", 2.0)]);
    let c = compare(&original, &rerun, Tolerance::same_image());
    let names: Vec<&str> = c.differing.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["a", "m", "z"]);
    assert_eq!(c.compared, 4);
}

#[test]
fn empty_maps_are_reproduced() {
    let c = compare(
        &KeyNumbers::new(),
        &KeyNumbers::new(),
        Tolerance::same_image(),
    );
    assert_eq!(c.verdict, Verdict::Reproduced);
    assert_eq!(c.compared, 0);
    assert_eq!(c.max_abs_diff, 0.0);
}

#[test]
fn negative_zero_is_identical_to_zero() {
    let c = compare(
        &nums(&[("q", 0.0)]),
        &nums(&[("q", -0.0)]),
        Tolerance::same_image(),
    );
    assert_eq!(c.verdict, Verdict::Reproduced);
}
