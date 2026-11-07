//! The probe, surface and line parsers over files a real solver wrote.
//!
//! Every number below was read from the fixture file itself. The files are whole and unchanged:
//! see `PROVENANCE.md`.

use cfdcraft_monitors::{
    FieldKind, Value, coefficient_series, parse_coefficients, parse_probes, parse_surface,
    parse_xy, probe_series, surface_series, surface_series_component,
};

const PROBES: &str = include_str!("fixtures/v2512-real-probe_4-U.dat");
const SURFACE: &str = include_str!("fixtures/v2512-real-surfaceFieldValue-movingWall.dat");
const LINE: &str = include_str!("fixtures/v2512-real-centreline-vertical_U.xy");
const COEFFICIENTS: &str = include_str!("fixtures/v2512-real-cylinder-coefficient.dat");
const FRONT: &str = include_str!("fixtures/v2512-real-cylinder-probe_1-p.dat");
const REAR: &str = include_str!("fixtures/v2512-real-cylinder-probe_2-p.dat");
const HOT_FLUX: &str = include_str!("fixtures/v2512-real-heated-hot-gradT.dat");
const COLD_FLUX: &str = include_str!("fixtures/v2512-real-heated-cold-gradT.dat");
const HEATED_HORIZONTAL: &str = include_str!("fixtures/v2512-real-heated-line_1-U.xy");
const HEATED_VERTICAL: &str = include_str!("fixtures/v2512-real-heated-line_2-U.xy");
const STEP_FLOOR: &str = include_str!("fixtures/v2512-real-step-line_1-U.xy");
const STEP_X1: &str = include_str!("fixtures/v2512-real-step-line_2-U.xy");
const STEP_X10: &str = include_str!("fixtures/v2512-real-step-line_3-U.xy");

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

#[test]
fn a_real_force_coefficient_file_states_its_directions_and_twelve_columns() {
    // The force monitor of the cylinder, written with `origin (0 0 0)`, `e1 (1 0 0)` and
    // `e3 (0 1 0)`: the header gives the lift along y, with the exponent spelling of the solver.
    assert!(COEFFICIENTS.contains(
        "# liftDir           : (0.000000000000e+00 1.000000000000e+00 0.000000000000e+00)"
    ));
    let file = parse_coefficients(COEFFICIENTS).unwrap();
    assert_eq!(file.lift_dir, Some([0.0, 1.0, 0.0]));
    assert_eq!(file.drag_dir, Some([1.0, 0.0, 0.0]));
    assert_eq!(
        file.columns,
        [
            "Cd", "Cd(f)", "Cd(r)", "Cl", "Cl(f)", "Cl(r)", "CmPitch", "CmRoll", "CmYaw", "Cs",
            "Cs(f)", "Cs(r)"
        ]
    );
    assert_eq!(file.rows.len(), 60);
    assert_eq!(file.rows[0].time, 0.001);
    assert_eq!(file.rows[0].values[0], Value::Scalar(354.0205465873));
    assert_eq!(file.rows[0].values[3], Value::Scalar(-0.4257327367407));
    assert_eq!(file.rows[0].values.len(), 12);
    let last = file.rows.last().unwrap();
    assert_eq!(last.time, 0.06);
    assert_eq!(last.values[0], Value::Scalar(2.245944750352));
    assert_eq!(last.values[3], Value::Scalar(-0.004378517656129));
    assert!(file.rows.windows(2).all(|pair| pair[0].time < pair[1].time));
}

#[test]
fn a_real_force_coefficient_file_gives_a_series_per_column() {
    let file = parse_coefficients(COEFFICIENTS).unwrap();
    let series = coefficient_series(&file, "force_1");
    let names: Vec<&str> = series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "force_1.Cd",
            "force_1.Cd_f",
            "force_1.Cd_r",
            "force_1.Cl",
            "force_1.Cl_f",
            "force_1.Cl_r",
            "force_1.CmPitch",
            "force_1.CmRoll",
            "force_1.CmYaw",
            "force_1.Cs",
            "force_1.Cs_f",
            "force_1.Cs_r"
        ]
    );
    let cl = &series[3];
    assert_eq!(cl.times.len(), 60);
    assert_eq!(cl.values[0], -0.4257327367407);
    assert_eq!(cl.values[59], -0.004378517656129);
}

#[test]
fn a_real_pressure_probe_on_a_patch_names_the_patch_after_its_location() {
    // `patchProbes` writes `# Probe 0 (x y z) at patch <name> with a distance of ...`, then a
    // `# Time` line with the probe index, then one scalar per row.
    assert!(
        FRONT.starts_with("# Probe 0 (0.15 0.2 0.005) at patch obstacle with a distance of 0 m")
    );
    for (text, x, first, last) in [
        (FRONT, 0.15, 2140.21679273, 2.01506466107),
        (REAR, 0.25, 1917.24358296, 0.626459771527),
    ] {
        let file = parse_probes(text).unwrap();
        assert_eq!(file.probes.len(), 1);
        assert_eq!(file.probes[0].index, 0);
        assert_eq!(file.probes[0].location, [x, 0.2, 0.005]);
        assert!(!file.probes[0].not_found);
        assert_eq!(file.rows.len(), 60);
        assert_eq!(file.rows[0].time, 0.001);
        assert_eq!(file.rows[0].values, [Value::Scalar(first)]);
        assert_eq!(file.rows[59].time, 0.06);
        assert_eq!(file.rows[59].values, [Value::Scalar(last)]);
        let series = probe_series(&file, "probe", 0, FieldKind::Pressure);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "probe.p");
        assert_eq!(series[0].values.len(), 60);
        assert_eq!(series[0].values[59], last);
    }
}

