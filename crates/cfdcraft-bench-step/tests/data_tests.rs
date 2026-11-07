use cfdcraft_bench_step::{ArmalyCurve, DsProfiles, STATIONS};

const CSV: &str = include_str!("../data/ds_profiles.csv");

#[test]
fn the_profile_table_is_pinned() {
    assert!(CSV.starts_with("x_over_h,y_over_h,u_over_uref\n"));
    let p = DsProfiles::load().unwrap();
    let xs: Vec<f64> = p.stations.iter().map(|s| s.x_over_h).collect();
    assert_eq!(xs, STATIONS);
    let ends = [
        ((0.1, -0.008), (8.2, 0.919)),
        ((0.1, -0.171), (8.2, 0.892)),
        ((0.1, 0.147), (8.2, 0.87)),
        ((0.1, 0.418), (8.2, 0.827)),
    ];
    for (station, (first, last)) in p.stations.iter().zip(ends) {
        assert_eq!(station.y_over_h.len(), 28);
        assert_eq!(station.u_over_uref.len(), 28);
        assert_eq!((station.y_over_h[0], station.u_over_uref[0]), first);
        assert_eq!((station.y_over_h[27], station.u_over_uref[27]), last);
        assert!(station.y_over_h.windows(2).all(|w| w[0] < w[1]));
        assert!(
            station
                .u_over_uref
                .iter()
                .all(|u| u.is_finite() && (-0.5..=1.2).contains(u))
        );
    }
    assert_eq!(p.station(4.0).map(|s| s.x_over_h), Some(4.0));
    assert!(p.station(2.0).is_none());
}

/// A table with `n` rows at each station, the height rising from 0.1 and the velocity at 0.5.
fn table(stations: &[f64], n: usize) -> String {
    let mut text = String::from("x_over_h,y_over_h,u_over_uref\n");
    for x in stations {
        for i in 0..n {
            text.push_str(&format!("{x},{},0.5\n", 0.1 + 0.1 * i as f64));
        }
    }
    text
}

#[test]
fn a_profile_table_with_a_defect_is_refused() {
    assert!(DsProfiles::parse(&table(&STATIONS, 10)).is_ok());
    // Too few rows, a missing or extra station.
    assert!(DsProfiles::parse(&table(&STATIONS, 9)).is_err());
    assert!(DsProfiles::parse(&table(&[1.0, 4.0, 6.0], 10)).is_err());
    assert!(DsProfiles::parse(&table(&[1.0, 4.0, 6.0, 10.0, 12.0], 10)).is_err());
    // The header, a value that is not a number, a velocity out of range, a falling height.
    let ok = table(&STATIONS, 10);
    assert!(DsProfiles::parse(&ok.replacen("x_over_h", "x", 1)).is_err());
    assert!(DsProfiles::parse(&ok.replacen("0.5\n", "nan\n", 1)).is_err());
    assert!(DsProfiles::parse(&ok.replacen("0.5\n", "1.5\n", 1)).is_err());
    assert!(DsProfiles::parse(&ok.replacen("0.2,0.5", "0.05,0.5", 1)).is_err());
    assert!(DsProfiles::parse(&ok.replacen("0.5\n", "0.5,7\n", 1)).is_err());
    // The rows of one station must stay together.
    let mixed = format!("{ok}1,9.0,0.5\n");
    assert!(DsProfiles::parse(&mixed).is_err());
}

const ARMALY_CSV: &str = include_str!("../data/armaly_reattachment.csv");

#[test]
fn the_digitised_curve_is_pinned() {
    assert!(ARMALY_CSV.starts_with("re,x1_over_s\n"));
    let c = ArmalyCurve::load().unwrap();
    assert_eq!(c.re.len(), 7);
    assert_eq!(c.re, [100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0]);
    assert!(c.re.windows(2).all(|w| w[0] < w[1] && w[1] - w[0] <= 50.0));
    assert!(
        c.x1_over_s
            .iter()
            .all(|x| x.is_finite() && (2.0..=15.0).contains(x))
    );
    assert!(c.x1_over_s.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!((c.re[0], c.x1_over_s[0]), (100.0, 3.0));
    assert_eq!((c.re[6], c.x1_over_s[6]), (400.0, 8.6));
}

#[test]
fn a_curve_with_a_defect_is_refused() {
    let rows = |text: &str| format!("re,x1_over_s\n{text}");
    let ok = "100,3\n150,4\n200,5\n250,6\n300,7\n350,8\n400,9\n";
    assert!(ArmalyCurve::parse(&rows(ok)).is_ok());
    // Too few rows, a gap wider than 50, a range that does not span 100 to 400.
    assert!(ArmalyCurve::parse(&rows("100,3\n200,5\n300,7\n400,9\n")).is_err());
    assert!(
        ArmalyCurve::parse(&rows("100,3\n150,4\n200,5\n250,6\n300,7\n350,8\n450,9\n")).is_err()
    );
    assert!(ArmalyCurve::parse(&rows("100,3\n150,4\n200,5\n250,6\n300,7\n350,8\n")).is_err());
    assert!(
        ArmalyCurve::parse(&rows("125,3\n150,4\n200,5\n250,6\n300,7\n350,8\n400,9\n")).is_err()
    );
    // A falling length, a length out of range, a Reynolds number that does not rise, a bad header.
    assert!(ArmalyCurve::parse(&rows(&ok.replace("300,7", "300,4.5"))).is_err());
    assert!(ArmalyCurve::parse(&rows(&ok.replace("400,9", "400,16"))).is_err());
    assert!(ArmalyCurve::parse(&rows(&ok.replace("250,6", "150,6"))).is_err());
    assert!(ArmalyCurve::parse(&ok.replace("100,3", "re,x\n100,3")).is_err());
    assert!(ArmalyCurve::parse(&rows(&ok.replace("200,5", "200,nan"))).is_err());
}
