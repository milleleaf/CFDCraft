//! What templates exist, in words a person can choose from.

/// One template and the benchmark it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInfo {
    /// The id to pass to [`crate::build`].
    pub id: String,
    pub title: String,
    /// The id of the benchmark that judges a run of this template.
    pub benchmark: String,
    /// The published source of the reference values.
    pub reference: String,
    /// The physics the template has when no option changes it.
    pub defaults: String,
    /// The key numbers a mesh-independence study reads, by name.
    pub headline_quantities: Vec<String>,
}

fn info(
    id: &str,
    title: &str,
    benchmark: &str,
    reference: &str,
    defaults: &str,
    quantities: &[&str],
) -> TemplateInfo {
    TemplateInfo {
        id: id.to_string(),
        title: title.to_string(),
        benchmark: benchmark.to_string(),
        reference: reference.to_string(),
        defaults: defaults.to_string(),
        headline_quantities: quantities.iter().map(|name| (*name).to_string()).collect(),
    }
}

/// Every template, in a fixed order.
pub fn list() -> Vec<TemplateInfo> {
    vec![
        info(
            "lid-cavity",
            "Lid-driven cavity, Reynolds number 100",
            "ghia",
            "Ghia, Ghia and Shin, J. Comput. Phys. 48, 387-411 (1982)",
            "laminar, steady",
            &["bench.ghia.u_centre"],
        ),
        info(
            "backward-step",
            "Backward-facing step, turbulent, Reynolds number 36 000",
            "step",
            "Driver and Seegmiller, AIAA J. 23(2), 163-171 (1985), with the NASA turbulence modelling data",
            "k-omega SST, steady",
            &["bench.step.reattachment"],
        ),
        info(
            "backward-step-laminar",
            "Backward-facing step, laminar, Reynolds number 100 to 400",
            "armaly",
            "Armaly, Durst, Pereira and Schonung, J. Fluid Mech. 127, 473-496 (1983)",
            "laminar, steady",
            &["bench.armaly.reattachment"],
        ),
        info(
            "cylinder",
            "Flow around a cylinder, Reynolds number 100",
            "cylinder",
            "Schafer and Turek, Notes on Numerical Fluid Mechanics 52, 547-566 (1996), case 2D-2",
            "laminar, transient",
            &[
                "bench.cylinder.st",
                "bench.cylinder.cd_max",
                "bench.cylinder.cl_max",
            ],
        ),
        info(
            "heated-cavity",
            "Differentially heated cavity, Rayleigh number 1e3 to 1e6",
            "dvd",
            "de Vahl Davis, Int. J. Numer. Methods Fluids 3, 249-264 (1983)",
            "laminar, steady, Boussinesq buoyancy",
            &["bench.dvd.nu_hot", "bench.dvd.w_max"],
        ),
    ]
}
