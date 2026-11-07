use cfdcraft_bench_dvd::{PRANDTL, Table, Tolerances, judge_position, judge_relative, rayleigh};

const TEXT: &str = include_str!("../data/dvd_table5.toml");

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

#[test]
fn table_five_loads_and_is_pinned() {
    let table = Table::load().unwrap();
    let ras: Vec<f64> = table.rows.iter().map(|r| r.ra).collect();
    assert_eq!(ras, vec![1000.0, 10000.0, 100000.0, 1000000.0]);
    let r = table.row_for_ra(10000.0).unwrap();
    assert_eq!(
        [
            r.u_max, r.u_max_z, r.w_max, r.w_max_x, r.nu_avg, r.nu_0, r.nu_max, r.nu_max_z,
            r.nu_min, r.nu_min_z
        ],
        [
            16.178, 0.823, 19.617, 0.119, 2.243, 2.238, 3.528, 0.143, 0.586, 1.0
        ]
    );
    let top = table.row_for_ra(1.0e6).unwrap();
    assert_eq!(
        [top.u_max, top.w_max, top.nu_avg, top.nu_max],
        [64.63, 219.36, 8.8, 17.925]
    );
    let low = table.row_for_ra(1.0e3).unwrap();
    assert_eq!(
        [low.u_max, low.u_max_z, low.w_max, low.w_max_x, low.nu_avg],
        [3.649, 0.813, 3.697, 0.178, 1.118]
    );
    let mid = table.row_for_ra(1.0e5).unwrap();
    assert_eq!(
        [mid.u_max, mid.u_max_z, mid.w_max, mid.w_max_x, mid.nu_avg],
        [34.73, 0.855, 68.59, 0.066, 4.519]
    );
    assert_eq!(table.source.doi, "10.1002/fld.1650030305");
    assert_eq!(table.source.table, "V");
    assert_eq!(table.source.page, 256);
    assert_eq!(table.source.prandtl, PRANDTL);
    assert!(table.source.accuracy.contains("better than 1 per cent"));
}

#[test]
fn a_missing_field_or_a_value_that_is_not_finite_is_refused() {
    let without = TEXT.replacen("nu_min = 0.586\n", "", 1);
    assert_ne!(without, TEXT);
    assert!(Table::parse(&without).is_err());
    let not_finite = TEXT.replacen("nu_avg = 2.243", "nu_avg = nan", 1);
    assert_ne!(not_finite, TEXT);
    assert!(Table::parse(&not_finite).is_err());
    let twice = TEXT.replacen("ra = 10000.0", "ra = 1000.0", 1);
    assert!(Table::parse(&twice).is_err());
    let unexplained = TEXT.replace("justification = ", "unexplained = ");
    assert!(Table::parse(&unexplained).is_err());
}

#[test]
fn the_rayleigh_number_comes_from_the_study() {
    // The template numbers for Ra 1e4: g 9.81, beta 3e-3, dT 1 K, L 1 m, Pr 0.71.
    let ra = rayleigh(9.81, 3e-3, 1.0, 1.0, 1.4455e-3, 0.71);
    assert!(close(ra, 1.0e4, 1e-3), "{ra}");
    let table = Table::load().unwrap();
    assert_eq!(table.row_for_ra(ra).unwrap().ra, 1.0e4);
    assert_eq!(table.row_for_ra(1.004e6).unwrap().ra, 1.0e6);
    assert!(table.row_for_ra(1.006e6).is_none());
    assert!(table.row_for_ra(5.0e4).is_none());
    assert!(table.row_for_ra(f64::NAN).is_none());
    // A longer cavity side raises Ra with the cube.
    assert!(close(
        rayleigh(9.81, 3e-3, 1.0, 2.0, 1.4455e-3, 0.71),
        8.0 * ra,
        1e-12
    ));
}

#[test]
fn a_value_is_judged_with_a_relative_allowance() {
    let pass = judge_relative("Nu", 2.25, 2.243, 0.02);
    assert!(pass.passed);
    assert!((pass.max_abs_dev - 0.007).abs() < 1e-9);
    assert!((pass.tolerance - 0.04486).abs() < 1e-9);
    assert_eq!(pass.reference_range, Some([2.243, 2.243]));
    assert!(!judge_relative("Nu", 2.35, 2.243, 0.02).passed);
}

#[test]
fn a_position_is_judged_absolutely() {
    assert!(judge_position("u_max z", 0.80, 0.823, 0.03).passed);
    assert!(!judge_position("u_max z", 0.70, 0.823, 0.03).passed);
}

#[test]
fn the_tolerances_scale_at_the_highest_rayleigh_number() {
    let table = Table::load().unwrap();
    let t = &table.tolerances;
    let low = table.row_for_ra(1.0e5).unwrap();
    let top = table.row_for_ra(1.0e6).unwrap();
    assert_eq!((t.nu_allowance(low), t.nu_allowance(top)), (0.02, 0.03));
    assert_eq!(
        (t.velocity_allowance(low), t.velocity_allowance(top)),
        (0.03, 0.05)
    );
    assert_eq!(t.position_abs, 0.03);
    assert!(!t.justification.is_empty());

    let ok = "[tolerance]\nnu_rel = 0.05\njustification = \"a coarse mesh\"\n";
    let changed = Tolerances::from_toml(ok, t).unwrap();
    assert_eq!(changed.nu_rel, 0.05);
    assert_eq!(changed.velocity_rel, 0.03);
    assert_eq!(changed.justification, "a coarse mesh");
    // More than three times the shipped value, no reason given, a bad value, an unknown key.
    let wide = "[tolerance]\nnu_rel = 0.07\njustification = \"x\"\n";
    assert!(Tolerances::from_toml(wide, t).is_err());
    let silent = "[tolerance]\nnu_rel = 0.05\n";
    assert!(Tolerances::from_toml(silent, t).is_err());
    let zero = "[tolerance]\nnu_rel = 0.0\njustification = \"x\"\n";
    assert!(Tolerances::from_toml(zero, t).is_err());
    let unknown = "[tolerance]\nnu = 0.05\njustification = \"x\"\n";
    assert!(Tolerances::from_toml(unknown, t).is_err());
}
