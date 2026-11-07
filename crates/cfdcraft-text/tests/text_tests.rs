use cfdcraft_text::{Embedded, Line, SOURCES, Text};
use std::collections::BTreeSet;

fn migrated() -> Line {
    Line::Migrated {
        from: 0,
        to: 1,
        summary: "Moved the lid.".to_string(),
    }
}

#[test]
fn migrated_renders_in_english() {
    let out = Text::english().render(&migrated());
    assert_eq!(
        out,
        "This file was written by an older version (schema 0). It was updated to schema 1 in memory; the file on disk is unchanged until you save. Moved the lid."
    );
    assert!(!out.contains('{'));
}

#[test]
fn pseudo_marks_the_line_and_records_the_lookup() {
    let pseudo = Text::pseudo();
    let out = pseudo.render(&migrated());
    assert_ne!(out, Text::english().render(&migrated()));
    assert!(out.contains('0') && out.contains('1') && out.contains("Moved the lid."));
    assert!(pseudo.looked_up().contains(migrated().source()));
}

#[test]
fn english_text_ignores_the_machine_locale() {
    // Run the English check again in a child process whose locale variables name another language.
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "english_text_is_english", "--nocapture"])
        .env("LANG", "de_DE.UTF-8")
        .env("LC_ALL", "de_DE.UTF-8")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed"), "{stdout}");
}

#[test]
fn english_text_is_english() {
    let out = Text::english().render(&Line::Saved {
        path: "a".to_string(),
    });
    assert_eq!(out, "Saved a.");
}

#[test]
fn control_characters_in_a_value_never_reach_the_sentence() {
    let out = Text::english().render(&Line::Saved {
        path: "a\u{1b}[2J\r\u{7}\u{85}b".to_string(),
    });
    assert_eq!(out, "Saved a[2Jb.");
}

