mod common;

use cfdcraft_backend::{CompileFailure, CompileOptions};
use cfdcraft_backend_codesaturne::{
    markers::{PROBES_PRESSURE_FILE, PROBES_VELOCITY_FILE, RESIDUALS_FILE},
    mesh::gmsh_text,
    probes::{CENTRELINE_HORIZONTAL, CENTRELINE_VERTICAL, probe_set},
    setup::MESH_FILE,
};
use cfdcraft_model::{ResolvedShape, ResolvedStudy, Study, diagnose, has_errors};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice, build};
use common::{
    CAVITY_V1, Msh, backend, cavity, coarse, compiled, options, read, resolved, setup_root,
};

fn checked_mesh(study: &ResolvedStudy) -> Msh {
    let msh = read(&gmsh_text(study).unwrap());
    // The groups are numbered one after the other from 1, the cells last.
    let ids: Vec<u32> = msh.names.keys().copied().collect();
    let want: Vec<u32> = (1..=u32::try_from(ids.len()).unwrap()).collect();
    assert_eq!(ids, want);
    assert_eq!(msh.names[&ids[ids.len() - 1]], (3, "fluid".to_owned()));
    assert_eq!(msh.names[&ids[ids.len() - 2]], (2, "z_symmetry".to_owned()));
    // Every hexahedron is positive: the area of its lower quad in the x-y plane is above zero.
    let mut hexahedra = 0;
    for (kind, _, nodes) in &msh.elements {
        if *kind != 5 {
            continue;
        }
        hexahedra += 1;
        let p: Vec<[f64; 3]> = nodes[..4].iter().map(|n| msh.node(*n)).collect();
        let area: f64 = (0..4)
            .map(|i| p[i][0] * p[(i + 1) % 4][1] - p[(i + 1) % 4][0] * p[i][1])
            .sum::<f64>()
            / 2.0;
        assert!(area > 0.0, "a hexahedron has area {area}");
    }
    assert!(hexahedra > 0);
    msh
}

#[test]
fn the_step_and_the_cylinder_meshes_extrude_with_their_boundary_groups() {
    // The step: the four sides and the start-up section, no obstacle.
    let step = coarse("backward-step");
    let msh = checked_mesh(&step);
    for name in ["x_min", "x_max", "y_min", "y_max", "startup"] {
        assert!(!msh.quads_of(name).is_empty(), "{name}");
    }
    assert!(msh.group_id("obstacle").is_none());
    let ids: Vec<&str> = msh.names.values().map(|(_, n)| n.as_str()).collect();
    assert_eq!(
        ids,
        [
            "x_min",
            "x_max",
            "y_min",
            "y_max",
            "startup",
            "z_symmetry",
            "fluid"
        ]
    );
    // The inlet group lies in the plane x = 0 and the outlet group at x = the length.
    for node in msh.quads_of("x_min").iter().flat_map(|q| q.iter()) {
        assert!(msh.node(*node)[0].abs() <= 1e-12);
    }
    for node in msh.quads_of("x_max").iter().flat_map(|q| q.iter()) {
        assert!((msh.node(*node)[0] - step.domain.length_x).abs() <= 1e-9);
    }

    // A step with no start-up section has no such group.
    let mut plain = step.clone();
    if let Some(ResolvedShape::BackwardStep { startup_length, .. }) = plain.domain.shape.as_mut() {
        *startup_length = 0.0;
    }
    let plain_mesh = checked_mesh(&plain);
    assert!(plain_mesh.group_id("startup").is_none());

    // The cylinder: the obstacle group, whose boundary quads lie on the circle.
    let cylinder = coarse("cylinder");
    let msh = checked_mesh(&cylinder);
    let Some(ResolvedShape::CylinderChannel {
        diameter,
        centre_x,
        centre_y,
    }) = cylinder.domain.shape
    else {
        panic!("the cylinder template has a cylinder");
    };
    let quads = msh.quads_of("obstacle");
    assert!(!quads.is_empty());
    for node in quads.iter().flat_map(|q| q.iter()) {
        let [x, y, _] = msh.node(*node);
        let radius = ((x - centre_x).powi(2) + (y - centre_y).powi(2)).sqrt();
        assert!(
            (radius - diameter / 2.0).abs() <= 1e-9 * diameter,
            "{radius}"
        );
    }
    assert!(msh.group_id("startup").is_none());
}

