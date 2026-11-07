//! The probe, surface and line parsers over files a real solver wrote.
//!
//! Every number below was read from the fixture file itself. The files are whole and unchanged:
//! see `PROVENANCE.md`.

use cfdcraft_monitors::{
    FieldKind, Value, parse_probes, parse_surface, parse_xy, probe_series, surface_series,
};

const PROBES: &str = include_str!("fixtures/v2512-real-probe_4-U.dat");
const SURFACE: &str = include_str!("fixtures/v2512-real-surfaceFieldValue-movingWall.dat");
const LINE: &str = include_str!("fixtures/v2512-real-centreline-vertical_U.xy");

#[test]
fn a_real_probes_file_has_one_probe_and_a_row_per_iteration() {
    assert!(PROBES.starts_with("# Probe 0 (0.025 0.075 0.025)\n# Time"));
    let file = parse_probes(PROBES).unwrap();
    assert_eq!(file.probes.len(), 1);
    assert_eq!(file.probes[0].index, 0);
    assert_eq!(file.probes[0].location, [0.025, 0.075, 0.025]);
    assert!(!file.probes[0].not_found);
    assert_eq!(file.rows.len(), 5000);
    assert_eq!(file.rows[0].time, 1.0);
    assert_eq!(
        file.rows[0].values,
        [Value::Vector([-0.000203356288285, 0.000135872932648, 0.0])]
    );
    let last = file.rows.last().unwrap();
    assert_eq!(last.time, 5000.0);
    assert_eq!(
        last.values,
        [Value::Vector([-0.00381060030536, 0.0242331591981, 0.0])]
    );
    assert!(file.rows.windows(2).all(|pair| pair[0].time < pair[1].time));
}

#[test]
fn a_real_probes_file_gives_ux_and_uy_series() {
    let file = parse_probes(PROBES).unwrap();
    let series = probe_series(&file, "probe_4", 0, FieldKind::Velocity);
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["probe_4.Ux", "probe_4.Uy"]);
    for s in &series {
        assert_eq!(s.times.len(), 5000);
        assert_eq!(s.values.len(), 5000);
        assert_eq!(s.times[0], 1.0);
        assert_eq!(s.times[4999], 5000.0);
    }
    assert_eq!(series[0].values[0], -0.000203356288285);
    assert_eq!(series[0].values[4999], -0.00381060030536);
    assert_eq!(series[1].values[0], 0.000135872932648);
    assert_eq!(series[1].values[4999], 0.0242331591981);
}

#[test]
fn a_real_surface_file_has_its_region_columns_and_rows() {
    let file = parse_surface(SURFACE).unwrap();
    assert_eq!(file.region, "patch movingWall");
    assert_eq!(file.faces, 128);
    assert_eq!(file.area, 5.0e-3);
    assert_eq!(file.columns, ["sum(phi)"]);
    assert_eq!(file.rows.len(), 5000);
    assert_eq!(file.rows[0].time, 1.0);
    assert_eq!(file.rows[4999].time, 5000.0);
    // A closed cavity passes no flux through any wall: every value is exactly zero.
    assert!(
        file.rows
            .iter()
            .all(|row| row.values == [Value::Scalar(0.0)])
    );
}

#[test]
fn a_real_surface_file_gives_a_series_for_its_column() {
    let file = parse_surface(SURFACE).unwrap();
    let series = surface_series(&file, "movingWall", &[("sum(phi)", "flux"), ("area", "x")]);
    assert_eq!(series.len(), 1, "a column the file lacks is skipped");
    assert_eq!(series[0].name, "movingWall.flux");
    assert_eq!(series[0].times.len(), 5000);
    assert!(series[0].values.iter().all(|v| *v == 0.0));
}

#[test]
fn a_real_sampled_line_is_one_coordinate_then_the_three_velocity_components() {
    // Written by a `sets` monitor with `axis y`: the distance along the line, then Ux Uy Uz.
    // The line monitors the writer makes use `axis xyz` (three coordinates first) and have not
    // yet been recorded from a real run.
    let file = parse_xy(LINE).unwrap();
    assert_eq!(file.columns, 4);
    assert_eq!(file.rows.len(), 64);
    assert_eq!(file.rows[0], [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        file.rows[1],
        [0.0015873015873, -0.00116496772114, 1.32451856198e-07, 0.0]
    );
    // At the moving lid the horizontal velocity is the lid speed.
    assert_eq!(file.rows[63], [0.1, 0.1, 4.24140398146e-18, 0.0]);
    assert!(file.rows.iter().all(|row| row[3] == 0.0));
}
