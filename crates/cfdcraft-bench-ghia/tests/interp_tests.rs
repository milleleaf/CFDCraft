use cfdcraft_bench_ghia::{InterpError, Profile, linear_sample};

fn line() -> Profile {
    Profile {
        coords: vec![0.0, 0.5, 1.0],
        values: vec![0.0, 1.0, 0.0],
    }
}

#[test]
fn interpolation_at_knots_is_exact() {
    for (&x, &y) in line().coords.iter().zip(&line().values) {
        assert_eq!(linear_sample(&line(), x), Ok(y));
    }
}

#[test]
fn interpolation_between_knots_is_linear() {
    assert_eq!(linear_sample(&line(), 0.25), Ok(0.5));
    assert_eq!(linear_sample(&line(), 0.75), Ok(0.5));
}

#[test]
fn outside_range_clamps_to_nearest_endpoint() {
    let p = Profile {
        coords: vec![0.2, 0.8],
        values: vec![1.0, 3.0],
    };
    assert_eq!(linear_sample(&p, 0.0), Ok(1.0));
    assert_eq!(linear_sample(&p, 1.0), Ok(3.0));
}

#[test]
fn empty_and_single_point_profiles_return_error() {
    for n in [0, 1] {
        assert_eq!(
            linear_sample(
                &Profile {
                    coords: vec![0.0; n],
                    values: vec![0.0; n]
                },
                0.0
            ),
            Err(InterpError::TooFewPoints)
        );
    }
}

#[test]
fn mismatched_lengths_are_rejected() {
    assert_eq!(
        linear_sample(
            &Profile {
                coords: vec![0.0, 1.0],
                values: vec![0.0]
            },
            0.0
        ),
        Err(InterpError::LengthMismatch)
    );
}

#[test]
fn unsorted_and_duplicate_coordinates_are_rejected() {
    for coords in [vec![1.0, 0.0], vec![0.0, 0.0]] {
        assert_eq!(
            linear_sample(
                &Profile {
                    coords,
                    values: vec![0.0, 1.0]
                },
                0.0
            ),
            Err(InterpError::NotIncreasing)
        );
    }
}

#[test]
fn nonfinite_coordinates_velocities_and_query_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut p = line();
        p.coords[1] = value;
        assert_eq!(linear_sample(&p, 0.25), Err(InterpError::NonFinite));
        p = line();
        p.values[1] = value;
        assert_eq!(linear_sample(&p, 0.25), Err(InterpError::NonFinite));
        assert_eq!(linear_sample(&line(), value), Err(InterpError::NonFinite));
    }
}
