//! The sentences that say why a stage is out of date.
//!
//! A change between two recipes is plain data. Each one becomes a line of the text crate, and the
//! lines of one stage are joined into the reason the status shows, for example `lid velocity
//! 0.1 → 0.2 m/s`. Numbers print as Rust prints a decimal (no exponent, `0.1` and `128`), an
//! absent velocity as `none`, and a long image digest in its first 19 characters.

use cfdcraft_keys::{Axis, Change, PhysicsPart, Residual, Stage, ToolPart};
use cfdcraft_text::{Line, Text};

/// How many characters of an image digest a sentence shows (`sha256:` and twelve more).
const DIGEST_SHOWN: usize = 19;

fn axis_word(axis: Axis) -> String {
    match axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
    .to_owned()
}

fn number(value: f64) -> String {
    value.to_string()
}

fn velocity(value: Option<f64>) -> String {
    value.map_or_else(|| "none".to_owned(), number)
}

fn short_digest(digest: &str) -> String {
    if digest.starts_with("sha256:") && digest.chars().count() > DIGEST_SHOWN {
        digest.chars().take(DIGEST_SHOWN).collect()
    } else {
        digest.to_owned()
    }
}

/// The part of the physics in the snake_case word of a study file.
fn physics_word(part: PhysicsPart) -> &'static str {
    match part {
        PhysicsPart::Time => "time",
        PhysicsPart::DeltaT => "delta_t",
        PhysicsPart::Turbulence => "turbulence",
        PhysicsPart::Intensity => "intensity",
        PhysicsPart::ViscosityRatio => "viscosity_ratio",
        PhysicsPart::ReferenceVelocity => "reference_velocity",
        PhysicsPart::Buoyancy => "buoyancy",
        PhysicsPart::Init => "init",
    }
}

/// The line that tells one change.
pub fn change_line(change: &Change) -> Line {
    match change {
        Change::Tool { part, from, to } => match part {
            ToolPart::Image => Line::ReasonImage {
                from: short_digest(from),
                to: short_digest(to),
            },
            ToolPart::Dialect => Line::ReasonDialect {
                from: from.clone(),
                to: to.clone(),
            },
            ToolPart::Sidecar => Line::ReasonSidecar {
                from: from.clone(),
                to: to.clone(),
            },
            ToolPart::Writer => Line::ReasonWriter {
                from: from.clone(),
                to: to.clone(),
            },
        },
        Change::Length { axis, from, to } => Line::ReasonLength {
            axis: axis_word(*axis),
            from: number(*from),
            to: number(*to),
        },
        Change::DomainOpen { to, .. } => {
            if *to {
                Line::ReasonDomainOpened
            } else {
                Line::ReasonDomainClosed
            }
        }
        Change::Cells { axis, from, to } => Line::ReasonCells {
            axis: axis_word(*axis),
            from: from.to_string(),
            to: to.to_string(),
        },
        Change::MeshLevel { from, to } => Line::ReasonMeshLevel {
            from: from.to_string(),
            to: to.to_string(),
        },
        Change::Grading { axis, from, to } => Line::ReasonGrading {
            axis: axis_word(*axis),
            from: number(*from),
            to: number(*to),
        },
        Change::Shape { from, to } => Line::ReasonShape {
            from: from.clone().unwrap_or_else(|| "none".to_owned()),
            to: to.clone().unwrap_or_else(|| "none".to_owned()),
        },
        Change::Layout => Line::ReasonLayout,
        Change::InletProfile { boundary, from, to } => Line::ReasonInletProfile {
            boundary: boundary.clone(),
            from: from.clone().unwrap_or_else(|| "none".to_owned()),
            to: to.clone().unwrap_or_else(|| "none".to_owned()),
        },
        Change::Physics { what, from, to } => Line::ReasonPhysics {
            what: physics_word(*what).to_owned(),
            from: from.clone(),
            to: to.clone(),
        },
        Change::Thermal { boundary, from, to } => Line::ReasonThermal {
            boundary: boundary.clone(),
            from: from.clone().unwrap_or_else(|| "none".to_owned()),
            to: to.clone().unwrap_or_else(|| "none".to_owned()),
        },
        Change::Viscosity { from, to } => Line::ReasonViscosity {
            from: number(*from),
            to: number(*to),
        },
        Change::BoundaryVelocity { boundary, from, to } => Line::ReasonBoundaryVelocity {
            boundary: boundary.clone(),
            from: velocity(*from),
            to: velocity(*to),
        },
        Change::BoundaryKind { boundary, from, to } => Line::ReasonBoundaryKind {
            boundary: boundary.clone(),
            from: from.clone(),
            to: to.clone(),
        },
        Change::BoundaryFaces { boundary, from, to } => Line::ReasonBoundaryFaces {
            boundary: boundary.clone(),
            from: from.clone(),
            to: to.clone(),
        },
        Change::BoundaryAdded { boundary } => Line::ReasonBoundaryAdded {
            boundary: boundary.clone(),
        },
        Change::BoundaryRemoved { boundary } => Line::ReasonBoundaryRemoved {
            boundary: boundary.clone(),
        },
        Change::EndTime { from, to } => Line::ReasonEndTime {
            from: number(*from),
            to: number(*to),
        },
        Change::WriteInterval { from, to } => Line::ReasonWriteInterval {
            from: number(*from),
            to: number(*to),
        },
        Change::Convergence { which, from, to } => match which {
            Residual::Pressure => Line::ReasonPressureLimit {
                from: number(*from),
                to: number(*to),
            },
            Residual::Velocity => Line::ReasonVelocityLimit {
                from: number(*from),
                to: number(*to),
            },
        },
        Change::CentrelineSamples { from, to } => Line::ReasonSamples {
            from: from.to_string(),
            to: to.to_string(),
        },
        Change::MeshChanged => Line::ReasonMesh,
        Change::RawOverride { id, from, to } => Line::ReasonRawOverride {
            id: id.clone(),
            from: from.clone().unwrap_or_else(|| "none".to_owned()),
            to: to.clone().unwrap_or_else(|| "none".to_owned()),
        },
        Change::RawMesh => Line::ReasonRawMesh,
        Change::Monitor { id, from, to } => Line::ReasonMonitor {
            id: id.clone(),
            from: from.clone().unwrap_or_else(|| "none".to_owned()),
            to: to.clone().unwrap_or_else(|| "none".to_owned()),
        },
    }
}

