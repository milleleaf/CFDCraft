mod support;

use cfdcraft_bench_dvd::fixture::{Tweak, good_files};
use cfdcraft_bench_dvd::{Refusal, Setup, measure, peak};
use cfdcraft_model::ResolvedStudy;
use support::{Case, Tree, study};

fn tree(r: &ResolvedStudy) -> Tree {
    Tree::from_pairs(good_files(r, &Tweak::default()).unwrap())
}

/// The path of the one file a function object wrote.
fn path_of(t: &Tree, fo: &str) -> String {
    let prefix = format!("postProcessing/{fo}/");
    t.0.keys()
        .find(|k| k.starts_with(&prefix))
        .cloned()
        .unwrap()
}

fn surface(area: f64, rows: &[f64]) -> String {
    let mut out = format!(
        "# Region type : patch isothermal2\n# Faces : 40\n# Area : {area}\n# Time areaIntegrate(gradT)\n"
    );
    for (i, v) in rows.iter().enumerate() {
        out.push_str(&format!("{} ({v} 0 0)\n", i + 1));
    }
    out
}

/// A line of 41 samples along a unit cavity side, as four or six columns, whose `component` is a
/// parabola that peaks at `at` with the value `top` in m/s.
fn line(columns: usize, axis: usize, component: usize, at: f64, top: f64) -> String {
    let mut out = String::new();
    for i in 0..=40 {
        let t = f64::from(i) / 40.0;
        let v = top - 2.0 * (t - at) * (t - at);
        let mut u = [0.0; 3];
        u[component] = v;
        if columns == 6 {
            let (x, y) = if axis == 0 { (t, 0.5) } else { (0.5, t) };
            out.push_str(&format!("{x} {y} 0.05 {} {} {}\n", u[0], u[1], u[2]));
        } else {
            out.push_str(&format!("{t} {} {} {}\n", u[0], u[1], u[2]));
        }
    }
    out
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn nusselt_from_the_hot_wall_flux() {
    let r = study(&Case::default());
    let setup = Setup::from_study(&r).unwrap();
    assert!(near(setup.length, 1.0, 1e-12) && near(setup.delta_t, 1.0, 1e-12));
    let mut t = tree(&r);
    let hot = path_of(&t, &setup.hot_fo);
    // The last row counts, not the first.
    t.put(&hot, &surface(1.0, &[-9.0, -1.12]));
    assert!(near(measure(&t, &r).unwrap().nu_hot, 1.12, 1e-9));
    t.put(&hot, &surface(0.1, &[-0.112]));
    assert!(near(measure(&t, &r).unwrap().nu_hot, 1.12, 1e-9));
    // A positive gradient integral is a negative Nusselt number.
    t.put(&hot, &surface(0.1, &[0.112]));
    assert!(near(measure(&t, &r).unwrap().nu_hot, -1.12, 1e-9));
}

#[test]
fn the_peak_is_refined_by_a_parabola() {
    let xs = [0.0, 0.25, 0.5, 0.75, 1.0];
    let samples: Vec<(f64, f64)> = xs
        .iter()
        .map(|&x| (x, 5.0 - (x - 0.3) * (x - 0.3)))
        .collect();
    let (at, top) = peak("test", &samples).unwrap();
    assert!(near(at, 0.3, 1e-6) && near(top, 5.0, 1e-6), "{at} {top}");
    // The largest sample at an end is taken as it is.
    let rising = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];
    assert_eq!(peak("test", &rising).unwrap(), (2.0, 2.0));
    // Too few samples, or positions that do not increase, are refused.
    assert!(peak("test", &rising[..2]).is_err());
    assert!(peak("test", &[(0.0, 1.0), (0.0, 2.0), (1.0, 1.0)]).is_err());
}

#[test]
fn velocities_are_scaled_by_the_diffusivity() {
    let r = study(&Case::default());
    let setup = Setup::from_study(&r).unwrap();
    let mut t = tree(&r);
    let vertical = path_of(&t, &setup.vertical_fo);
    let horizontal = path_of(&t, &setup.horizontal_fo);
    t.put(&vertical, &line(6, 1, 0, 0.823, 0.0280));
    t.put(&horizontal, &line(6, 0, 1, 0.119, 0.0100));
    let m = measure(&t, &r).unwrap();
    let scale = 1.0 * 0.71 / setup.nu;
    assert!(near(setup.nu, 1.4455e-3, 1e-6), "{}", setup.nu);
    assert!(near(m.u_max, 0.0280 * scale, 1e-6), "{}", m.u_max);
    assert!(near(m.u_max, 13.75, 0.01), "{}", m.u_max);
    assert!(near(m.u_max_y, 0.823, 1e-6), "{}", m.u_max_y);
    assert!(near(m.w_max, 0.0100 * scale, 1e-6), "{}", m.w_max);
    assert!(near(m.w_max_x, 0.119, 1e-6), "{}", m.w_max_x);
}