#[test]
fn probe_lines_cover_the_benchmark_centre_lines() {
    // A box has the two centre lines with a point at every node of the mesh along them.
    let study = cavity(4);
    let set = probe_set(&study).unwrap();
    let (cx, cy) = (study.domain.length_x / 2.0, study.domain.length_y / 2.0);
    let vertical = set.points_of(CENTRELINE_VERTICAL).unwrap();
    let horizontal = set.points_of(CENTRELINE_HORIZONTAL).unwrap();
    assert_eq!(vertical.len(), 5);
    assert_eq!(horizontal.len(), 5);
    for window in vertical.windows(2) {
        assert!(window[1].y > window[0].y);
    }
    for p in vertical {
        assert!((p.x - cx).abs() <= 1e-12);
    }
    for p in horizontal {
        assert!((p.y - cy).abs() <= 1e-12);
    }
    assert!(vertical[0].y.abs() <= 1e-12);
    assert!((vertical[4].y - study.domain.length_y).abs() <= 1e-12);
    for p in &set.points {
        assert!(p.x.is_finite() && p.y.is_finite());
        assert!((p.z - study.domain.length_z / 2.0).abs() <= 1e-12);
    }
    // The five default points follow the two lines.
    assert_eq!(set.runs.len(), 2 + 5);
    assert_eq!(set.runs[2].name, "probe_1");

    // The setup holds one entry per point, at the point.
    let root = setup_root(&compiled(&study));
    let output = root.find(&["analysis_control", "output"]).unwrap();
    let entries: Vec<_> = output.children_named("probe").collect();
    assert_eq!(entries.len(), set.points.len());
    for (entry, point) in entries.iter().zip(&set.points) {
        let at = |tag: &str| entry.child(tag).unwrap().text.parse::<f64>().unwrap();
        assert!((at("probe_x") - point.x).abs() <= 1e-12);
        assert!((at("probe_y") - point.y).abs() <= 1e-12);
        assert!((at("probe_z") - point.z).abs() <= 1e-12);
    }

    // The step is read along its own lines, with their own sample counts and no centre lines.
    let step = coarse("backward-step");
    let set = probe_set(&step).unwrap();
    assert!(set.run(CENTRELINE_VERTICAL).is_none());
    let plan = step.monitor_plan();
    assert!(!plan.lines.is_empty());
    for line in &plan.lines {
        let run = set.run(&line.fo).unwrap();
        assert_eq!(run.count as u64, line.samples, "{}", line.fo);
        let points = set.points_of(&line.fo).unwrap();
        let first = points[0];
        let last = points[points.len() - 1];
        assert!((first.x - line.x0).abs() <= 1e-12 && (first.y - line.y0).abs() <= 1e-12);
        assert!((last.x - line.x1).abs() <= 1e-12 && (last.y - line.y1).abs() <= 1e-12);
    }

    // What the solver cannot record is refused by name, never dropped.
    let cylinder = coarse("cylinder");
    assert!(!cylinder.monitors.forces.is_empty());
    assert!(matches!(
        backend().compile(&cylinder, &CompileOptions::default()),
        Err(CompileFailure::NotSupported { .. })
    ));
}

#[test]
fn the_parabolic_inlet_is_a_named_diagnostic_and_the_flat_inlet_compiles() {
    let caps = backend().capabilities();
    assert!(!caps.parabolic_inlet);
    for id in ["backward-step-laminar", "cylinder"] {
        let study = build(id, &TemplateOptions::default()).unwrap();
        assert!(
            diagnose(&study, &caps)
                .iter()
                .any(|d| d.code == "CFD-BACKEND-012"),
            "{id}"
        );
    }
    // A writer that is asked anyway refuses at the inlet.
    let laminar = resolved("backward-step-laminar", &options(MeshPreset::Coarse));
    match backend().compile(&laminar, &CompileOptions::default()) {
        Err(CompileFailure::NotSupported { setting, .. }) => assert_eq!(setting, "inlet profile"),
        other => panic!("{other:?}"),
    }

    // The flat inlet of the turbulent step and the cavity compile, scan clean and read back.
    let written = [
        (
            build("backward-step", &options(MeshPreset::Coarse)).unwrap(),
            coarse("backward-step"),
        ),
        (Study::from_json(CAVITY_V1).unwrap(), cavity(4)),
    ];
    for (study, resolved) in &written {
        let found = diagnose(study, &caps);
        assert!(!has_errors(&found), "{found:?}");
        let tree = compiled(resolved);
        assert!(backend().scan(&tree).is_empty());
        let back = backend().read_back(resolved, &tree);
        assert!(back.is_empty(), "{back:?}");
    }
}

#[test]
fn the_run_plan_names_the_final_files() {
    let steady = backend().run_plan(&cavity(2), 1);
    assert_eq!(
        steady.final_fields,
        [RESIDUALS_FILE, PROBES_VELOCITY_FILE, PROBES_PRESSURE_FILE]
    );
    assert!(!steady.transient);
    let transient = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..options(MeshPreset::Coarse)
    };
    let plan = backend().run_plan(&resolved("lid-cavity", &transient), 1);
    assert!(plan.transient);
    assert!(plan.final_fields.contains(&PROBES_VELOCITY_FILE.to_owned()));
}

#[test]
fn the_mesh_file_stands_for_the_mesh() {
    let study = cavity(2);
    assert_eq!(backend().mesh_files(&study), [MESH_FILE]);
    // The file is in the case the writer makes, under the same name.
    let tree = compiled(&study);
    assert!(
        tree.files
            .iter()
            .any(|file| file.rel_path.to_str() == Some(MESH_FILE))
    );
}
