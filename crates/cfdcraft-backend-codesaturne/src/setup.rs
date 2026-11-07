//! The setup file of a case, written as a string from the resolved study.
//!
//! The file holds constants only. The solver turns a formula typed into its setup into compiled
//! code, so no formula, user function or script is ever written: every value below is a number
//! or a name taken from the study. The nodes themselves come from three modules: the boundary
//! zones, the model, time and numerics nodes, and the monitoring points.

use crate::{boundaries, mesh::layout_of, models, probes};
use cfdcraft_backend::CompileFailure;
use cfdcraft_model::ResolvedStudy;

/// The setup file, from the case root.
pub const SETUP_FILE: &str = "DATA/setup.xml";

/// The mesh file, from the case root.
pub const MESH_FILE: &str = "MESH/mesh.msh";

/// The folder the setup searches for the mesh, from the case root.
pub const MESH_DIR: &str = "MESH";

/// The mesh file name inside that folder.
pub const MESH_NAME: &str = "mesh.msh";

/// The group of the two faces along z, which carry a symmetry condition.
pub const Z_FACES_GROUP: &str = "z_symmetry";

/// The root element of a setup.
pub const ROOT: &str = "Code_Saturne_GUI";

/// The version attribute of the root element.
pub const FORMAT_VERSION: &str = "2.0";

/// A setting the writer does not offer, by name.
pub(crate) fn unsupported(setting: &str, reason: &str) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.to_owned(),
    }
}

/// A study the writer cannot express.
pub(crate) fn invalid(reason: &str) -> CompileFailure {
    CompileFailure::Invalid {
        reason: reason.to_owned(),
    }
}

/// A number in the shortest form that reads back as the same number. A negative zero is written
/// as zero, and a number that is not finite is refused.
pub fn number(value: f64) -> Result<String, CompileFailure> {
    if !value.is_finite() {
        return Err(invalid(
            "a value written into the case is not a finite number",
        ));
    }
    Ok((if value == 0.0 { 0.0 } else { value }).to_string())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// A string builder that indents two spaces per level and escapes every text and attribute.
pub(crate) struct Writer {
    out: String,
    depth: usize,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Self {
            out: String::new(),
            depth: 0,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }

    pub(crate) fn line(&mut self, text: &str) {
        self.out.push_str(&"  ".repeat(self.depth));
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn tag(name: &str, attributes: &[(&str, &str)]) -> String {
        let mut tag = name.to_owned();
        for (key, value) in attributes {
            tag.push(' ');
            tag.push_str(key);
            tag.push_str("=\"");
            tag.push_str(&escape(value));
            tag.push('"');
        }
        tag
    }

    pub(crate) fn open(&mut self, name: &str, attributes: &[(&str, &str)]) {
        self.line(&format!("<{}>", Self::tag(name, attributes)));
        self.depth += 1;
    }

    pub(crate) fn close(&mut self, name: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.line(&format!("</{name}>"));
    }

    pub(crate) fn empty(&mut self, name: &str, attributes: &[(&str, &str)]) {
        self.line(&format!("<{}/>", Self::tag(name, attributes)));
    }

    pub(crate) fn leaf(&mut self, name: &str, attributes: &[(&str, &str)], text: &str) {
        self.line(&format!(
            "<{}>{}</{name}>",
            Self::tag(name, attributes),
            escape(text)
        ));
    }
}

/// The setup file of a case on a box or a shaped channel, steady or in time, laminar or with the
/// k-omega SST model, with monitoring points.
///
/// The fluid has density 1, so the dynamic viscosity the solver takes equals the kinematic
/// viscosity of the study.
pub fn setup_xml(study: &ResolvedStudy) -> Result<String, CompileFailure> {
    let layout = layout_of(study)?;
    let zones = boundaries::zones(study, &layout)?;
    let run = models::run_of(study)?;
    let points = probes::probe_set(study)?;
    let frequency = probes::recording_frequency(study, &run)?;
    let viscosity = study.fluid.kinematic_viscosity;
    if viscosity <= 0.0 {
        return Err(invalid("the viscosity must be above zero"));
    }
    let viscosity = number(viscosity)?;
    let mut w = Writer::new();
    w.line("<?xml version=\"1.0\" encoding=\"utf-8\"?>");
    w.open(
        ROOT,
        &[
            ("case", "CASE"),
            ("solver_version", ""),
            ("study", "STUDY"),
            ("version", FORMAT_VERSION),
        ],
    );

    w.open("solution_domain", &[]);
    w.open("meshes_list", &[]);
    w.empty("meshdir", &[("name", MESH_DIR)]);
    w.empty("mesh", &[("name", MESH_NAME)]);
    w.close("meshes_list");
    w.close("solution_domain");

    w.open("analysis_control", &[]);
    models::write_time(&mut w, &run)?;
    probes::write_output(&mut w, &points, frequency)?;
    w.close("analysis_control");

    w.empty("calculation_management", &[]);

    w.open("thermophysical_models", &[]);
    probes::write_recorded_variables(&mut w);
    models::write_turbulence(&mut w, study)?;
    w.close("thermophysical_models");

    w.empty("additional_scalars", &[]);

    w.open("physical_properties", &[]);
    w.open("fluid_properties", &[]);
    for (name, label, value) in [
        ("density", "Density", "1"),
        ("molecular_viscosity", "LamVisc", viscosity.as_str()),
    ] {
        w.open(
            "property",
            &[("choice", "constant"), ("label", label), ("name", name)],
        );
        w.leaf("initial_value", &[], value);
        w.close("property");
    }
    w.close("fluid_properties");
    w.close("physical_properties");

    boundaries::write_zones(&mut w, &zones)?;

    w.open("numerical_parameters", &[]);
    models::write_algorithm(&mut w, &run);
    w.close("numerical_parameters");

    w.close(ROOT);
    Ok(w.finish())
}
