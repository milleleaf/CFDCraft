//! Every setting of the resolved study moves exactly the changes that name it. Adding a field to
//! a recipe without a change for it fails the table or the equality test here.

use cfdcraft_keys::{
    Axis, Change, Residual, StageRecipes, ToolIdentity, ToolPart, diff_mesh, diff_result,
};
use cfdcraft_model::{BoundaryKind, ResolvedStudy, Study, resolve};

fn cavity() -> ResolvedStudy {
    let study = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap();
    resolve(&study).unwrap()
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes(resolved: &ResolvedStudy) -> StageRecipes {
    StageRecipes::from_resolved(resolved, &tool())
}

struct Row {
    name: &'static str,
    edit: fn(&mut ResolvedStudy),
    mesh: Vec<Change>,
    result: Vec<Change>,
}

fn row(
    name: &'static str,
    edit: fn(&mut ResolvedStudy),
    mesh: Vec<Change>,
    result: Vec<Change>,
) -> Row {
    Row {
        name,
        edit,
        mesh,
        result,
    }
}

type Case = (ToolPart, fn(&mut ToolIdentity), &'static str, &'static str);

fn text(s: &str) -> String {
    s.to_owned()
}

fn velocity(boundary: &str, from: Option<f64>, to: Option<f64>) -> Change {
    Change::BoundaryVelocity {
        boundary: text(boundary),
        from,
        to,
    }
}

fn rows(o: &ResolvedStudy) -> Vec<Row> {
    let lid = o.boundary_conditions["lid"].velocity.unwrap();
    let doubled = |from: f64| (from, from * 2.0);
    let (lx, ly, lz) = (o.domain.length_x, o.domain.length_y, o.domain.length_z);
    let length = |axis, from: f64| Change::Length {
        axis,
        from,
        to: from * 2.0,
    };
    let cells = |axis, from: u64| Change::Cells {
        axis,
        from,
        to: from + 1,
    };
    let moved = vec![Change::MeshChanged];
    let (nu_from, nu_to) = doubled(o.fluid.kinematic_viscosity);
    let (end_from, end_to) = doubled(o.solver.end_time);
    let (write_from, write_to) = doubled(o.solver.write_interval);
    let (p_from, p_to) = doubled(o.solver.convergence.p);
    let (u_from, u_to) = doubled(o.solver.convergence.u);
    let samples = o.monitors.centreline_u_sample_count;
    vec![
        row(
            "length x",
            |r| r.domain.length_x *= 2.0,
            vec![length(Axis::X, lx)],
            moved.clone(),
        ),
        row(
            "length y",
            |r| r.domain.length_y *= 2.0,
            vec![length(Axis::Y, ly)],
            moved.clone(),
        ),
        row(
            "length z",
            |r| r.domain.length_z *= 2.0,
            vec![length(Axis::Z, lz)],
            moved.clone(),
        ),
        row(
            "domain open",
            |r| r.domain.open = !r.domain.open,
            vec![Change::DomainOpen {
                from: o.domain.open,
                to: !o.domain.open,
            }],
            moved.clone(),
        ),
        row(
            "cells x",
            |r| r.mesh.cells_x += 1,
            vec![cells(Axis::X, o.mesh.cells_x)],
            moved.clone(),
        ),
        row(
            "cells y",
            |r| r.mesh.cells_y += 1,
            vec![cells(Axis::Y, o.mesh.cells_y)],
            moved.clone(),
        ),
        row(
            "cells z",
            |r| r.mesh.cells_z += 1,
            vec![cells(Axis::Z, o.mesh.cells_z)],
            moved.clone(),
        ),
        row(
            "walls kind",
            |r| r.boundary_conditions.get_mut("walls").unwrap().kind = BoundaryKind::SlipWall,
            vec![Change::Layout],
            vec![
                Change::MeshChanged,
                Change::BoundaryKind {
                    boundary: text("walls"),
                    from: text("NoSlipWall"),
                    to: text("SlipWall"),
                },
            ],
        ),
        row(
            "walls faces",
            |r| {
                r.boundary_conditions.get_mut("walls").unwrap().faces.pop();
            },
            vec![Change::Layout],
            vec![
                Change::MeshChanged,
                Change::BoundaryFaces {
                    boundary: text("walls"),
                    from: text("XMin, XMax, YMin"),
                    to: text("XMin, XMax"),
                },
            ],
        ),
        row(
            "lid kind",
            |r| r.boundary_conditions.get_mut("lid").unwrap().kind = BoundaryKind::NoSlipWall,
            vec![Change::Layout],
            vec![
                Change::MeshChanged,
                Change::BoundaryKind {
                    boundary: text("lid"),
                    from: text("MovingWall"),
                    to: text("NoSlipWall"),
                },
            ],
        ),
        row(
            "lid faces",
            |r| r.boundary_conditions.get_mut("lid").unwrap().faces.clear(),
            vec![Change::Layout],
            vec![
                Change::MeshChanged,
                Change::BoundaryFaces {
                    boundary: text("lid"),
                    from: text("YMax"),
                    to: text(""),
                },
            ],
        ),
        row(
            "a boundary added",
            |r| {
                let lid = r.boundary_conditions["lid"].clone();
                r.boundary_conditions.insert(text("extra"), lid);
            },
            vec![Change::Layout],
            vec![
                Change::MeshChanged,
                Change::BoundaryAdded {
                    boundary: text("extra"),
                },
            ],
        ),
        row(
            "a boundary renamed",
            |r| {
                let lid = r.boundary_conditions.remove("lid").unwrap();
                r.boundary_conditions.insert(text("top"), lid);
            },
            vec![],
            vec![
                Change::BoundaryRemoved {
                    boundary: text("lid"),
                },
                Change::BoundaryAdded {
                    boundary: text("top"),
                },
            ],
        ),
        row(
            "viscosity",
            |r| r.fluid.kinematic_viscosity *= 2.0,
            vec![],
            vec![Change::Viscosity {
                from: nu_from,
                to: nu_to,
            }],
        ),
        row(
            "lid velocity",
            |r| {
                let lid = r.boundary_conditions.get_mut("lid").unwrap();
                lid.velocity = lid.velocity.map(|v| v * 2.0);
            },
            vec![],
            vec![velocity("lid", Some(lid), Some(lid * 2.0))],
        ),
        row(
            "lid velocity removed",
            |r| r.boundary_conditions.get_mut("lid").unwrap().velocity = None,
            vec![],
            vec![velocity("lid", Some(lid), None)],
        ),
        row(
            "walls velocity added",
            |r| r.boundary_conditions.get_mut("walls").unwrap().velocity = Some(0.5),
            vec![],
            vec![velocity("walls", None, Some(0.5))],
        ),
        row(
            "end time",
            |r| r.solver.end_time *= 2.0,
            vec![],
            vec![Change::EndTime {
                from: end_from,
                to: end_to,
            }],
        ),
        row(
            "write interval",
            |r| r.solver.write_interval *= 2.0,
            vec![],
            vec![Change::WriteInterval {
                from: write_from,
                to: write_to,
            }],
        ),
        row(
            "convergence p",
            |r| r.solver.convergence.p *= 2.0,
            vec![],
            vec![Change::Convergence {
                which: Residual::Pressure,
                from: p_from,
                to: p_to,
            }],
        ),
        row(
            "convergence u",
            |r| r.solver.convergence.u *= 2.0,
            vec![],
            vec![Change::Convergence {
                which: Residual::Velocity,
                from: u_from,
                to: u_to,
            }],
        ),
        row(
            "monitor samples",
            |r| r.monitors.centreline_u_sample_count += 1,
            vec![],
            vec![Change::CentrelineSamples {
                from: samples,
                to: samples + 1,
            }],
        ),
    ]
}

fn edited(original: &ResolvedStudy, row: &Row) -> StageRecipes {
    let mut changed = original.clone();
    (row.edit)(&mut changed);
    recipes(&changed)
}

#[test]
fn every_setting_gives_the_change_that_names_it() {
    let original = cavity();
    let old = recipes(&original);
    for row in rows(&original) {
        let new = edited(&original, &row);
        assert_eq!(
            diff_mesh(&old.mesh, &new.mesh),
            row.mesh,
            "{}: the mesh changes",
            row.name
        );
        assert_eq!(
            diff_result(&old.result, &new.result),
            row.result,
            "{}: the result changes",
            row.name
        );
    }
}

#[test]
fn the_diff_is_empty_exactly_when_the_recipes_are_equal() {
    let original = cavity();
    let old = recipes(&original);
    assert!(diff_mesh(&old.mesh, &old.mesh).is_empty());
    assert!(diff_result(&old.result, &old.result).is_empty());
    for row in rows(&original) {
        let new = edited(&original, &row);
        assert_eq!(
            diff_mesh(&old.mesh, &new.mesh).is_empty(),
            old.mesh == new.mesh,
            "{}: the mesh",
            row.name
        );
        assert_eq!(
            diff_result(&old.result, &new.result).is_empty(),
            old.result == new.result,
            "{}: the result",
            row.name
        );
    }
}

#[test]
fn a_velocity_that_appears_is_a_change() {
    let original = cavity();
    let mut changed = original.clone();
    changed
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .velocity = Some(0.5);
    let (old, new) = (recipes(&original), recipes(&changed));
    assert_eq!(
        diff_result(&old.result, &new.result),
        vec![velocity("walls", None, Some(0.5))]
    );
    assert!(diff_mesh(&old.mesh, &new.mesh).is_empty());
}

#[test]
fn several_changes_come_in_a_fixed_order() {
    let original = cavity();
    let lid = original.boundary_conditions["lid"].velocity.unwrap();
    let mut changed = original.clone();
    changed.boundary_conditions.get_mut("lid").unwrap().velocity = Some(lid * 2.0);
    changed.solver.end_time *= 2.0;
    changed.fluid.kinematic_viscosity *= 2.0;
    let (old, new) = (recipes(&original), recipes(&changed));
    let expected = vec![
        Change::Viscosity {
            from: original.fluid.kinematic_viscosity,
            to: changed.fluid.kinematic_viscosity,
        },
        velocity("lid", Some(lid), Some(lid * 2.0)),
        Change::EndTime {
            from: original.solver.end_time,
            to: changed.solver.end_time,
        },
    ];
    assert_eq!(diff_result(&old.result, &new.result), expected);
    assert_eq!(diff_result(&old.result, &new.result), expected);
}

#[test]
fn every_row_moves_a_key_exactly_when_it_has_a_change() {
    let original = cavity();
    let old = recipes(&original);
    for row in rows(&original) {
        let new = edited(&original, &row);
        assert_eq!(
            old.mesh.key() != new.mesh.key(),
            !diff_mesh(&old.mesh, &new.mesh).is_empty(),
            "{}: the mesh key",
            row.name
        );
        assert_eq!(
            old.result.key() != new.result.key(),
            !diff_result(&old.result, &new.result).is_empty(),
            "{}: the result key",
            row.name
        );
    }
}

#[test]
fn every_tool_part_gives_its_own_change() {
    let original = cavity();
    let old = recipes(&original);
    let cases: [Case; 4] = [
        (
            ToolPart::Image,
            |t| t.image_digest = text("sha256:bb"),
            "sha256:aa",
            "sha256:bb",
        ),
        (
            ToolPart::Dialect,
            |t| t.dialect = text("esi-v2606"),
            "esi-v2512",
            "esi-v2606",
        ),
        (
            ToolPart::Sidecar,
            |t| t.sidecar_lock = Some(text("lock-1")),
            "none",
            "lock-1",
        ),
        (
            ToolPart::Writer,
            |t| t.writer_version = text("0.2.0"),
            "0.1.0",
            "0.2.0",
        ),
    ];
    for (part, edit, from, to) in cases {
        let mut other = tool();
        edit(&mut other);
        let new = StageRecipes::from_resolved(&original, &other);
        let expected = vec![Change::Tool {
            part,
            from: text(from),
            to: text(to),
        }];
        assert_eq!(diff_mesh(&old.mesh, &new.mesh), expected, "{part:?}: mesh");
        // A tool change also changes the mesh key, so the result reports its mesh as changed too.
        let mut with_mesh = expected.clone();
        with_mesh.push(Change::MeshChanged);
        assert_eq!(
            diff_result(&old.result, &new.result),
            with_mesh,
            "{part:?}: result"
        );
    }
}