fn texts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_variant_renders_to_its_english_sentence() {
    let expected = texts(&[
        "This file was written by an older version (schema 0). It was updated to schema 1 in memory; the file on disk is unchanged until you save. Moved the lid.",
        "Saved a.cfdcraft.",
        "Opened a.cfdcraft.",
        "This file was written by a newer version of CFDCraft (schema 9). This version reads schemas up to 1, so nothing was read from the file.",
        "a.zip is not a CFDCraft study file: no manifest",
        "The linked file part.step is missing. It was expected at geometry/part.step.",
        "The linked file part.step has changed since it was linked.",
        "The link to part.step cannot be followed: it leaves the project folder",
        "Linked part.step as geometry/part.step.",
        "A stored result (ab12) is missing from the store next to the file.",
        "A stored result (ab12) does not match its recorded fingerprint.",
        "Packed 2 stored results into a.pack.",
        "Unpacked 2 stored results into a-dir.",
        "The pack does not include 2 linked files; copy them next to the unpacked study.",
        "a.cfdcraft already exists. Choose another name or remove it first.",
        "Stored result ab12 is not in the store, so the study cannot be packed.",
        "Stored result ab12 in the pack does not match its fingerprint. No study file was written.",
        "The pack is missing stored result ab12. No study file was written.",
        "a.cfdcraft could not be written: the disk is full",
        "part.step could not be linked: a link named part already exists",
        "result stale because lid velocity 0.1 → 0.2 m/s",
        "domain length in x 1 → 2 m",
        "the domain was opened",
        "the domain was closed",
        "cells in y 64 → 128",
        "the boundary layout changed",
        "viscosity 0.01 → 0.02 m²/s",
        "lid velocity 1 → 2 m/s",
        "lid kind wall → inlet",
        "lid faces top → top, left",
        "boundary lid was added",
        "boundary lid was removed",
        "end time 10 → 20 s",
        "write interval 1 → 2 s",
        "pressure residual limit 0.001 → 0.0001",
        "velocity residual limit 0.001 → 0.0001",
        "centreline samples 17 → 33",
        "solver image sha256:aa → sha256:bb",
        "solver dialect esi-v2512 → esi-v2606",
        "geometry sidecar lock none → lock-1",
        "CFDCraft version 0.1.0 → 0.2.0",
        "its mesh changed",
        "The mesh is up to date (made by run-1).",
        "No result has been made yet.",
        "The stored mesh (ab12) is missing from the store, so it counts as out of date.",
        "Recorded run-2: mesh, result.",
        "Nothing to run: the mesh and the result are up to date.",
        "run-3 reproduced run-1: every checked quantity is identical.",
        "run-3 is within tolerance of run-1; the largest difference is 1e-10.",
        "run-3 differs from run-1; quantities outside the tolerance: 2.",
        "This result was made with a different tool (solver image sha256:aa → sha256:bb), so it cannot be rerun as recorded.",
        "There is no run run-9 in this file.",
        "This file has no runs yet.",
        "run-1 recorded no result, so there is nothing to rerun or compare.",
        "The record of run-2 is inconsistent: its result key does not match its recipe",
        "cavity:mesh is up to date, so there is nothing to update.",
        "There is no item cavity; an item is written name:mesh or name:result.",
        "cavity:result is made from cavity:mesh, which is also out of date; update cavity:mesh too, and first.",
        "The order must list each selected item exactly once.",
        "cavity:result cannot come before cavity:mesh, which it is made from.",
        "Updated cavity:mesh in run-4.",
        "Nothing is out of date.",
        "This file was written by an older version (schema 1) and was saved at schema 2. Added runs.",
        "The output of the run could not be recorded: a result file is a link",
        "Two documents are called cavity; give them different file names.",
        "Created variant fast-lid of base.",
        "Updated variant fast-lid.",
        "Updated the name, pin or notes of fast-lid.",
        "There is no variant fast-lid in this file.",
        "The variants cannot be saved: a parent must come first",
        "The change to /mesh/cells_x in fast-lid does not apply to the study: there is no such setting",
        "fast-lid differs from base: lid velocity 0.1 → 0.2 m/s; cells in x 128 → 256.",
        "base and copy make the same case.",
        "raw override relax-u esi system/fvSolution relaxationFactors/equations/U = 0.7 → esi system/fvSolution relaxationFactors/equations/U = 0.5",
        "a raw mesh override changed",
        "Sweep grid would make 1000 points, more than the 200 allowed.",
        "Sweep wind cannot be made: a parameter has no values",
        "Created sweep wind: 9 variants.",
        "There is no sweep wind in this file.",
        "wind-004 fails the checks: cells in x is zero",
        "All 9 points of wind pass the checks; they need 3 different meshes.",
        "2 of 9 points of wind fail the checks; nothing was solved.",
        "The stored results use 4096 bytes across 3 variants.",
        "Nothing to prune.",
        "Pruning would remove 3 stored results and free 1024 bytes. Nothing was removed; run again with apply to remove them.",
        "Removed 3 stored results and freed 1024 bytes. The key numbers of every run are kept.",
        "The stored result of run-3 was pruned to free disk; its key numbers are kept.",
        "Pruning was refused: a variant is unknown",
        "A stored result could not be removed: permission denied",
        "Added 3 jobs to the queue; 3 are waiting.",
        "Nothing to add: everything asked for is up to date or already queued.",
        "The queue request was refused: the queue is full",
        "There is no job q-009 in the queue.",
        "Job q-003 is running, so it cannot be reordered.",
        "Moved q-004 to position 1 among the waiting jobs.",
        "Cancelled q-005.",
        "Asked q-003 to write its latest results and stop.",
        "Started the queue runner; it keeps running if you close this program.",
        "The queue runner is already running.",
        "The queue file cannot be used: a line is not valid",
        "q-003 is waiting.",
        "q-003 is starting on the compute host.",
        "q-003 is running: Time = 120",
        "q-003 is stopping (stop requested).",
        "q-003 finished and was recorded as run-4.",
        "q-003 stopped at time 1200 and was recorded as run-4; it can be continued.",
        "q-003 failed: the solver exited with an error",
        "q-005 was cancelled before it started.",
        "q-003 was interrupted: the queue runner was stopped",
        "The compute host cannot be reached; q-003 keeps running there and will be picked up again.",
        "Picked up q-003 again where it left off.",
        "Continuing run-4 from time 1200.",
        "The run cannot be continued: it is not a stopped run",
        "Running on 4 processes (40000 cells).",
        "run-4 is a stopped run (latest time 1200); it holds no finished result.",
        "The run diverged (CFD-RUN-004): Ux reached 5200, over the limit 1e4. It was stopped without writing the unstable fields.",
        "The solver stopped because a file it needs is missing (CFD-SOLVER-001). Export the case again, and check that every file the study refers to exists.",
        "The solution blew up (CFD-SOLVER-002): the numbers became unstable. Lower the relaxation factors, check the mesh quality, and check the boundary values.",
        "The solver does not know a name the case uses (CFD-SOLVER-003). Remove the raw override or monitor that uses it.",
        "A required entry is missing in a case file (CFD-SOLVER-004). Add the entry, or remove the override that replaced a whole block.",
        "A linear solver did not reach its tolerance (CFD-SOLVER-005). Tighten the settings, improve the mesh, or relax more.",
        "The mesh failed its quality checks (CFD-SOLVER-006). Refine it or fix the geometry, then run again.",
        "The solver ran out of memory (CFD-SOLVER-007). Use a smaller mesh, or ask for a host with more memory.",
        "The disk on the compute host is full (CFD-SOLVER-008). Free space there; the disk floor should have stopped the run earlier.",
        "The parallel decomposition does not match the case (CFD-SOLVER-009). Run the decomposition again for the same number of processes.",
        "The parallel run asked for more processes than the host allows (CFD-SOLVER-010). Lower the number of processes or raise the host's core cap.",
        "The solver crashed (CFD-SOLVER-011). Report it with the last lines of the log.",
        "The solver stopped with an error this program does not recognise (CFD-SOLVER-099). The last lines of its log:\ncannot open file system/fvSolution | FOAM exiting\nCheck the setup and the mesh, then run it again.",
        "The compute host has 8 GB free, below its floor of 20 GB, so the run was not started.",
        "The run was stopped because free disk on the compute host fell to 19 GB, the floor being 20 GB. Its latest results were kept.",
        "probe_1.Ux: last 0.0123, tail mean 0.0121, spread 0.0004: settled.",
        "settled",
        "drifting",
        "oscillating",
        "too early to tell",
        "too few samples to tell",
        "Mass balance: closes (imbalance 0.2 % of the inlet flux).",
        "Energy balance: does not close (imbalance 7 % of the reference).",
        "Converged: every monitored quantity has settled and the balances close.",
        "Not converged: monitor probe_1 drifting.",
        "Convergence cannot be judged: no monitors are defined.",
        "monitor probe_1 none → x 0.05 m, y 0.05 m, velocity",
    ]);
    let samples = Line::samples();
    assert_eq!(samples.len(), expected.len());
    let english = Text::english();
    for (line, want) in samples.iter().zip(&expected) {
        let got = english.render(line);
        assert_eq!(&got, want);
        assert!(!got.contains('{') && !got.contains('}'), "{got}");
    }
}