#[test]
fn the_energy_imbalance_is_reported() {
    let r = study(&Case::default());
    let setup = Setup::from_study(&r).unwrap();
    let mut t = tree(&r);
    let hot = path_of(&t, &setup.hot_fo);
    let cold = path_of(&t, &setup.cold_fo);
    t.put(&hot, &surface(1.0, &[-1.12]));
    t.put(&cold, &surface(1.0, &[-1.10]));
    let m = measure(&t, &r).unwrap();
    assert!(near(m.energy_imbalance, 0.02 / 1.12, 1e-9));
    assert!(near(m.energy_imbalance, 0.01786, 1e-5));
    t.put(&cold, &surface(1.0, &[-1.12]));
    assert!(near(measure(&t, &r).unwrap().energy_imbalance, 0.0, 1e-12));
}

#[test]
fn a_missing_or_unreadable_file_is_named() {
    let r = study(&Case::default());
    let setup = Setup::from_study(&r).unwrap();
    let full = tree(&r);

    // No folder for the hot flux.
    let mut t = Tree(full.0.clone());
    let prefix = format!("postProcessing/{}/", setup.hot_fo);
    t.0.retain(|k, _| !k.starts_with(&prefix));
    let err = measure(&t, &r).unwrap_err();
    assert!(matches!(err, Refusal::Missing { .. }), "{err}");
    assert!(err.to_string().starts_with("CFD-BENCH-002"));
    assert!(err.to_string().contains(&setup.hot_fo), "{err}");

    // A heat-flux file with no area.
    let mut t = Tree(full.0.clone());
    let hot = path_of(&t, &setup.hot_fo);
    let text = t.0[&hot].replace("# Area", "# Space");
    t.put(&hot, &text);
    let err = measure(&t, &r).unwrap_err();
    assert!(matches!(err, Refusal::Unreadable { .. }), "{err}");
    assert!(err.to_string().contains(&hot), "{err}");

    // A mid-plane file with a number that is not finite.
    let mut t = Tree(full.0.clone());
    let vertical = path_of(&t, &setup.vertical_fo);
    let text = t.0[&vertical].replacen("0.000000000000 ", "NaN ", 1);
    assert_ne!(text, t.0[&vertical]);
    t.put(&vertical, &text);
    let err = measure(&t, &r).unwrap_err();
    assert!(matches!(err, Refusal::Unreadable { .. }), "{err}");
    assert!(err.to_string().contains(&vertical), "{err}");

    // A study without the hot flux surface is named by its label.
    let without = study(&Case {
        hot_flux: false,
        ..Case::default()
    });
    let err = measure(&full, &without).unwrap_err();
    assert!(matches!(err, Refusal::Unusable { .. }), "{err}");
    assert!(err.to_string().contains("hot_flux"), "{err}");
}

#[test]
fn both_line_layouts_are_read() {
    let r = study(&Case::default());
    let setup = Setup::from_study(&r).unwrap();
    let mut six = tree(&r);
    let mut four = tree(&r);
    for (t, columns) in [(&mut six, 6), (&mut four, 4)] {
        let vertical = path_of(t, &setup.vertical_fo);
        let horizontal = path_of(t, &setup.horizontal_fo);
        t.put(&vertical, &line(columns, 1, 0, 0.823, 0.0280));
        t.put(&horizontal, &line(columns, 0, 1, 0.119, 0.0100));
    }
    let a = measure(&six, &r).unwrap();
    let b = measure(&four, &r).unwrap();
    assert!(near(a.u_max_y, b.u_max_y, 1e-9) && near(a.w_max_x, b.w_max_x, 1e-9));
    assert!(near(a.u_max, b.u_max, 1e-9) && near(a.w_max, b.w_max, 1e-9));
    assert!(near(a.u_max_y, 0.823, 1e-6));
}
