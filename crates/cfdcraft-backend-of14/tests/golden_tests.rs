//! The benchmarks and the graded box compile to the files of their golden folders,
//! deterministically.
//!
//! A difference prints the whole actual text of every differing file, so it can be pasted into the
//! golden file as it is. A golden file that still holds the placeholder text is a difference.

mod common;

use common::{
    cavity, check_golden, cylinder, cylinder_with_every_monitor, graded_box, heated, sst_step, step,
};

#[test]
fn the_cavity_matches_its_golden() {
    check_golden("cavity-of14", &cavity());
}

#[test]
fn the_step_is_deterministic_and_golden() {
    check_golden("step-laminar-of14", &step());
}

#[test]
fn the_heated_cavity_matches_its_golden() {
    check_golden("heated-of14", &heated());
}

#[test]
fn the_cylinder_matches_its_golden() {
    check_golden("cylinder-of14", &cylinder());
}

#[test]
fn the_cylinder_with_every_monitor_matches_its_golden() {
    check_golden("cylinder-monitors-of14", &cylinder_with_every_monitor());
}

#[test]
fn the_sst_step_matches_its_golden() {
    check_golden("step-sst-of14", &sst_step());
}

#[test]
fn the_graded_box_matches_its_golden() {
    check_golden("graded-box-of14", &graded_box());
}
