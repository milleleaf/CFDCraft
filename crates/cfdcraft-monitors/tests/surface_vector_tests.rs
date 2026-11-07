//! A surface file whose column holds vectors is read one component at a time.

mod support;

use cfdcraft_monitors::{parse_surface, surface_series, surface_series_component};
use support::{close, surface_text};

fn file() -> cfdcraft_monitors::SurfaceFile {
    let rows = vec![
        (1.0, vec![vec![0.5, -1.5, 2.5], vec![10.0]]),
        (2.0, vec![vec![0.75, -1.75, 2.75], vec![20.0]]),
    ];
    let text = surface_text(
        "patch hot",
        32,
        0.25,
        &["areaIntegrate(gradT)", "areaIntegrate(p)"],
        &rows,
    );
    parse_surface(&text).unwrap()
}

#[test]
fn a_vector_column_is_read_one_component_at_a_time() {
    let file = file();
    for (component, axis, first, second) in [
        (0, "x", 0.5, 0.75),
        (1, "y", -1.5, -1.75),
        (2, "z", 2.5, 2.75),
    ] {
        let series = surface_series_component(&file, "surface_1", component);
        assert_eq!(series.len(), 1, "the scalar column is skipped");
        assert_eq!(
            series[0].name,
            format!("surface_1.areaIntegrate(gradT).{axis}")
        );
        assert_eq!(series[0].times, vec![1.0, 2.0]);
        close(series[0].values[0], first);
        close(series[0].values[1], second);
    }
    assert!(surface_series_component(&file, "surface_1", 3).is_empty());
}

#[test]
fn the_scalar_reader_still_skips_a_vector_column() {
    let file = file();
    let series = surface_series(
        &file,
        "surface_1",
        &[
            ("areaIntegrate(gradT)", "vector"),
            ("areaIntegrate(p)", "scalar"),
        ],
    );
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].name, "surface_1.scalar");
}
