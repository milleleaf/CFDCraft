//! Names, file names and keyword forms of Foundation OpenFOAM 14 that the writer relies on.
//!
//! Every entry marked `UNPROBED` is believed from the public release notes and the keyword names
//! of the 14 sources (read, never copied) and has not been shown by a real run of the pinned
//! image yet. A hosted probe checks each one, and a mismatch there is a reviewed change of the
//! constant, never a reason to weaken a test.

/// The solver module a case selects in its control dictionary. The module is run by `SOLVER`.
pub const SOLVER_MODULE: &str = "incompressibleFluid";
/// The program that runs the solver module chosen in the control dictionary.
pub const SOLVER: &str = "foamRun";
/// The log the solver's output is written to.
pub const SOLVER_LOG: &str = "log.foamRun";
/// The log of the mesh generator.
pub const BLOCK_MESH_LOG: &str = "log.blockMesh";
/// The log of the mesh check.
pub const CHECK_MESH_LOG: &str = "log.checkMesh";

/// The file that holds the viscosity model and the viscosity.
pub const PHYSICAL_PROPERTIES: &str = "constant/physicalProperties";
/// The file that holds the momentum transport (turbulence) selection.
pub const MOMENTUM_TRANSPORT: &str = "constant/momentumTransport";

/// UNPROBED: the library file names the function objects load. The 14 tutorials spell a library as
/// a quoted file name, where the older line takes the bare name.
pub const LIB_SAMPLING: &str = "libsampling.so";
/// UNPROBED: see `LIB_SAMPLING`.
pub const LIB_FIELD_FUNCTIONS: &str = "libfieldFunctionObjects.so";
/// UNPROBED: see `LIB_SAMPLING`.
pub const LIB_FORCES: &str = "libforces.so";

/// UNPROBED: the type of a uniformly sampled line inside the `sets` of a sampling function object,
/// which the 14 sources write as a dictionary of named sets and not as a list.
pub const LINE_SET_TYPE: &str = "lineUniform";
/// UNPROBED: the key that names the patch a surface integral reads (the older line writes a region
/// type and a name).
pub const SURFACE_PATCH_KEY: &str = "patch";

/// UNPROBED: the spelling of the unit set after the viscosity (`nu 1e-5 [m^2/s];`). A plain number
/// is also read by the 14 tutorials.
pub const VISCOSITY_UNITS: &str = "[m^2/s]";
/// UNPROBED: whether the numeric dimension set (`[0 1 -1 0 0 0 0]`) that the field files carry is
/// still read by the 14 field reader. The 14 tutorials write named units instead.
pub const NUMERIC_DIMENSIONS_READ: bool = true;
/// UNPROBED: whether `convertToMeters` in `blockMeshDict` is still read by the 14 mesh generator
/// (the release notes introduce a units form that can replace a scale).
pub const CONVERT_TO_METERS_READ: bool = true;
/// UNPROBED: whether the final-iteration solver entries (`pFinal`, `UFinal`) are required for the
/// single outer corrector steady form. They are written so that the question does not matter.
pub const FINAL_SOLVERS_REQUIRED: bool = false;
/// UNPROBED: the name the stress divergence scheme carries for the incompressible module.
pub const STRESS_DIVERGENCE_SCHEME: &str = "div((nuEff*dev2(T(grad(U)))))";

/// UNPROBED: the solver module that carries the heat transfer (the buoyant, enthalpy form), run by
/// `SOLVER` like the plain module.
pub const FLUID_MODULE: &str = "fluid";
/// UNPROBED: the command of the standalone potential-flow step and the flag that applies the
/// boundary values to the velocity. The older line spells the same flag.
pub const POTENTIAL_COMMAND: &str = "potentialFoam -initialiseUBCs";
/// The log of the potential-flow step.
pub const POTENTIAL_LOG: &str = "log.potentialFoam";
/// The time scheme of a transient run: second order, as the older line's transient runs.
pub const TRANSIENT_TIME_SCHEME: &str = "backward";
/// UNPROBED: the stress divergence scheme name of the fluid module, whose stress carries the
/// density.
pub const STRESS_DIVERGENCE_SCHEME_FLUID: &str = "div(((rho*nuEff)*dev2(T(grad(U)))))";

/// The file of the thermophysical transport models, written for every heat transfer case.
pub const THERMOPHYSICAL_TRANSPORT: &str = "constant/thermophysicalTransport";
/// UNPROBED: the model word of the thermal conduction of a laminar case.
pub const LAMINAR_HEAT_MODEL: &str = "Fourier";
/// UNPROBED: the model word of the turbulent heat transport; its turbulent Prandtl number is the
/// `Prt` entry beside it.
pub const TURBULENT_HEAT_MODEL: &str = "eddyDiffusivity";
/// UNPROBED: the thermodynamic type, the equation of state and the energy variable of the heated
/// cavity. The Boussinesq equation of state is `rho0 (1 - beta (T - T0))`.
pub const THERMO_TYPE: &str = "heRhoThermo";
/// UNPROBED: see `THERMO_TYPE`.
pub const EQUATION_OF_STATE: &str = "Boussinesq";
/// UNPROBED: see `THERMO_TYPE`.
pub const ENERGY: &str = "sensibleEnthalpy";
/// UNPROBED: the turbulent thermal diffusivity wall function of a heat transfer case with a
/// turbulence model.
pub const ALPHAT_WALL_FUNCTION: &str = "compressible::alphatWallFunction";
/// The field the fluid module solves the energy equation for; its residual is the temperature
/// residual of a heat transfer run.
pub const ENERGY_FIELD: &str = "h";
