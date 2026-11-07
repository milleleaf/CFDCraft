use cfdcraft_bench_cylinder::{LoadError, Reference, Tolerances, judge_all, judge_scalar};
use cfdcraft_gate::MetricKind;

const DATA: &str = include_str!("../data/st_2d2.toml");

fn shipped() -> (Reference, Tolerances) {
    (
        Reference::embedded().unwrap(),
        Tolerances::default_embedded().unwrap(),
    )
}

/// The shipped data with one piece of text replaced; the piece must exist.
fn edited(from: &str, to: &str) -> String {
    assert!(DATA.contains(from), "{from} is not in the data");
    DATA.replace(from, to)
}

#[test]
fn the_reference_loads() {
    let (r, t) = shipped();
    assert_eq!((r.cd_max.low, r.cd_max.high), (3.22, 3.24));
    assert_eq!((r.cl_max.low, r.cl_max.high), (0.99, 1.01));
    assert_eq!((r.st.low, r.st.high), (0.295, 0.305));
    assert_eq!((r.delta_p.low, r.delta_p.high), (2.46, 2.5));
    assert_eq!(
        (t.cd_max, t.cl_max, t.st, t.delta_p),
        (0.02, 0.03, 0.03, 0.03)
    );
    assert_eq!((t.window_cycles, t.min_cycles), (5, 10));
    assert!(!t.overridden);
    assert!(t.justification.contains("0.6 %"));
    assert_eq!(r.source.doi, "10.1007/978-3-322-89849-4_39");
    assert_eq!((r.source.table.as_str(), r.source.page), ("Table 4", 13));
    assert!(r.source.terms.starts_with("no terms stated"));
    assert_eq!(r.definition.reynolds, "Re = Ubar D / nu");
    assert_eq!(r.definition.drag, "cD = 2 F / (rho Ubar^2 D)");
    assert_eq!(r.definition.strouhal, "St = D f / Ubar");
    assert_eq!(r.definition.front_point, [0.15, 0.2]);
    assert_eq!(r.definition.rear_point, [0.25, 0.2]);
}

#[test]
fn a_damaged_data_file_is_refused() {
    let missing = edited("page = 13\n", "");
    assert!(matches!(
        Reference::parse(&missing),
        Err(LoadError::Corrupt { .. })
    ));
    let inverted = edited("low = 3.2200\nhigh = 3.2400", "low = 3.2500\nhigh = 3.2400");
    assert!(matches!(
        Reference::parse(&inverted),
        Err(LoadError::Corrupt { .. })
    ));
    let zero = edited("cd_max = 0.02\n", "cd_max = 0.0\n");
    assert!(matches!(
        Tolerances::parse(&zero),
        Err(LoadError::Corrupt { .. })
    ));
    let window = edited("min_cycles = 10\n", "min_cycles = 4\n");
    assert!(matches!(
        Tolerances::parse(&window),
        Err(LoadError::Corrupt { .. })
    ));
    let unknown = edited("[tolerance]\n", "[tolerance]\nextra = 1\n");
    assert!(matches!(
        Tolerances::parse(&unknown),
        Err(LoadError::Corrupt { .. })
    ));
}

#[test]
fn a_value_inside_the_widened_interval_passes() {
    let m = judge_scalar("cDmax", 3.2500, 3.22, 3.24, 0.02);
    assert!(m.passed);
    assert!((m.max_abs_dev - 0.01).abs() < 1e-9);
    assert!((m.tolerance - 0.02 * 3.23).abs() < 1e-12);
    assert_eq!(m.kind, MetricKind::Scalar);
    assert_eq!(m.reference_range, Some([3.22, 3.24]));

    let inside = judge_scalar("cDmax", 3.23, 3.22, 3.24, 0.02);
    assert!(inside.passed);
    assert_eq!(inside.max_abs_dev, 0.0);

    let far = judge_scalar("cDmax", 3.40, 3.22, 3.24, 0.02);
    assert!(!far.passed);
    assert!((far.max_abs_dev - 0.16).abs() < 1e-9);

    assert!(!judge_scalar("cDmax", f64::NAN, 3.22, 3.24, 0.02).passed);
}

#[test]
fn the_four_scalars_make_one_report() {
    let (r, t) = shipped();
    let report = judge_all(&r, &t, 3.24, 1.0, 0.3, 2.48);
    assert_eq!(report.name, "cylinder");
    assert!(report.passed);
    assert!(report.notes.is_empty());
    let names: Vec<&str> = report.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["cDmax", "cLmax", "St", "DeltaP"]);
    for m in &report.metrics {
        assert_eq!(m.kind, MetricKind::Scalar);
        assert!(m.reference_range.is_some());
        assert_eq!(m.unit.as_deref(), Some("1"));
    }

    let failed = judge_all(&r, &t, 3.24, 1.3, 0.3, 2.48);
    assert!(!failed.passed);
    let failing: Vec<&str> = failed
        .metrics
        .iter()
        .filter(|m| !m.passed)
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(failing, ["cLmax"]);
}

#[test]
fn overrides_are_bounded() {
    let wide = "[tolerance]\ncd_max = 0.05\njustification = \"a coarse first mesh\"\n";
    let t = Tolerances::from_toml(wide).unwrap();
    assert!(t.overridden);
    assert_eq!(t.cd_max, 0.05);
    assert_eq!(t.cl_max, 0.03);
    assert_eq!(t.justification, "a coarse first mesh");
    assert_eq!((t.window_cycles, t.min_cycles), (5, 10));

    let refused = [
        // More than 3 times the shipped allowance.
        "[tolerance]\ncd_max = 0.07\njustification = \"too wide\"\n",
        "[tolerance]\ncl_max = 0.0\njustification = \"zero\"\n",
        "[tolerance]\nst = -0.01\njustification = \"negative\"\n",
        "[tolerance]\ndelta_p = nan\njustification = \"not a number\"\n",
        // No reason, a blank reason, a reason with a control character.
        "[tolerance]\ncd_max = 0.05\n",
        "[tolerance]\ncd_max = 0.05\njustification = \"   \"\n",
        "[tolerance]\ncd_max = 0.05\njustification = \"a\\u0001b\"\n",
        // The window is not an allowance.
        "[tolerance]\nmin_cycles = 2\njustification = \"fewer cycles\"\n",
        "not a table",
    ];
    for text in refused {
        assert!(
            matches!(Tolerances::from_toml(text), Err(LoadError::Override { .. })),
            "{text}"
        );
    }
}

#[test]
fn an_override_is_named_in_the_report() {
    let (r, _) = shipped();
    let wide = "[tolerance]\ncd_max = 0.05\njustification = \"a coarse first mesh\"\n";
    let t = Tolerances::from_toml(wide).unwrap();
    let report = judge_all(&r, &t, 3.24, 1.0, 0.3, 2.48);
    assert_eq!(report.notes.len(), 1);
    assert!(report.notes[0].starts_with("tolerance override used:"));
    assert!(report.notes[0].contains("a coarse first mesh"));
}
