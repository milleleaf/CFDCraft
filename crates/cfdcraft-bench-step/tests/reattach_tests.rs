use cfdcraft_bench_step::reattachment_length;

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-9)
}

#[test]
fn the_last_negative_to_positive_crossing_is_the_reattachment() {
    // A corner eddy next to the step (positive, then negative), the bubble, then attached flow.
    let line = [
        (2.0, 0.25),
        (2.5, 0.125),
        (3.0, -0.5),
        (4.0, -1.0),
        (5.0, -0.75),
        (6.0, -0.26),
        (7.0, 0.74),
        (8.0, 1.0),
        (9.0, 1.0),
    ];
    // The crossing is at x = 6.26; the step is at x = 2.
    assert!(close(reattachment_length(&line, 2.0), 4.26));

    assert_eq!(reattachment_length(&[(0.0, 1.0), (1.0, 2.0)], 0.0), None);
    assert_eq!(
        reattachment_length(&[(0.0, 1.0), (1.0, -1.0), (2.0, -2.0)], 0.0),
        None
    );
    assert_eq!(reattachment_length(&[(0.0, -1.0)], 0.0), None);
    assert_eq!(reattachment_length(&[], 0.0), None);

    // Two bubbles: the last crossing counts.
    let two = [(0.0, -1.0), (1.0, 1.0), (2.0, -1.0), (3.0, 1.0)];
    assert!(close(reattachment_length(&two, 0.0), 2.5));

    // A sample that is exactly zero is the crossing, once.
    let zero = [(0.0, -1.0), (1.0, 0.0), (2.0, 1.0)];
    assert!(close(reattachment_length(&zero, 0.0), 1.0));
}