#[test]
fn a_real_heat_flux_file_holds_a_vector_of_the_area_integrated_gradient() {
    // `areaIntegrate` of a vector field writes one `(x y z)` per row, in a column named
    // `areaIntegrate(gradT)`. The hot wall is the left one: the temperature falls into the fluid,
    // so the x component is negative on every row.
    for (text, region, first, last) in [
        (
            HOT_FLUX,
            "patch isothermal2",
            -6.692113787078,
            -0.4455807999518,
        ),
        (
            COLD_FLUX,
            "patch isothermal1",
            -6.695750176938,
            -0.4470622718931,
        ),
    ] {
        let file = parse_surface(text).unwrap();
        assert_eq!(file.region, region);
        assert_eq!(file.faces, 40);
        assert_eq!(file.area, 0.1);
        assert_eq!(file.columns, ["areaIntegrate(gradT)"]);
        assert_eq!(file.rows.len(), 60);
        assert_eq!(file.rows[0].time, 1.0);
        assert_eq!(file.rows[59].time, 60.0);
        let Value::Vector(first_row) = file.rows[0].values[0] else {
            panic!("a vector")
        };
        assert_eq!(first_row[0], first);
        assert_eq!(first_row[2], 0.0);
        let Value::Vector(last_row) = file.rows[59].values[0] else {
            panic!("a vector")
        };
        assert_eq!(last_row[0], last);
        assert!(file.rows.iter().all(|row| match row.values[0] {
            Value::Vector(v) => v[0] < 0.0 && v[2] == 0.0,
            Value::Scalar(_) => false,
        }));
        // A scalar series skips the vector column; the component series reads it.
        assert!(surface_series(&file, "wall", &[("areaIntegrate(gradT)", "q")]).is_empty());
        let x = surface_series_component(&file, "wall", 0);
        assert_eq!(x.len(), 1);
        assert_eq!(x[0].name, "wall.areaIntegrate(gradT).x");
        assert_eq!(x[0].values.len(), 60);
        assert_eq!(x[0].values[59], last);
    }
}

#[test]
fn a_real_line_monitor_writes_three_coordinates_then_three_components() {
    // The line monitors the writer makes use `axis xyz`: six columns, the first three the point.
    let horizontal = parse_xy(HEATED_HORIZONTAL).unwrap();
    assert_eq!(horizontal.columns, 6);
    assert_eq!(horizontal.rows.len(), 201);
    assert_eq!(horizontal.rows[0], [0.0, 0.5, 0.05, 0.0, 0.0, 0.0]);
    assert_eq!(
        horizontal.rows[1],
        [0.005, 0.5, 0.05, -9.60810929489e-07, 0.00371759134925, 0.0]
    );
    assert_eq!(horizontal.rows[200], [1.0, 0.5, 0.05, 0.0, 0.0, 0.0]);
    let vertical = parse_xy(HEATED_VERTICAL).unwrap();
    assert_eq!(vertical.columns, 6);
    assert_eq!(vertical.rows.len(), 201);
    assert_eq!(
        vertical.rows[1],
        [0.5, 0.005, 0.05, -0.000363765772507, 1.7498597273e-06, 0.0]
    );

    // The step lines: 1000 samples along the floor, 400 up each profile.
    let floor = parse_xy(STEP_FLOOR).unwrap();
    assert_eq!(floor.columns, 6);
    assert_eq!(floor.rows.len(), 1000);
    assert_eq!(floor.rows[0], [1.651, 0.000127, 0.00635, 0.0, 0.0, 0.0]);
    assert_eq!(floor.rows[999][0], 2.286);
    assert!(floor.rows.windows(2).all(|pair| pair[0][0] < pair[1][0]));
    for text in [STEP_X1, STEP_X10] {
        let profile = parse_xy(text).unwrap();
        assert_eq!(profile.columns, 6);
        assert_eq!(profile.rows.len(), 400);
        assert_eq!(profile.rows[0][1], 0.0);
        assert_eq!(profile.rows[399][1], 0.1143);
        assert!(profile.rows.windows(2).all(|pair| pair[0][1] < pair[1][1]));
        assert!(profile.rows.iter().all(|row| row[0] == profile.rows[0][0]));
    }
    assert_eq!(parse_xy(STEP_X1).unwrap().rows[0][0], 1.6637);
    assert_eq!(parse_xy(STEP_X10).unwrap().rows[0][0], 1.778);
}
