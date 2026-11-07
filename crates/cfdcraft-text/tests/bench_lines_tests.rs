use cfdcraft_text::{Line, Text};

fn s(v: &str) -> String {
    v.to_string()
}

fn english(line: &Line) -> String {
    Text::english().render(line)
}

#[test]
fn the_gci_sentences_render() {
    let q = || s("bench.step.reattachment");
    let cases: Vec<(Line, &str)> = vec![
        (
            Line::GciSummary {
                quantity: q(),
                order: s("1.53"),
                gci_percent: s("2.17"),
                class: s("monotone"),
            },
            "Mesh independence of bench.step.reattachment: observed order 1.53, GCI of the fine mesh 2.17 % (monotone).",
        ),
        (
            Line::GciOrderNone {
                quantity: q(),
                class: s("divergent"),
            },
            "No grid convergence index for bench.step.reattachment: the three values are divergent, so no positive order of convergence exists.",
        ),
        (
            Line::GciWarnOscillatory { quantity: q() },
            "bench.step.reattachment converges in an oscillating way, so its index is less reliable than a monotone one.",
        ),
        (
            Line::GciWarnOrder {
                quantity: q(),
                order: s("3.1"),
            },
            "The observed order 3.1 of bench.step.reattachment is outside 0.5 to 2.5; check that the meshes are in the asymptotic range.",
        ),
        (
            Line::GciWarnAsymptotic {
                quantity: q(),
                ratio: s("1.3"),
            },
            "The asymptotic ratio 1.3 of bench.step.reattachment is outside 0.9 to 1.1; the meshes may not be in the asymptotic range.",
        ),
        (
            Line::GciWarnTransient {
                quantity: s("bench.cylinder.st"),
            },
            "bench.cylinder.st comes from a time-dependent run; the index does not include the error of sampling a finite number of cycles.",
        ),
        (
            Line::GciWarnWallFunction { quantity: q() },
            "This study uses wall functions, so the near-wall cell size changes with the mesh and the order of bench.step.reattachment is less reliable.",
        ),
        (
            Line::GciWarnNoise {
                quantity: s("bench.cylinder.cd_max"),
            },
            "The noise in bench.cylinder.cd_max is large against its change between meshes; the study is noise limited.",
        ),
        (
            Line::GciWarnNotRefined { quantity: q() },
            "The meshes for bench.step.reattachment are not refined by the same ratio each time.",
        ),
        (
            Line::GciCreated {
                sweep: s("levels"),
                points: s("3"),
            },
            "Created the mesh independence sweep levels with 3 meshes.",
        ),
        (
            Line::GciWaiting {
                sweep: s("levels"),
                missing: s("level-1, level-2"),
            },
            "The mesh independence sweep levels is not finished: level-1, level-2 have no recorded result.",
        ),
    ];
    assert_eq!(cases.len(), 11);
    for (line, want) in &cases {
        let got = english(line);
        assert_eq!(&got, want);
        assert!(!got.contains('{') && !got.contains('}'), "{got}");
        assert!(!got.contains('\t'), "{got}");
    }
}

