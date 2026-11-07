//! The SU2 configuration of a resolved study, and the whole case it belongs to.
//!
//! Every key written here is on the allowlist of the guard, and every number is formatted by this
//! file, never copied as text from the study. Notes on what the solver accepts, each read in the
//! SU2 8.6.0 sources:
//! - the density-based coupled incompressible solver is chosen explicitly;
//! - the dimensional scheme is chosen explicitly, because the default scheme divides by the start
//!   velocity, which is zero for a study that starts at rest;
//! - Boussinesq buoyancy and a body force are refused together by the configuration check, and the
//!   Boussinesq source takes a fixed gravity, so a study's gravity enters through the thermal
//!   expansion coefficient (the source is proportional to the product);
//! - time marching needs its own count of time steps, whose default is one.

use crate::{
    inlet,
    markers::{
        CFL_STEADY, CFL_TRANSIENT, CONFIG_FILE, CONVERGENCE_FIELD_ENERGY, CONVERGENCE_FIELDS,
        HISTORY_GROUPS_STEADY, HISTORY_GROUPS_TRANSIENT, HISTORY_STEM, INLET_FILE,
        INNER_ITERATIONS, MESH_FILE, NEUTRAL_TEMPERATURE, SCREEN_STEADY, SCREEN_TRANSIENT,
        STANDARD_GRAVITY, VOLUME_STEM,
    },
    mesh::{Marker, invalid, layout_of, markers, mesh_of, mesh_text},
};
use cfdcraft_backend::{CaseFile, CaseTree, CompileFailure};
use cfdcraft_model::{BoundaryKind, InitMode, ResolvedStudy, TurbulenceModel, inlet_turbulence};

/// Settings the study does not parameterise: the numerical scheme. They are covered by the golden
/// case text, not by facts read back.
const NUMERICS: [(&str, &str); 8] = [
    ("NUM_METHOD_GRAD", "GREEN_GAUSS"),
    ("CFL_ADAPT", "NO"),
    ("CONV_NUM_METHOD_FLOW", "FDS"),
    ("MUSCL_FLOW", "YES"),
    ("SLOPE_LIMITER_FLOW", "NONE"),
    ("TIME_DISCRE_FLOW", "EULER_IMPLICIT"),
    ("LINEAR_SOLVER", "FGMRES"),
    ("LINEAR_SOLVER_PREC", "ILU"),
];
/// The tolerance and the iteration limit of the linear solver.
const LINEAR_ERROR: f64 = 1e-6;
const LINEAR_ITERATIONS: u64 = 20;
/// Iterations before the convergence criterion is looked at.
const CONVERGENCE_START: u64 = 10;
/// The time marching scheme of a transient run.
pub const TIME_MARCHING: &str = "DUAL_TIME_STEPPING-2ND_ORDER";

/// A number as the configuration writes it: the shortest text that reads back as the same number,
/// without an exponent, and zero without a sign.
pub fn num(v: f64) -> String {
    if v == 0.0 {
        "0".to_owned()
    } else {
        format!("{v}")
    }
}

fn tuple(items: &[String]) -> String {
    format!("( {} )", items.join(", "))
}

fn finite(v: f64, what: &str) -> Result<f64, CompileFailure> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(invalid(&format!("{what} is not a finite number")))
    }
}

/// A whole number of at least one, from a count held as a number.
fn whole(v: f64, what: &str) -> Result<u64, CompileFailure> {
    if v.is_finite() && v >= 1.0 && v.fract() == 0.0 && v <= 1e12 {
        Ok(v as u64)
    } else {
        Err(invalid(&format!(
            "{what} must be a whole number of at least one"
        )))
    }
}

/// The speed the inflow values of a turbulence model are built from: the reference velocity of the
/// study, else the speed of a velocity inlet, else the speed of a moving wall.
pub fn reference_speed(study: &ResolvedStudy) -> Option<f64> {
    let driven = |kind: BoundaryKind| {
        study
            .boundary_conditions
            .values()
            .filter(|b| b.kind == kind)
            .find_map(|b| b.velocity.filter(|v| *v != 0.0).map(f64::abs))
    };
    study
        .physics
        .as_ref()
        .and_then(|p| p.turbulence.reference_velocity)
        .or_else(|| driven(BoundaryKind::VelocityInlet))
        .or_else(|| driven(BoundaryKind::MovingWall))
}