#[test]
fn plurals_use_the_right_form() {
    let english = Text::english();
    let packed = |count| {
        english.render(&Line::Packed {
            count,
            path: "out.pack".to_string(),
        })
    };
    assert_eq!(packed(1), "Packed 1 stored result into out.pack.");
    assert_eq!(packed(2), "Packed 2 stored results into out.pack.");
    assert_eq!(packed(0), "Packed 0 stored results into out.pack.");
    assert_eq!(
        packed(1234),
        "Packed 1234 stored results into out.pack.",
        "counts use plain digits, no grouping"
    );
    let unpacked = |count| {
        english.render(&Line::Unpacked {
            count,
            path: "dir".to_string(),
        })
    };
    assert_eq!(unpacked(1), "Unpacked 1 stored result into dir.");
    assert_eq!(unpacked(3), "Unpacked 3 stored results into dir.");
    let links = |count| english.render(&Line::LinksNotPacked { count });
    assert_eq!(
        links(1),
        "The pack does not include 1 linked file; copy it next to the unpacked study."
    );
    assert_eq!(
        links(2),
        "The pack does not include 2 linked files; copy them next to the unpacked study."
    );
}

#[test]
fn a_value_that_looks_like_a_placeholder_is_inserted_as_written() {
    let english = Text::english();
    let out = english.render(&Line::LinkChanged {
        name: "a{path}b".to_string(),
    });
    assert_eq!(
        out,
        "The linked file a{path}b has changed since it was linked."
    );
    let out = english.render(&Line::LinkAdded {
        name: "{path}".to_string(),
        path: "{name}".to_string(),
    });
    assert_eq!(out, "Linked {path} as {name}.");
    let out = english.render(&Line::Packed {
        count: 2,
        path: "{n}".to_string(),
    });
    assert_eq!(out, "Packed 2 stored results into {n}.");
}