#[test]
fn the_benchmark_sentences_render() {
    let cases: Vec<(Line, &str)> = vec![
        (
            Line::BenchmarkPassed {
                benchmark: s("step"),
                metrics: s("2"),
            },
            "step: every metric is within its tolerance (2 checked).",
        ),
        (
            Line::BenchmarkFailed {
                benchmark: s("step"),
                failed: s("reattachment"),
            },
            "step: outside tolerance on reattachment.",
        ),
        (
            Line::BenchmarkNotMatching {
                benchmark: s("step"),
                differences: s("the step height differs"),
            },
            "This study is not the step benchmark: the step height differs.",
        ),
        (
            Line::BenchmarkListed {
                id: s("step"),
                title: s("Backward-facing step"),
                reference: s("Driver and Seegmiller 1985"),
            },
            "step: Backward-facing step (Driver and Seegmiller 1985).",
        ),
        (
            Line::BenchmarkCreated {
                benchmark: s("step"),
                mesh: s("medium"),
                path: s("step.json"),
            },
            "Wrote the step study at the medium mesh to step.json.",
        ),
        (
            Line::BenchmarkResultsIncomplete {
                benchmark: s("step"),
                file: s("log.simpleFoam"),
            },
            "The results lack log.simpleFoam, which the step comparison needs.",
        ),
        (
            Line::BenchmarkOverrideUsed {
                benchmark: s("step"),
            },
            "A tolerance override was used for step; the table shows the tolerance in force.",
        ),
        (
            Line::BenchmarkNotConverged {
                benchmark: s("step"),
                reasons: s("not_settled:floor, residuals_above_limit"),
            },
            "Warning: the run behind this step verdict is NOT CONVERGED (not_settled:floor, residuals_above_limit). The verdict compares the numbers as they stand; it does not say they have settled.",
        ),
        (
            Line::BenchmarkConvergenceUnknown {
                benchmark: s("step"),
                reasons: s("unknown: the run recorded no assessment"),
            },
            "Warning: whether the run behind this step verdict had settled is UNKNOWN (unknown: the run recorded no assessment). The verdict compares the numbers as they stand; it does not say they have settled.",
        ),
    ];
    assert_eq!(cases.len(), 9);
    for (line, want) in &cases {
        let got = english(line);
        assert_eq!(&got, want);
        assert!(!got.contains('{') && !got.contains('}'), "{got}");
        assert!(!got.contains('\t'), "{got}");
    }
}

#[test]
fn a_hostile_value_cannot_forge_a_sentence() {
    let out = english(&Line::GciWarnOscillatory {
        quantity: s("a\u{1b}[2J\r{quantity}\u{7}b"),
    });
    assert_eq!(
        out,
        "a[2J{quantity}b converges in an oscillating way, so its index is less reliable than a monotone one."
    );
    let out = english(&Line::GciSummary {
        quantity: s("{order}"),
        order: s("1.5"),
        gci_percent: s("{class}"),
        class: s("monotone"),
    });
    assert_eq!(
        out,
        "Mesh independence of {order}: observed order 1.5, GCI of the fine mesh {class} % (monotone)."
    );
}

#[test]
fn the_reason_sentences_render() {
    let cases: Vec<(Line, &str)> = vec![
        (
            Line::ReasonMeshLevel {
                from: s("0"),
                to: s("1"),
            },
            "mesh level 0 → 1",
        ),
        (
            Line::ReasonGrading {
                axis: s("x"),
                from: s("1"),
                to: s("1.2"),
            },
            "grading in x 1 → 1.2",
        ),
        (
            Line::ReasonShape {
                from: s("box"),
                to: s("backward step"),
            },
            "domain shape box → backward step",
        ),
        (
            Line::ReasonInletProfile {
                boundary: s("feed"),
                from: s("uniform"),
                to: s("parabolic"),
            },
            "feed inlet profile uniform → parabolic",
        ),
        (
            Line::ReasonPhysics {
                what: s("turbulence"),
                from: s("laminar"),
                to: s("k-omega SST"),
            },
            "turbulence laminar → k-omega SST",
        ),
        (
            Line::ReasonThermal {
                boundary: s("hot"),
                from: s("none"),
                to: s("fixed 307.8 K"),
            },
            "hot thermal condition none → fixed 307.8 K",
        ),
    ];
    assert_eq!(cases.len(), 6);
    for (line, want) in &cases {
        let got = english(line);
        assert_eq!(&got, want);
        assert!(!got.contains('{') && !got.contains('}'), "{got}");
    }
}

#[test]
fn a_diff_with_the_new_reasons_reads_in_one_line() {
    let level = english(&Line::ReasonMeshLevel {
        from: s("0"),
        to: s("2"),
    });
    let shape = english(&Line::ReasonShape {
        from: s("box"),
        to: s("cylinder channel"),
    });
    let out = english(&Line::ScenarioDiffers {
        from: s("base"),
        to: s("fine"),
        reasons: format!("{level}; {shape}"),
    });
    assert_eq!(
        out,
        "fine differs from base: mesh level 0 → 2; domain shape box → cylinder channel."
    );
    let stale = english(&Line::StaleBecause {
        stage: s("mesh"),
        reason: format!("{level}; {shape}"),
    });
    assert_eq!(
        stale,
        "mesh stale because mesh level 0 → 2; domain shape box → cylinder channel"
    );
}
