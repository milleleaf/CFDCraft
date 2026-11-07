//! A changed monitor is one change, named by its place in its list, with both sides in words.

use cfdcraft_keys::{Change, StageRecipes, ToolIdentity, diff_mesh, diff_result};
use cfdcraft_model::{
    MonitorField, ResolvedLine, ResolvedProbe, ResolvedStudy, ResolvedSurface, Study,
    SurfaceQuantity, resolve,
};

fn cavity() -> ResolvedStudy {
    let study = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap();
    resolve(&study).unwrap()
}

fn recipes(resolved: &ResolvedStudy) -> StageRecipes {
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    StageRecipes::from_resolved(resolved, &tool)
}

fn probe(label: &str, x: f64, y: f64) -> ResolvedProbe {
    ResolvedProbe {
        label: label.to_owned(),
        x,
        y,
        field: MonitorField::Velocity,
        boundary: None,
    }
}

/// The five default positions of the cavity, written out as listed probes.
fn defaults_listed() -> ResolvedStudy {
    let mut resolved = cavity();
    let (lx, ly) = (resolved.domain.length_x, resolved.domain.length_y);
    let places = [
        (0.5, 0.5),
        (0.25, 0.25),
        (0.75, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
    ];
    resolved.monitors.probes = places
        .iter()
        .zip(["a", "b", "c", "d", "e"])
        .map(|((fx, fy), label)| probe(label, fx * lx, fy * ly))
        .collect();
    resolved
}

fn monitor(id: &str, from: Option<&str>, to: Option<&str>) -> Change {
    Change::Monitor {
        id: id.to_owned(),
        from: from.map(str::to_owned),
        to: to.map(str::to_owned),
    }
}

#[test]
fn adding_a_probe_is_one_monitor_change() {
    let mut six = defaults_listed();
    six.monitors.probes.push(probe("f", 0.05, 0.05));
    let (old, new) = (recipes(&defaults_listed()), recipes(&six));
    assert_eq!(
        diff_result(&old.result, &new.result),
        vec![monitor(
            "probe_6",
            None,
            Some("x 0.05 m, y 0.05 m, velocity")
        )]
    );
}

#[test]
fn removing_and_moving_are_reported_by_position() {
    let original = defaults_listed();
    let mut moved = original.clone();
    moved.monitors.probes[1].x = 0.03;
    let (old, new) = (recipes(&original), recipes(&moved));
    assert_eq!(
        diff_result(&old.result, &new.result),
        vec![monitor(
            "probe_2",
            Some("x 0.025 m, y 0.025 m, velocity"),
            Some("x 0.03 m, y 0.025 m, velocity")
        )]
    );
    let mut fewer = original.clone();
    fewer.monitors.probes.pop();
    let new = recipes(&fewer);
    // Written from the same product the plan computes, so no decimal is assumed.
    let (x, y) = (
        0.75 * original.domain.length_x,
        0.75 * original.domain.length_y,
    );
    let last = format!("x {x} m, y {y} m, velocity");
    assert_eq!(
        diff_result(&old.result, &new.result),
        vec![monitor("probe_5", Some(&last), None)]
    );
}

#[test]
fn the_default_set_and_the_same_set_listed_are_no_change() {
    let (old, new) = (recipes(&cavity()), recipes(&defaults_listed()));
    assert!(diff_result(&old.result, &new.result).is_empty());
    assert_eq!(old.result.key(), new.result.key());
}

#[test]
fn lines_and_surfaces_are_reported_with_their_own_ids() {
    let mut watched = defaults_listed();
    watched.monitors.lines.push(ResolvedLine {
        label: "mid".to_owned(),
        x0: 0.0,
        y0: 0.05,
        x1: 0.1,
        y1: 0.05,
        samples: 64,
        field: MonitorField::Pressure,
    });
    watched.monitors.surfaces.push(ResolvedSurface {
        label: "drag".to_owned(),
        boundary: "lid".to_owned(),
        quantity: SurfaceQuantity::Flux,
    });
    let (old, new) = (recipes(&defaults_listed()), recipes(&watched));
    assert_eq!(
        diff_result(&old.result, &new.result),
        vec![
            monitor(
                "line_1",
                None,
                Some("from (0, 0.05) m to (0.1, 0.05) m, 64 samples, pressure")
            ),
            monitor("surface_1", None, Some("flux on movingWall")),
        ]
    );
}

#[test]
fn monitor_changes_come_after_the_raw_overrides_in_the_list() {
    let original = defaults_listed();
    let mut changed = original.clone();
    changed.raw_overrides.insert(
        "relax-u".to_owned(),
        cfdcraft_model::RawOverride {
            backend: cfdcraft_model::RawBackend::Esi,
            file: "system/fvSolution".to_owned(),
            entry: "relaxationFactors/equations/U".to_owned(),
            value: "0.5".to_owned(),
        },
    );
    changed.monitors.probes[0].x = 0.04;
    changed.solver.end_time *= 2.0;
    let (old, new) = (recipes(&original), recipes(&changed));
    let kinds: Vec<&str> = diff_result(&old.result, &new.result)
        .iter()
        .map(|c| match c {
            Change::EndTime { .. } => "end",
            Change::RawOverride { .. } => "raw",
            Change::Monitor { .. } => "monitor",
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["end", "raw", "monitor"]);
}

#[test]
fn the_mesh_diff_never_reports_a_monitor() {
    let mut six = defaults_listed();
    six.monitors.probes.push(probe("f", 0.05, 0.05));
    let (old, new) = (recipes(&defaults_listed()), recipes(&six));
    assert!(diff_mesh(&old.mesh, &new.mesh).is_empty());
    assert_eq!(old.mesh.key(), new.mesh.key());
}
