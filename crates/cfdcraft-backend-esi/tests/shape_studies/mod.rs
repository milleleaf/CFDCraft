//! Studies for the box, the backward step and the cylinder channel, as JSON text. Every study goes
//! through `resolve`, so a combination the model refuses fails the test that builds it.
#![allow(dead_code)]

use cfdcraft_model::{ResolvedStudy, Study, resolve};

/// A study as JSON text. `shape` is either empty or starts with a comma and holds the `shape`
/// member of the domain; `mesh_extra` is empty or starts with a comma and holds more mesh members.
pub struct Text<'a> {
    pub domain: [f64; 3],
    pub open: bool,
    pub shape: &'a str,
    pub boundaries: &'a str,
    pub cells: [u64; 2],
    pub mesh_extra: &'a str,
    pub nu: f64,
}

impl Text<'_> {
    pub fn json(&self) -> String {
        let [x, y, z] = self.domain;
        let [cx, cy] = self.cells;
        let (open, shape, boundaries, mesh_extra, nu) = (
            self.open,
            self.shape,
            self.boundaries,
            self.mesh_extra,
            self.nu,
        );
        format!(
            r#"{{"schema_version":1,"domain":{{"length_x":{{"value":{x},"unit":"m"}},"length_y":{{"value":{y},"unit":"m"}},"length_z":{{"value":{z},"unit":"m"}},"open":{open}{shape}}},"fluid":{{"kinematic_viscosity":{{"value":{nu},"unit":"m2/s"}}}},"boundary_conditions":{{{boundaries}}},"mesh":{{"cells_x":{cx},"cells_y":{cy},"cells_z":1{mesh_extra}}},"solver":{{"solver":"SimpleLaminar","end_time":{{"value":100,"unit":"s"}},"write_interval":{{"value":100,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64}}}}"#
        )
    }
}

/// The lid-driven box with a given cell count along both axes.
pub fn box_json(cells: u64, mesh_extra: &str) -> String {
    Text {
        domain: [0.1, 0.1, 0.05],
        open: false,
        shape: "",
        boundaries: r#""lid":{"faces":["YMax"],"kind":"MovingWall","velocity":{"value":0.1,"unit":"m/s"}},"walls":{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}"#,
        cells: [cells, cells],
        mesh_extra,
        nu: 0.0001,
    }
    .json()
}

pub fn graded_json(level: u32) -> String {
    box_json(
        40,
        &format!(r#","level":{level},"grading":{{"x":1.05,"y":1.05}}"#),
    )
}

/// The backward step, with or without the start-up section, whose floor boundary has this label.
pub fn step_json(startup: bool, level: u32, floor_label: &str) -> String {
    let (startup_length, cells_x) = if startup { (0.254, 220) } else { (0.0, 218) };
    let shape = format!(
        r#","shape":{{"kind":"backward_step","step_height":{{"value":12.7,"unit":"mm"}},"upstream_length":{{"value":1.651,"unit":"m"}},"startup_length":{{"value":{startup_length},"unit":"m"}}}}"#
    );
    let boundaries = format!(
        r#""inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":44.2,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"{floor_label}":{{"faces":["YMin"],"kind":"NoSlipWall"}},"top_wall":{{"faces":["YMax"],"kind":"NoSlipWall"}}"#
    );
    Text {
        domain: [2.286, 0.1143, 0.0127],
        open: true,
        shape: &shape,
        boundaries: &boundaries,
        cells: [cells_x, 114],
        mesh_extra: &format!(r#","level":{level}"#),
        nu: 0.0000156,
    }
    .json()
}

/// The cylinder channel, whose cylinder boundary has this label.
pub fn cylinder_json(level: u32, obstacle_label: &str) -> String {
    let shape = r#","shape":{"kind":"cylinder_channel","diameter":{"value":0.1,"unit":"m"},"centre_x":{"value":0.2,"unit":"m"},"centre_y":{"value":0.2,"unit":"m"}}"#;
    let boundaries = format!(
        r#""inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":1.0,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"channel_walls":{{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"{obstacle_label}":{{"faces":["Obstacle"],"kind":"NoSlipWall"}}"#
    );
    Text {
        domain: [2.2, 0.41, 0.1],
        open: true,
        shape,
        boundaries: &boundaries,
        cells: [114, 44],
        mesh_extra: &format!(r#","level":{level}"#),
        nu: 0.001,
    }
    .json()
}

pub fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}