/// Each change as its own sentence part, in the order given.
pub fn reason_parts(changes: &[Change]) -> Vec<String> {
    let text = Text::english();
    changes
        .iter()
        .map(|change| text.render(&change_line(change)))
        .collect()
}

/// All the reasons of one stage, joined by a semicolon and a space.
pub fn reason_text(changes: &[Change]) -> String {
    reason_parts(changes).join("; ")
}

/// The line that says `stage` is out of date and why.
pub fn stale_line(stage: Stage, changes: &[Change]) -> Line {
    Line::StaleBecause {
        stage: stage.as_str().to_owned(),
        reason: reason_text(changes),
    }
}

/// The sentence that says `stage` is out of date and why, for example
/// `result stale because lid velocity 0.1 → 0.2 m/s`.
pub fn stale_sentence(stage: Stage, changes: &[Change]) -> String {
    Text::english().render(&stale_line(stage, changes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(change: &Change) -> String {
        Text::english().render(&change_line(change))
    }

    fn tool(part: ToolPart, from: &str, to: &str) -> Change {
        Change::Tool {
            part,
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    fn boundary(name: &str) -> String {
        name.to_owned()
    }

    #[test]
    fn every_change_has_its_sentence() {
        let table: Vec<(Change, &str)> = vec![
            (
                tool(
                    ToolPart::Image,
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaa",
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbb",
                ),
                "solver image sha256:aaaaaaaaaaaa → sha256:bbbbbbbbbbbb",
            ),
            (
                tool(ToolPart::Image, "sha256:aa", "other"),
                "solver image sha256:aa → other",
            ),
            (
                tool(ToolPart::Dialect, "esi-v2512", "esi-v2606"),
                "solver dialect esi-v2512 → esi-v2606",
            ),
            (
                tool(ToolPart::Sidecar, "none", "sha256:cccccccccccccccccccccccc"),
                "geometry sidecar lock none → sha256:cccccccccccccccccccccccc",
            ),
            (
                tool(ToolPart::Writer, "0.1.0", "0.2.0"),
                "CFDCraft version 0.1.0 → 0.2.0",
            ),
            (
                Change::Length {
                    axis: Axis::X,
                    from: 0.1,
                    to: 0.2,
                },
                "domain length in x 0.1 → 0.2 m",
            ),
            (
                Change::Length {
                    axis: Axis::Z,
                    from: 1.0,
                    to: 2.5,
                },
                "domain length in z 1 → 2.5 m",
            ),
            (
                Change::DomainOpen {
                    from: false,
                    to: true,
                },
                "the domain was opened",
            ),
            (
                Change::DomainOpen {
                    from: true,
                    to: false,
                },
                "the domain was closed",
            ),
            (
                Change::Cells {
                    axis: Axis::Y,
                    from: 128,
                    to: 256,
                },
                "cells in y 128 → 256",
            ),
            (Change::Layout, "the boundary layout changed"),
            (
                Change::Viscosity {
                    from: 0.0001,
                    to: 0.0002,
                },
                "viscosity 0.0001 → 0.0002 m²/s",
            ),
            (
                Change::BoundaryVelocity {
                    boundary: boundary("lid"),
                    from: Some(0.1),
                    to: Some(0.2),
                },
                "lid velocity 0.1 → 0.2 m/s",
            ),
            (
                Change::BoundaryVelocity {
                    boundary: boundary("walls"),
                    from: None,
                    to: Some(0.5),
                },
                "walls velocity none → 0.5 m/s",
            ),
            (
                Change::BoundaryVelocity {
                    boundary: boundary("walls"),
                    from: Some(0.5),
                    to: None,
                },
                "walls velocity 0.5 → none m/s",
            ),
            (
                Change::BoundaryKind {
                    boundary: boundary("lid"),
                    from: "MovingWall".to_owned(),
                    to: "NoSlipWall".to_owned(),
                },
                "lid kind MovingWall → NoSlipWall",
            ),
            (
                Change::BoundaryFaces {
                    boundary: boundary("walls"),
                    from: "XMin".to_owned(),
                    to: "XMin, XMax".to_owned(),
                },
                "walls faces XMin → XMin, XMax",
            ),
            (
                Change::BoundaryAdded {
                    boundary: boundary("top"),
                },
                "boundary top was added",
            ),
            (
                Change::BoundaryRemoved {
                    boundary: boundary("lid"),
                },
                "boundary lid was removed",
            ),
            (
                Change::EndTime {
                    from: 5000.0,
                    to: 6000.0,
                },
                "end time 5000 → 6000 s",
            ),
            (
                Change::WriteInterval {
                    from: 100.0,
                    to: 200.0,
                },
                "write interval 100 → 200 s",
            ),
            (
                Change::Convergence {
                    which: Residual::Pressure,
                    from: 0.000001,
                    to: 0.00001,
                },
                "pressure residual limit 0.000001 → 0.00001",
            ),
            (
                Change::Convergence {
                    which: Residual::Velocity,
                    from: 0.000001,
                    to: 0.00001,
                },
                "velocity residual limit 0.000001 → 0.00001",
            ),
            (
                Change::CentrelineSamples { from: 64, to: 128 },
                "centreline samples 64 → 128",
            ),
            (Change::MeshChanged, "its mesh changed"),
            (
                Change::RawOverride {
                    id: "relax-u".to_owned(),
                    from: Some(
                        "esi system/fvSolution relaxationFactors/equations/U = 0.7".to_owned(),
                    ),
                    to: Some(
                        "esi system/fvSolution relaxationFactors/equations/U = 0.5".to_owned(),
                    ),
                },
                "raw override relax-u esi system/fvSolution relaxationFactors/equations/U = 0.7 \
                 → esi system/fvSolution relaxationFactors/equations/U = 0.5",
            ),
            (Change::RawMesh, "a raw mesh override changed"),
            (
                Change::Monitor {
                    id: "probe_2".to_owned(),
                    from: Some("x 0.025 m, y 0.025 m, velocity".to_owned()),
                    to: Some("x 0.03 m, y 0.025 m, velocity".to_owned()),
                },
                "monitor probe_2 x 0.025 m, y 0.025 m, velocity → x 0.03 m, y 0.025 m, velocity",
            ),
        ];
        for (change, expected) in table {
            assert_eq!(text(&change), expected, "{change:?}");
        }
    }

    #[test]
    fn the_mesh_level_grading_and_shape_changes_have_their_lines() {
        assert_eq!(
            change_line(&Change::MeshLevel { from: 0, to: 2 }),
            Line::ReasonMeshLevel {
                from: "0".to_owned(),
                to: "2".to_owned(),
            }
        );
        assert_eq!(
            change_line(&Change::Grading {
                axis: Axis::Y,
                from: 1.0,
                to: 1.05,
            }),
            Line::ReasonGrading {
                axis: "y".to_owned(),
                from: "1".to_owned(),
                to: "1.05".to_owned(),
            }
        );
        assert_eq!(
            change_line(&Change::Shape {
                from: None,
                to: Some("cylinder, diameter 0.1 m at (0.2, 0.2) m".to_owned()),
            }),
            Line::ReasonShape {
                from: "none".to_owned(),
                to: "cylinder, diameter 0.1 m at (0.2, 0.2) m".to_owned(),
            }
        );
    }

    #[test]
    fn the_physics_thermal_and_profile_changes_have_their_lines() {
        assert_eq!(
            change_line(&Change::Physics {
                what: PhysicsPart::ViscosityRatio,
                from: "none".to_owned(),
                to: "10".to_owned(),
            }),
            Line::ReasonPhysics {
                what: "viscosity_ratio".to_owned(),
                from: "none".to_owned(),
                to: "10".to_owned(),
            }
        );
        assert_eq!(
            reason_text(&[Change::Thermal {
                boundary: "hot".to_owned(),
                from: None,
                to: Some("fixed 307.8 K".to_owned()),
            }]),
            "hot thermal condition none → fixed 307.8 K"
        );
        assert_eq!(
            reason_text(&[Change::InletProfile {
                boundary: "feed".to_owned(),
                from: Some("uniform".to_owned()),
                to: Some("parabolic".to_owned()),
            }]),
            "feed inlet profile uniform → parabolic"
        );
    }

    #[test]
    fn a_raw_override_change_reads_as_a_sentence() {
        let added = Change::RawOverride {
            id: "relax-u".to_owned(),
            from: None,
            to: Some("esi system/fvSolution relaxationFactors/equations/U = 0.5".to_owned()),
        };
        assert_eq!(
            reason_text(&[added]),
            "raw override relax-u none → esi system/fvSolution relaxationFactors/equations/U = 0.5"
        );
        let removed = Change::RawOverride {
            id: "relax-u".to_owned(),
            from: Some("esi system/fvSolution relaxationFactors/equations/U = 0.5".to_owned()),
            to: None,
        };
        assert_eq!(
            reason_text(&[removed]),
            "raw override relax-u esi system/fvSolution relaxationFactors/equations/U = 0.5 → none"
        );
        assert_eq!(
            reason_text(&[Change::RawMesh]),
            "a raw mesh override changed"
        );
    }

    #[test]
    fn a_monitor_change_reads_as_its_reason_line() {
        let added = Change::Monitor {
            id: "probe_6".to_owned(),
            from: None,
            to: Some("x 0.05 m, y 0.05 m, velocity".to_owned()),
        };
        assert_eq!(
            change_line(&added),
            Line::ReasonMonitor {
                id: "probe_6".to_owned(),
                from: "none".to_owned(),
                to: "x 0.05 m, y 0.05 m, velocity".to_owned(),
            }
        );
        let removed = Change::Monitor {
            id: "surface_1".to_owned(),
            from: Some("flux on movingWall".to_owned()),
            to: None,
        };
        assert_eq!(
            reason_text(&[removed]),
            "monitor surface_1 flux on movingWall → none"
        );
    }

    #[test]
    fn several_reasons_are_joined_with_a_semicolon_and_a_space() {
        let changes = [
            Change::EndTime {
                from: 5000.0,
                to: 6000.0,
            },
            Change::MeshChanged,
        ];
        assert_eq!(
            reason_text(&changes),
            "end time 5000 → 6000 s; its mesh changed"
        );
        assert_eq!(reason_parts(&changes).len(), 2);
    }

    #[test]
    fn stale_sentence_wraps_the_joined_reasons() {
        let changes = [
            Change::BoundaryVelocity {
                boundary: "lid".to_owned(),
                from: Some(0.1),
                to: Some(0.2),
            },
            Change::EndTime {
                from: 5000.0,
                to: 6000.0,
            },
        ];
        assert_eq!(
            stale_sentence(Stage::Result, &changes),
            "result stale because lid velocity 0.1 → 0.2 m/s; end time 5000 → 6000 s"
        );
    }
}