/// The temperature the solver starts from and an inlet carries: the reference temperature of a
/// buoyant study, else a neutral value that changes nothing while the energy equation is off.
pub fn initial_temperature(study: &ResolvedStudy) -> f64 {
    study
        .physics
        .as_ref()
        .and_then(|p| p.buoyancy)
        .map_or(NEUTRAL_TEMPERATURE, |b| b.reference_temperature)
}

/// The turbulence model of a study.
pub fn model_of(study: &ResolvedStudy) -> TurbulenceModel {
    study
        .physics
        .as_ref()
        .map_or(TurbulenceModel::Laminar, |p| p.turbulence.model)
}

/// The largest component of a velocity vector and the unit direction it points along. An inlet or
/// a moving wall moves along one axis, so the largest component is its speed.
pub fn speed_and_direction(vector: [f64; 3]) -> (f64, [f64; 3]) {
    let speed = vector.iter().fold(0.0_f64, |a, c| a.max(c.abs()));
    if speed == 0.0 {
        return (0.0, [0.0; 3]);
    }
    (
        speed,
        vector.map(|c| if c == 0.0 { 0.0 } else { c / speed }),
    )
}

/// One configuration file under construction: keys in the order they are written.
struct Lines(Vec<(&'static str, String)>);

impl Lines {
    fn put(&mut self, key: &'static str, value: impl Into<String>) {
        self.0.push((key, value.into()));
    }

    fn text(&self) -> String {
        self.0
            .iter()
            .map(|(key, value)| format!("{key}= {value}\n"))
            .collect()
    }
}

/// A study the writer does not offer, as the finding it is.
fn check_supported(study: &ResolvedStudy) -> Result<(), CompileFailure> {
    let not_supported = |setting: &str, reason: &str| CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.to_owned(),
    };
    if !matches!(
        model_of(study),
        TurbulenceModel::Laminar | TurbulenceModel::KOmegaSst
    ) {
        return Err(not_supported(
            "this turbulence model",
            "SU2 offers the laminar and the k-omega SST models",
        ));
    }
    if study
        .physics
        .as_ref()
        .is_some_and(|p| p.init == InitMode::Potential)
    {
        return Err(not_supported(
            "a potential-flow start",
            "SU2 starts from a uniform field",
        ));
    }
    if !study.raw_overrides.is_empty() {
        return Err(not_supported(
            "raw overrides",
            "SU2 has no dictionaries to override",
        ));
    }
    if study.mesh.cells_z != 1 {
        return Err(invalid(
            "an SU2 case is two-dimensional, so the mesh needs exactly one cell through z",
        ));
    }
    Ok(())
}

/// The configuration text of a study whose markers are `markers`.
pub fn config_text(study: &ResolvedStudy, markers: &[Marker]) -> Result<String, CompileFailure> {
    let physics = study.physics.as_ref();
    let model = model_of(study);
    let sst = model == TurbulenceModel::KOmegaSst;
    let buoyancy = physics.and_then(|p| p.buoyancy);
    let step = study.effective_delta_t();
    let nu = finite(study.fluid.kinematic_viscosity, "the viscosity")?;
    let temperature = finite(initial_temperature(study), "the reference temperature")?;
    let mut c = Lines(Vec::new());
    c.put("SOLVER", if sst { "INC_RANS" } else { "INC_NAVIER_STOKES" });
    c.put("KIND_TURB_MODEL", if sst { "SST" } else { "NONE" });
    c.put("MATH_PROBLEM", "DIRECT");
    c.put("RESTART_SOL", "NO");
    c.put("KIND_INCOMP_SYSTEM", "DENSITY_BASED");
    c.put("INC_NONDIM", "DIMENSIONAL");
    c.put(
        "INC_DENSITY_MODEL",
        if buoyancy.is_some() {
            "BOUSSINESQ"
        } else {
            "CONSTANT"
        },
    );
    c.put("INC_DENSITY_INIT", num(1.0));
    // The turbulence inflow values are built from the speed the solver starts at.
    let start = if sst {
        reference_speed(study)
            .ok_or_else(|| invalid("a turbulence model needs a reference speed"))?
    } else {
        0.0
    };
    c.put(
        "INC_VELOCITY_INIT",
        tuple(&[
            num(finite(start, "the reference speed")?),
            num(0.0),
            num(0.0),
        ]),
    );
    c.put("INC_TEMPERATURE_INIT", num(temperature));
    c.put(
        "INC_ENERGY_EQUATION",
        if buoyancy.is_some() { "YES" } else { "NO" },
    );
    c.put("VISCOSITY_MODEL", "CONSTANT_VISCOSITY");
    c.put("MU_CONSTANT", num(nu));
    if let Some(b) = buoyancy {
        c.put(
            "THERMAL_EXPANSION_COEFF",
            num(finite(
                b.gravity * b.beta / STANDARD_GRAVITY,
                "the buoyancy",
            )?),
        );
        c.put("PRANDTL_LAM", num(finite(b.prandtl, "the Prandtl number")?));
        c.put(
            "PRANDTL_TURB",
            num(finite(b.turbulent_prandtl, "the turbulent Prandtl number")?),
        );
    }
    if sst {
        let t = physics.map(|p| &p.turbulence);
        let intensity = t.and_then(|t| t.intensity);
        let ratio = t.and_then(|t| t.viscosity_ratio);
        let (Some(intensity), Some(ratio)) = (intensity, ratio) else {
            return Err(invalid(
                "the turbulence model needs an intensity and a viscosity ratio",
            ));
        };
        c.put(
            "FREESTREAM_TURBULENCEINTENSITY",
            num(finite(intensity, "the turbulence intensity")?),
        );
        c.put(
            "FREESTREAM_TURB2LAMVISCRATIO",
            num(finite(ratio, "the viscosity ratio")?),
        );
    }
    boundary_keys(
        &mut c,
        markers,
        temperature,
        inlet::parabolic_requested(study),
    )?;
    // Numerics and time.
    c.put(
        "CFL_NUMBER",
        num(if step.is_some() {
            CFL_TRANSIENT
        } else {
            CFL_STEADY
        }),
    );
    for (key, value) in NUMERICS {
        c.put(key, value);
    }
    c.put("LINEAR_SOLVER_ERROR", num(LINEAR_ERROR));
    c.put("LINEAR_SOLVER_ITER", LINEAR_ITERATIONS.to_string());
    let write_interval = study.solver.write_interval;
    match step {
        None => {
            c.put(
                "ITER",
                whole(study.solver.end_time, "the iteration count")?.to_string(),
            );
            let mut fields = CONVERGENCE_FIELDS.to_owned();
            if buoyancy.is_some() {
                fields = format!("{fields}, {CONVERGENCE_FIELD_ENERGY}");
            }
            c.put("CONV_FIELD", format!("( {fields} )"));
            let limit = study.solver.convergence.p.min(study.solver.convergence.u);
            if !(limit.is_finite() && limit > 0.0) {
                return Err(invalid("the convergence limits must be above zero"));
            }
            c.put("CONV_RESIDUAL_MINVAL", format!("{:.4}", limit.log10()));
            c.put("CONV_STARTITER", CONVERGENCE_START.to_string());
            c.put(
                "OUTPUT_WRT_FREQ",
                whole(write_interval, "the write interval")?.to_string(),
            );
            c.put("HISTORY_OUTPUT", format!("( {HISTORY_GROUPS_STEADY} )"));
            c.put("SCREEN_OUTPUT", format!("( {SCREEN_STEADY} )"));
        }
        Some(dt) => {
            if !(dt.is_finite() && dt > 0.0) {
                return Err(invalid("the time step must be above zero"));
            }
            let end = study.solver.end_time;
            c.put("TIME_DOMAIN", "YES");
            c.put("TIME_MARCHING", TIME_MARCHING);
            c.put("TIME_STEP", num(dt));
            c.put("MAX_TIME", num(finite(end, "the end time")?));
            c.put(
                "TIME_ITER",
                whole((end / dt).round(), "the number of time steps")?.to_string(),
            );
            c.put("INNER_ITER", INNER_ITERATIONS.to_string());
            c.put(
                "OUTPUT_WRT_FREQ",
                whole((write_interval / dt).round().max(1.0), "the write interval")?.to_string(),
            );
            c.put("HISTORY_OUTPUT", format!("( {HISTORY_GROUPS_TRANSIENT} )"));
            c.put("SCREEN_OUTPUT", format!("( {SCREEN_TRANSIENT} )"));
        }
    }
    c.put("MESH_FILENAME", MESH_FILE);
    c.put("MESH_FORMAT", "SU2");
    c.put("CONV_FILENAME", HISTORY_STEM);
    c.put("TABULAR_FORMAT", "CSV");
    c.put("VOLUME_FILENAME", VOLUME_STEM);
    c.put("OUTPUT_FILES", "( RESTART_ASCII, CSV )");
    c.put("OUTPUT_PRECISION", "10");
    Ok(c.text())
}

/// The keys that name markers: inlet, outlet, walls and symmetry planes.
fn boundary_keys(
    c: &mut Lines,
    markers: &[Marker],
    temperature: f64,
    profile: bool,
) -> Result<(), CompileFailure> {
    fn named(markers: &[Marker], pick: impl Fn(&Marker) -> Option<String>) -> Vec<String> {
        markers.iter().filter_map(pick).collect()
    }
    for inlet in markers
        .iter()
        .filter(|m| m.kind == BoundaryKind::VelocityInlet)
    {
        let (speed, d) = speed_and_direction(inlet.velocity.unwrap_or([0.0; 3]));
        c.put("INC_INLET_TYPE", "VELOCITY_INLET");
        c.put(
            "MARKER_INLET",
            tuple(&[
                inlet.name.to_owned(),
                num(temperature),
                num(finite(speed, "the inlet speed")?),
                num(d[0]),
                num(d[1]),
                num(d[2]),
            ]),
        );
        if profile {
            c.put("SPECIFIED_INLET_PROFILE", "YES");
            c.put("INLET_FILENAME", INLET_FILE);
        }
    }
    let outlets = named(markers, |m| {
        (m.kind == BoundaryKind::PressureOutlet).then(|| format!("{}, {}", m.name, num(0.0)))
    });
    if !outlets.is_empty() {
        c.put("INC_OUTLET_TYPE", "PRESSURE_OUTLET");
        c.put("MARKER_OUTLET", tuple(&outlets));
    }
    let moving: Vec<&Marker> = markers
        .iter()
        .filter(|m| m.kind == BoundaryKind::MovingWall)
        .collect();
    if !moving.is_empty() {
        c.put(
            "SURFACE_MOVEMENT",
            moving
                .iter()
                .map(|_| "MOVING_WALL")
                .collect::<Vec<_>>()
                .join(", "),
        );
        c.put(
            "MARKER_MOVING",
            moving.iter().map(|m| m.name).collect::<Vec<_>>().join(", "),
        );
        let rates: Vec<String> = moving
            .iter()
            .flat_map(|m| m.velocity.unwrap_or([0.0; 3]))
            .map(num)
            .collect();
        c.put("SURFACE_TRANSLATION_RATE", rates.join(" "));
    }
    let adiabatic = named(markers, |m| {
        let wall = m.kind == BoundaryKind::MovingWall
            || (m.kind == BoundaryKind::NoSlipWall && m.temperature.is_none());
        wall.then(|| format!("{}, {}", m.name, num(0.0)))
    });
    if !adiabatic.is_empty() {
        c.put("MARKER_HEATFLUX", tuple(&adiabatic));
    }
    let isothermal = named(markers, |m| {
        m.temperature.map(|t| format!("{}, {}", m.name, num(t)))
    });
    if !isothermal.is_empty() {
        c.put("MARKER_ISOTHERMAL", tuple(&isothermal));
    }
    for (key, kind) in [
        ("MARKER_EULER", BoundaryKind::SlipWall),
        ("MARKER_SYM", BoundaryKind::Symmetry),
    ] {
        let names = named(markers, |m| (m.kind == kind).then(|| m.name.to_owned()));
        if !names.is_empty() {
            c.put(key, tuple(&names));
        }
    }
    Ok(())
}

/// The turbulent kinetic energy and the specific dissipation rate of the inflow, when the model
/// has them.
pub fn inflow_turbulence(study: &ResolvedStudy) -> Option<(f64, f64)> {
    let t = study.physics.as_ref().map(|p| &p.turbulence)?;
    if t.model != TurbulenceModel::KOmegaSst {
        return None;
    }
    let v = inlet_turbulence(
        t.intensity?,
        t.viscosity_ratio?,
        reference_speed(study)?,
        study.fluid.kinematic_viscosity,
    )?;
    Some((v.k, v.omega))
}

/// The files of the case: the configuration, the mesh and, for a parabolic inlet, its profile.
pub fn compile(study: &ResolvedStudy) -> Result<CaseTree, CompileFailure> {
    check_supported(study)?;
    let layout = layout_of(study)?;
    let list = markers(study, &layout)?;
    let mesh = mesh_of(&layout)?;
    let mut files = vec![
        CaseFile {
            rel_path: CONFIG_FILE.into(),
            bytes: config_text(study, &list)?.into_bytes(),
        },
        CaseFile {
            rel_path: MESH_FILE.into(),
            bytes: mesh_text(&mesh, &list)?.into_bytes(),
        },
    ];
    if let Some(text) = inlet::profile_text(study, &mesh, &list)? {
        files.push(CaseFile {
            rel_path: INLET_FILE.into(),
            bytes: text.into_bytes(),
        });
    }
    Ok(CaseTree { files })
}