#[test]
fn direction_marks_in_a_value_are_dropped() {
    let hostile = "a\u{202E}b\u{2067}c\u{200F}d";
    let out = Text::english().render(&Line::Saved {
        path: hostile.to_string(),
    });
    assert_eq!(out, "Saved abcd.");
}

#[test]
fn the_pseudo_locale_shows_every_source_was_looked_up() {
    let pseudo = Text::pseudo();
    let english = Text::english();
    for line in Line::samples() {
        let out = pseudo.render(&line);
        assert!(out.starts_with('\u{27E6}'), "{out}");
        assert_ne!(out, english.render(&line));
        assert!(!out.contains('{'), "{out}");
    }
    let looked_up = pseudo.looked_up();
    for line in Line::samples() {
        if line.plural_source().is_none() {
            assert!(looked_up.contains(line.source()), "{}", line.source());
        }
    }
}

#[test]
fn the_samples_cover_every_variant_and_every_source() {
    let samples = Line::samples();
    let kinds: BTreeSet<String> = samples
        .iter()
        .map(|l| {
            format!("{l:?}")
                .split([' ', '{'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds.len(), samples.len(), "one sample per variant");
    let mut from_samples: BTreeSet<&str> = BTreeSet::new();
    for line in &samples {
        from_samples.insert(line.source());
        if let Some(other) = line.plural_source() {
            from_samples.insert(other);
        }
    }
    let listed: BTreeSet<&str> = SOURCES.iter().copied().collect();
    assert_eq!(listed.len(), SOURCES.len(), "no source listed twice");
    assert_eq!(from_samples, listed);
}

static GOOD: &Embedded = &[("de", "", "\tSaved {path}.\tTESTDATA gespeichert: {path}\n")];
static BAD: &Embedded = &[("de", "", "\tOpened {path}.\tTESTDATA geoeffnet\n")];

#[test]
fn a_catalog_row_replaces_one_sentence() {
    let text = Text::with_catalogs(GOOD, &["de"], "de").unwrap();
    assert!(text.check_all().is_empty(), "{:?}", text.check_all());
    let saved = text.render(&Line::Saved {
        path: "a".to_string(),
    });
    assert_eq!(saved, "TESTDATA gespeichert: a");
    let opened = text.render(&Line::Opened {
        path: "a".to_string(),
    });
    assert_eq!(opened, "Opened a.");
}

#[test]
fn a_row_with_a_missing_placeholder_is_reported_and_skipped() {
    let text = Text::with_catalogs(BAD, &["de"], "de").unwrap();
    assert!(!text.check_all().is_empty());
    let opened = text.render(&Line::Opened {
        path: "a".to_string(),
    });
    assert_eq!(opened, "Opened a.");
}

#[test]
fn a_language_that_is_not_offered_gives_no_text() {
    assert!(Text::with_catalogs(GOOD, &[], "de").is_none());
    assert!(Text::with_catalogs(GOOD, &["de"], "fr").is_none());
}

#[test]
fn a_stale_result_reads_with_its_reason() {
    let english = Text::english();
    let reason = english.render(&Line::ReasonBoundaryVelocity {
        boundary: "lid".to_string(),
        from: "0.1".to_string(),
        to: "0.2".to_string(),
    });
    assert_eq!(reason, "lid velocity 0.1 → 0.2 m/s");
    let sentence = english.render(&Line::StaleBecause {
        stage: "result".to_string(),
        reason,
    });
    assert_eq!(sentence, "result stale because lid velocity 0.1 → 0.2 m/s");
}

#[test]
fn a_stale_mesh_reads_with_its_cell_change() {
    let english = Text::english();
    let reason = english.render(&Line::ReasonCells {
        axis: "x".to_string(),
        from: "128".to_string(),
        to: "256".to_string(),
    });
    assert_eq!(reason, "cells in x 128 → 256");
    let sentence = english.render(&Line::StaleBecause {
        stage: "mesh".to_string(),
        reason,
    });
    assert_eq!(sentence, "mesh stale because cells in x 128 → 256");
}

#[test]
fn a_reason_value_that_looks_like_a_placeholder_is_inserted_as_written() {
    let out = Text::english().render(&Line::ReasonBoundaryVelocity {
        boundary: "{from}".to_string(),
        from: "1".to_string(),
        to: "2".to_string(),
    });
    assert_eq!(out, "{from} velocity 1 → 2 m/s");
    let out = Text::english().render(&Line::ScenarioDiffers {
        from: "base".to_string(),
        to: "fast-lid".to_string(),
        reasons: "{from}".to_string(),
    });
    assert_eq!(out, "fast-lid differs from base: {from}.");
}

#[test]
fn a_saved_migration_says_it_was_saved() {
    let out = Text::english().render(&Line::MigratedAndSaved {
        from: 1,
        to: 2,
        summary: "Added runs.".to_string(),
    });
    assert_eq!(
        out,
        "This file was written by an older version (schema 1) and was saved at schema 2. Added runs."
    );
    assert!(!out.contains("unchanged"));
}

#[test]
fn the_summary_of_a_saved_migration_is_looked_up_like_the_other() {
    let pseudo = Text::pseudo();
    let line = Line::MigratedAndSaved {
        from: 1,
        to: 2,
        summary: "Added runs.".to_string(),
    };
    let out = pseudo.render(&line);
    assert!(out.contains("Added runs."), "{out}");
    assert!(pseudo.looked_up().contains("Added runs."));
    assert!(pseudo.looked_up().contains(line.source()));
}

#[test]
fn a_rerun_verdict_names_both_runs() {
    let out = Text::english().render(&Line::RerunReproduced {
        run: "run-3".to_string(),
        of: "run-1".to_string(),
    });
    assert_eq!(
        out,
        "run-3 reproduced run-1: every checked quantity is identical."
    );
}

#[test]
fn an_update_refusal_names_the_item_and_what_it_needs() {
    let out = Text::english().render(&Line::NeedsUpstream {
        item: "cavity:result".to_string(),
        needs: "cavity:mesh".to_string(),
    });
    assert_eq!(
        out,
        "cavity:result is made from cavity:mesh, which is also out of date; update cavity:mesh too, and first."
    );
}

#[test]
fn a_raw_override_change_reads_with_both_sides() {
    let english = Text::english();
    let side = "esi system/fvSolution relaxationFactors/equations/U";
    let out = english.render(&Line::ReasonRawOverride {
        id: "relax-u".to_string(),
        from: format!("{side} = 0.7"),
        to: format!("{side} = 0.5"),
    });
    assert_eq!(
        out,
        "raw override relax-u esi system/fvSolution relaxationFactors/equations/U = 0.7 → esi system/fvSolution relaxationFactors/equations/U = 0.5"
    );
    let added = english.render(&Line::ReasonRawOverride {
        id: "relax-u".to_string(),
        from: "none".to_string(),
        to: format!("{side} = 0.5"),
    });
    assert_eq!(
        added,
        "raw override relax-u none → esi system/fvSolution relaxationFactors/equations/U = 0.5"
    );
}

#[test]
fn a_mesh_override_change_has_its_own_sentence() {
    assert_eq!(
        Text::english().render(&Line::ReasonRawMesh),
        "a raw mesh override changed"
    );
}

#[test]
fn a_diff_reads_with_its_reasons() {
    let english = Text::english();
    let differs = english.render(&Line::ScenarioDiffers {
        from: "base".to_string(),
        to: "fast-lid".to_string(),
        reasons: "lid velocity 0.1 → 0.2 m/s; cells in x 128 → 256".to_string(),
    });
    assert_eq!(
        differs,
        "fast-lid differs from base: lid velocity 0.1 → 0.2 m/s; cells in x 128 → 256."
    );
    let same = english.render(&Line::ScenarioSame {
        from: "base".to_string(),
        to: "copy".to_string(),
    });
    assert_eq!(same, "base and copy make the same case.");
}

#[test]
fn a_dry_run_prune_says_nothing_was_removed() {
    let out = Text::english().render(&Line::PruneDryRun {
        blobs: "3".to_string(),
        bytes: "1024".to_string(),
    });
    assert_eq!(
        out,
        "Pruning would remove 3 stored results and free 1024 bytes. Nothing was removed; run again with apply to remove them."
    );
}

#[test]
fn an_applied_prune_says_the_key_numbers_are_kept() {
    let out = Text::english().render(&Line::Pruned {
        blobs: "3".to_string(),
        bytes: "1024".to_string(),
    });
    assert_eq!(
        out,
        "Removed 3 stored results and freed 1024 bytes. The key numbers of every run are kept."
    );
}

#[test]
fn a_pruned_stage_is_not_called_stale() {
    let out = Text::english().render(&Line::StagePruned {
        stage: "result".to_string(),
        run: "run-3".to_string(),
    });
    assert_eq!(
        out,
        "The stored result of run-3 was pruned to free disk; its key numbers are kept."
    );
    assert!(!out.contains("stale") && !out.contains("out of date"));
}

#[test]
fn pre_flight_sentences_name_the_point_and_the_count() {
    let english = Text::english();
    let failed = english.render(&Line::PreflightFailed {
        sweep: "wind".to_string(),
        failed: "2".to_string(),
        points: "9".to_string(),
    });
    assert_eq!(
        failed,
        "2 of 9 points of wind fail the checks; nothing was solved."
    );
    let passed = english.render(&Line::PreflightPassed {
        sweep: "wind".to_string(),
        points: "9".to_string(),
        meshes: "3".to_string(),
    });
    assert_eq!(
        passed,
        "All 9 points of wind pass the checks; they need 3 different meshes."
    );
    let point = english.render(&Line::PreflightPointFailed {
        point: "wind-004".to_string(),
        reason: "cells in x is zero".to_string(),
    });
    assert_eq!(point, "wind-004 fails the checks: cells in x is zero");
}

#[test]
fn a_sweep_that_is_too_large_says_how_large() {
    let out = Text::english().render(&Line::SweepTooLarge {
        sweep: "grid".to_string(),
        points: "1000".to_string(),
        max: "200".to_string(),
    });
    assert_eq!(
        out,
        "Sweep grid would make 1000 points, more than the 200 allowed."
    );
}
