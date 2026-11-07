//! Success criteria 2 and 4 of the benchmark suite: steady or transient, each of the four
//! turbulence models and a uniform or potential-flow start compile for every template where they
//! apply, each template's default is the one preselected, the heated cavity is buoyant, and a
//! potential-flow start adds its solver block and its `Allrun` step and is refused where it does
//! not apply. Every case goes through the whole chain (the model's checks, the compiler, the
//! safety scan and the read-back check). Nothing here runs a solver.

use std::collections::BTreeSet;
use std::path::Path;

use cfdcraft_backend_esi::case::CaseOptions;
use cfdcraft_backend_esi::guard::scan_tree;
use cfdcraft_backend_esi::patches::{layout_inputs, layout_of};
use cfdcraft_backend_esi::{
    Dialect, EvidenceExpectations, EvidencePlan, RunEvidence, SolverPlan, Verdict, judge_run_with,
};
use cfdcraft_model::{InitMode, ResolvedStudy, ResolvedTime, TurbulenceModel, resolve};
use cfdcraft_ops::prepare::prepare_with_options;
use cfdcraft_ops::{BenchmarkOptions, ExitClass, Prepared, benchmark_create, prepare};
use cfdcraft_shapes::problems;
use cfdcraft_templates::{MeshPreset, TemplateOptions, build as template, list};
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::edit;

const TEMPLATES: [&str; 5] = [
    "lid-cavity",
    "backward-step",
    "backward-step-laminar",
    "cylinder",
    "heated-cavity",
];

/// Every choice of a word for each option, with the choice of leaving the option out.
const TURBULENCE: [Option<&str>; 5] = [
    None,
    Some("laminar"),
    Some("k-omega-sst"),
    Some("realizable-ke"),
    Some("rng-k-epsilon"),
];
const TIME: [Option<&str>; 3] = [None, Some("steady"), Some("transient")];
const INIT: [Option<&str>; 3] = [None, Some("uniform"), Some("potential")];

/// A template and the words chosen for turbulence, time and the start.
type Key = (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

fn describe(key: &Key) -> String {
    let word = |w: Option<&str>| w.unwrap_or("default").to_owned();
    format!(
        "{} turbulence={} time={} init={}",
        key.0,
        word(key.1),
        word(key.2),
        word(key.3)
    )
}

/// What came of asking for a template with some options.
enum Made {
    /// The template refused the options, with this sentence.
    Refused(String),
    /// A study the whole chain accepted.
    Case(Box<Prepared>),
    /// A study the chain refused, with the report text.
    Broken(String),
}

fn words(
    mesh: &str,
    turbulence: Option<&str>,
    time: Option<&str>,
    init: Option<&str>,
) -> BenchmarkOptions {
    let owned = |word: Option<&str>| word.map(str::to_owned);
    BenchmarkOptions {
        mesh: Some(mesh.to_owned()),
        turbulence: owned(turbulence),
        time: owned(time),
        init: owned(init),
        ..BenchmarkOptions::default()
    }
}

/// The study of a template for these options through `benchmark_create`, then the whole chain.
fn make(
    id: &str,
    mesh: &str,
    turbulence: Option<&str>,
    time: Option<&str>,
    init: Option<&str>,
) -> Made {
    let (report, text) = benchmark_create(id, &words(mesh, turbulence, time, init));
    let Some(text) = text else {
        let sentence = report
            .diagnostics
            .first()
            .map_or_else(|| report.render_text(), |d| d.message.clone());
        return Made::Refused(sentence);
    };
    match prepare(&text, Dialect::EsiV2512) {
        Ok(prepared) => Made::Case(Box::new(prepared)),
        Err(report) => Made::Broken(report.render_text()),
    }
}

/// Every template at the coarse mesh with every combination of the three options.
fn grid() -> Vec<(Key, Made)> {
    let mut all = Vec::new();
    for id in TEMPLATES {
        for turbulence in TURBULENCE {
            for time in TIME {
                for init in INIT {
                    let made = make(id, "coarse", turbulence, time, init);
                    all.push(((id, turbulence, time, init), made));
                }
            }
        }
    }
    all
}

/// The text of a file of the exported case: the dictionaries, `Allrun` and the README.
fn file(prepared: &Prepared, name: &str) -> Option<String> {
    prepared
        .export_tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .map(|f| String::from_utf8_lossy(&f.bytes).into_owned())
}

fn has_line(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line.trim() == wanted)
}

/// Whether `parts` occur in `text` one after the other, in this order.
fn in_order(text: &str, parts: &[&str]) -> bool {
    let mut at = 0;
    for part in parts {
        match text[at..].find(*part) {
            Some(found) => at += found + part.len(),
            None => return false,
        }
    }
    true
}

fn model_name(model: TurbulenceModel) -> &'static str {
    match model {
        TurbulenceModel::Laminar => "laminar",
        TurbulenceModel::KOmegaSst => "k-omega SST",
        TurbulenceModel::RealizableKe => "realizable k-epsilon",
        TurbulenceModel::RngKEpsilon => "RNG k-epsilon",
    }
}

fn model_word(model: TurbulenceModel) -> &'static str {
    match model {
        TurbulenceModel::Laminar => "laminar",
        TurbulenceModel::KOmegaSst => "k-omega-sst",
        TurbulenceModel::RealizableKe => "realizable-ke",
        TurbulenceModel::RngKEpsilon => "rng-k-epsilon",
    }
}

/// The physics a study has as the user sees it: the model, steady or transient, buoyancy and the
/// start.
struct Choice {
    model: TurbulenceModel,
    transient: bool,
    buoyant: bool,
    potential: bool,
}

fn choice_of(r: &ResolvedStudy) -> Choice {
    match &r.physics {
        None => Choice {
            model: TurbulenceModel::Laminar,
            transient: false,
            buoyant: false,
            potential: false,
        },
        Some(p) => Choice {
            model: p.turbulence.model,
            transient: matches!(p.time, ResolvedTime::Transient { .. }),
            buoyant: p.buoyancy.is_some(),
            potential: p.init == InitMode::Potential,
        },
    }
}

/// The words of the list of templates for a choice: `laminar, steady, Boussinesq buoyancy`.
fn defaults_text(c: &Choice) -> String {
    let time = if c.transient { "transient" } else { "steady" };
    let mut text = format!("{}, {time}", model_name(c.model));
    if c.buoyant {
        text.push_str(", Boussinesq buoyancy");
    }
    text
}

#[test]
fn sc2_every_option_compiles_and_the_defaults_are_preselected() {
    let mut failures: Vec<String> = Vec::new();
    let mut refused: BTreeSet<Key> = BTreeSet::new();
    for (key, made) in grid() {
        let name = describe(&key);
        match made {
            Made::Refused(sentence) => {
                if !sentence.contains("init") {
                    failures.push(format!(
                        "{name}: the refusal does not name the option: {sentence}"
                    ));
                }
                refused.insert(key);
            }
            Made::Broken(detail) => failures.push(format!("{name}: {detail}")),
            Made::Case(prepared) => {
                if prepared
                    .warnings
                    .iter()
                    .any(|d| d.code.starts_with("CFD-RT-"))
                {
                    failures.push(format!(
                        "{name}: the read-back check warned: {:?}",
                        prepared.warnings
                    ));
                }
            }
        }
    }
    // Every template at every preset with its own physics.
    for id in TEMPLATES {
        for mesh in ["coarse", "medium", "fine"] {
            match make(id, mesh, None, None, None) {
                Made::Case(_) => {}
                Made::Refused(sentence) => {
                    failures.push(format!("{id} at {mesh}: refused: {sentence}"))
                }
                Made::Broken(detail) => failures.push(format!("{id} at {mesh}: {detail}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} combinations failed:\n{}",
        failures.len(),
        failures.join("\n")
    );

    // The refused set is exactly the potential-flow start of the closed heated cavity.
    let mut expected: BTreeSet<Key> = BTreeSet::new();
    for turbulence in TURBULENCE {
        for time in TIME {
            expected.insert(("heated-cavity", turbulence, time, Some("potential")));
        }
    }
    assert_eq!(refused, expected);
}

#[test]
fn sc2_each_template_preselects_its_default() {
    // Each template's default, as the criterion states it.
    let expected = [
        ("lid-cavity", TurbulenceModel::Laminar, false, false),
        ("backward-step", TurbulenceModel::KOmegaSst, false, false),
        (
            "backward-step-laminar",
            TurbulenceModel::Laminar,
            false,
            false,
        ),
        ("cylinder", TurbulenceModel::Laminar, true, false),
        ("heated-cavity", TurbulenceModel::Laminar, false, true),
    ];
    let listed = list();
    for (id, model, transient, buoyant) in expected {
        let study = template(id, &TemplateOptions::default()).expect("a study");
        let r = resolve(&study).expect("no findings");
        let c = choice_of(&r);
        assert_eq!(c.model, model, "{id}");
        assert_eq!(c.transient, transient, "{id}");
        assert_eq!(c.buoyant, buoyant, "{id}");
        assert!(!c.potential, "{id}: the start is uniform unless asked");

        // The list of templates says the same words.
        let info = listed.iter().find(|t| t.id == id).expect("listed");
        assert_eq!(info.defaults, defaults_text(&c), "{id}");

        // Choosing exactly those words by hand makes the very same study.
        let (_, plain) = benchmark_create(id, &words("medium", None, None, None));
        let time = if transient { "transient" } else { "steady" };
        let chosen = words(
            "medium",
            Some(model_word(model)),
            Some(time),
            Some("uniform"),
        );
        let (_, by_hand) = benchmark_create(id, &chosen);
        assert!(plain.is_some(), "{id}");
        assert_eq!(plain, by_hand, "{id}");
    }
}

#[test]
fn sc2_the_solver_follows_the_physics() {
    // The word, the model line, the field the model adds and the field it does not.
    let models = [
        ("laminar", None),
        ("k-omega-sst", Some(("kOmegaSST", "omega", "epsilon"))),
        ("realizable-ke", Some(("realizableKE", "epsilon", "omega"))),
        ("rng-k-epsilon", Some(("RNGkEpsilon", "epsilon", "omega"))),
    ];
    let mut failures: Vec<String> = Vec::new();
    for id in TEMPLATES {
        let buoyant = id == "heated-cavity";
        for (time, transient) in [("steady", false), ("transient", true)] {
            for (word, ras) in models {
                let name = format!("{id} {word} {time}");
                let Made::Case(prepared) = make(id, "coarse", Some(word), Some(time), None) else {
                    failures.push(format!("{name}: not a case"));
                    continue;
                };
                let binary = match (buoyant, transient) {
                    (false, false) => "simpleFoam",
                    (false, true) => "pimpleFoam",
                    (true, false) => "buoyantBoussinesqSimpleFoam",
                    (true, true) => "buoyantBoussinesqPimpleFoam",
                };
                let allrun = file(&prepared, "Allrun").unwrap_or_default();
                let wanted = format!("{binary} > log.{binary} 2>&1 || exit 1");
                if allrun.lines().next_back() != Some(wanted.as_str()) {
                    failures.push(format!(
                        "{name}: the last step of Allrun is not {wanted}: {allrun}"
                    ));
                }
                let plan = SolverPlan::for_study(&prepared.resolved);
                if plan.binary != binary || plan.buoyant != buoyant || plan.transient != transient {
                    failures.push(format!("{name}: the solver plan is {plan:?}"));
                }

                let properties =
                    file(&prepared, "constant/turbulenceProperties").unwrap_or_default();
                let exists = |field: &str| file(&prepared, &format!("0/{field}")).is_some();
                match ras {
                    None => {
                        if !has_line(&properties, "simulationType laminar;") {
                            failures.push(format!("{name}: the case is not laminar"));
                        }
                        for field in ["k", "omega", "epsilon", "nut"] {
                            if exists(field) {
                                failures.push(format!("{name}: a laminar case writes 0/{field}"));
                            }
                        }
                    }
                    Some((model, present, absent)) => {
                        let line = format!("RASModel {model};");
                        if !has_line(&properties, "simulationType RAS;")
                            || !has_line(&properties, &line)
                        {
                            failures.push(format!("{name}: the dictionary lacks {line}"));
                        }
                        for field in ["k", "nut", present] {
                            if !exists(field) {
                                failures.push(format!("{name}: no 0/{field}"));
                            }
                        }
                        if exists(absent) {
                            failures.push(format!("{name}: 0/{absent} is written"));
                        }
                    }
                }

                for needed in ["constant/g", "0/T", "0/p_rgh"] {
                    if file(&prepared, needed).is_some() != buoyant {
                        failures.push(format!("{name}: {needed} is written only with buoyancy"));
                    }
                }
                if buoyant {
                    let temperature = file(&prepared, "0/T").unwrap_or_default();
                    let mesh = file(&prepared, "system/blockMeshDict").unwrap_or_default();
                    for patch in ["isothermal1", "isothermal2"] {
                        if !temperature.contains(patch) || !mesh.contains(patch) {
                            failures.push(format!("{name}: the thermal patch {patch} is missing"));
                        }
                    }
                    if !temperature.contains("zeroGradient") {
                        failures.push(format!("{name}: the insulated walls are not zero gradient"));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} checks failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A prepared case of a template with a start, a time choice and a number of processes.
fn started(id: &str, time: &str, init: Option<&str>, np: u32) -> Prepared {
    let (report, text) = benchmark_create(id, &words("coarse", None, Some(time), init));
    let text = text.unwrap_or_else(|| panic!("{id}: {}", report.render_text()));
    match prepare_with_options(&text, Dialect::EsiV2512, &CaseOptions { np }) {
        Ok(prepared) => prepared,
        Err(report) => panic!("{id} {time} np={np}: {}", report.render_text()),
    }
}

#[test]
fn sc4_potential_flow_initialises_a_run() {
    let mut failures: Vec<String> = Vec::new();
    for id in [
        "lid-cavity",
        "backward-step",
        "backward-step-laminar",
        "cylinder",
    ] {
        for time in ["steady", "transient"] {
            for np in [1_u32, 2] {
                let name = format!("{id} {time} np={np}");
                let potential = started(id, time, Some("potential"), np);
                let plain = started(id, time, None, np);
                let plan = SolverPlan::for_study(&potential.resolved);
                let binary = plan.binary.clone();

                // The step comes after the mesh checks and before the solver, and before the
                // decomposition of a parallel case.
                let allrun = file(&potential, "Allrun").unwrap_or_default();
                let step = "potentialFoam -initialiseUBCs > log.potentialFoam";
                let solver = if np == 1 {
                    format!("{binary} > log.{binary}")
                } else {
                    format!("mpirun -np {np} {binary} -parallel > log.{binary}")
                };
                let mut order = vec!["checkMesh > log.checkMesh", step];
                if np > 1 {
                    order.push("decomposePar -force > log.decomposePar");
                }
                order.push(solver.as_str());
                if !in_order(&allrun, &order) {
                    failures.push(format!(
                        "{name}: the steps of Allrun are out of order: {allrun}"
                    ));
                }

                // The solver block and the dictionary of the step are written.
                let solution = file(&potential, "system/fvSolution").unwrap_or_default();
                if !solution.contains("    Phi\n    {\n") {
                    failures.push(format!("{name}: fvSolution has no Phi solver"));
                }
                if !solution.contains("potentialFlow\n{\n") {
                    failures.push(format!("{name}: fvSolution has no potentialFlow block"));
                }

                // The run is judged on the log of the step as well.
                let logs = ["log.blockMesh", "log.checkMesh", "log.potentialFoam"];
                let judged = plan.judged_logs();
                if !plan.potential || judged != [logs[0], logs[1], logs[2], plan.log_file.as_str()]
                {
                    failures.push(format!("{name}: the judged logs are {judged:?}"));
                }

                // Without the start none of it appears.
                let allrun = file(&plain, "Allrun").unwrap_or_default();
                let solution = file(&plain, "system/fvSolution").unwrap_or_default();
                let plain_plan = SolverPlan::for_study(&plain.resolved);
                if allrun.contains("potentialFoam")
                    || solution.contains("Phi")
                    || solution.contains("potentialFlow")
                    || plain_plan.potential
                    || plain_plan
                        .judged_logs()
                        .iter()
                        .any(|log| log.contains("potential"))
                {
                    failures.push(format!(
                        "{name}: a uniform start shows the potential-flow step"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} checks failed:\n{}",
        failures.len(),
        failures.join("\n")
    );

    // A closed buoyant study with the start written into the study file is refused by the model.
    let (report, text) = benchmark_create("heated-cavity", &words("coarse", None, None, None));
    let text = text.unwrap_or_else(|| panic!("{}", report.render_text()));
    let edited = edit(&text, |study| {
        study["physics"]["init"] = json!("potential");
    });
    let Err(refusal) = prepare(&edited, Dialect::EsiV2512) else {
        panic!("the heated cavity accepted a potential-flow start");
    };
    assert_eq!(refusal.class(), ExitClass::StudyInvalid);
    assert!(
        refusal.diagnostics.iter().any(|d| d.code == "CFD-PHYS-005"),
        "{}",
        refusal.render_text()
    );

    // And the template refuses the word, naming the option.
    let (report, text) = benchmark_create(
        "heated-cavity",
        &words("coarse", None, None, Some("potential")),
    );
    assert!(text.is_none());
    assert!(report.diagnostics[0].message.contains("init"));
}

#[test]
fn sc4_the_potential_flow_log_must_end() {
    // The lid-driven cavity at the coarse mesh: 32 by 32 cells, 0.1 m by 0.1 m by 0.05 m.
    let prepared = started("lid-cavity", "steady", Some("potential"), 1);
    let plan = SolverPlan::for_study(&prepared.resolved);
    let end = prepared.resolved.solver.end_time;
    let solver_log = format!(
        "Time = {end}\n\nsmoothSolver:  Solving for Ux, Initial residual = 1e-8, Final residual = 1e-11, No Iterations 1\nsmoothSolver:  Solving for Uy, Initial residual = 1e-8, Final residual = 1e-11, No Iterations 1\nsmoothSolver:  Solving for p, Initial residual = 1e-8, Final residual = 1e-11, No Iterations 1\nExecutionTime = 1 s  ClockTime = 1 s\n\nEnd\n"
    );
    let times = [0.0, end];
    let evidence = RunEvidence {
        block_mesh_log: "Created 1024 cells\nEnd\n",
        check_mesh_log: "cells: 1024\nTotal volume = 0.0005\nMesh OK.\nEnd\n",
        solver_log: &solver_log,
        time_directories: &times,
    };
    let expected = EvidenceExpectations {
        end_time: end,
        cell_count: 1024,
        domain_volume: 0.0005,
        p_threshold: 1e-6,
        u_threshold: 1e-6,
    };
    let judge = |potential_log: Option<&str>| {
        let judged = EvidencePlan {
            potential_log: potential_log.map(str::to_owned),
            ..plan.evidence_plan()
        };
        judge_run_with(&evidence, &expected, &judged)
    };

    assert_eq!(
        judge(Some("Calculating potential flow\nEnd\n")),
        Verdict::Passed
    );
    let Verdict::Failed(reasons) = judge(Some("Calculating potential flow\n")) else {
        panic!("a log that does not end was accepted");
    };
    assert!(
        reasons.contains(&"potentialFoam: missing terminal End marker".to_owned()),
        "{reasons:?}"
    );
    let Verdict::Failed(reasons) = judge(Some("--> FOAM FATAL ERROR:\nEnd\n")) else {
        panic!("a fatal log was accepted");
    };
    assert!(
        reasons.contains(&"potentialFoam: fatal signal FOAM FATAL".to_owned()),
        "{reasons:?}"
    );
}

/// The words of every `libs` entry of a dictionary.
fn libs_of(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("libs") else {
            continue;
        };
        if !rest.starts_with([' ', '(']) {
            continue;
        }
        let inside = rest
            .trim()
            .trim_end_matches(';')
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')');
        found.extend(
            inside
                .split_whitespace()
                .map(|w| w.trim_matches('"').to_owned()),
        );
    }
    found
}

#[test]
fn no_template_case_holds_a_directive_a_coded_type_or_an_unlisted_library() {
    const ALLOWED: [&str; 6] = [
        "sampling",
        "libsampling.so",
        "fieldFunctionObjects",
        "libfieldFunctionObjects.so",
        "forces",
        "libforces.so",
    ];
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0_usize;
    let mut with_libs = 0_usize;
    for (key, made) in grid() {
        let Made::Case(prepared) = made else {
            continue;
        };
        let name = describe(&key);
        checked += 1;
        let refusals = scan_tree(&prepared.tree);
        if !refusals.is_empty() {
            failures.push(format!("{name}: the safety scan found {refusals:?}"));
        }
        for case_file in &prepared.tree.files {
            let path = case_file.rel_path.display();
            let text = String::from_utf8_lossy(&case_file.bytes);
            if text.contains('#') {
                failures.push(format!("{name}: {path} holds a #"));
            }
            let coded = text
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| word.to_ascii_lowercase().starts_with("code"));
            if coded {
                failures.push(format!("{name}: {path} holds a word that starts with code"));
            }
            for lib in libs_of(&text) {
                with_libs += 1;
                if !ALLOWED.contains(&lib.as_str()) {
                    failures.push(format!("{name}: {path} loads {lib}"));
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} cases were checked");
    assert!(
        with_libs > 0,
        "no case loads a library, so nothing was checked"
    );
    assert!(
        failures.is_empty(),
        "{} problems:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_mesh_of_every_shaped_case_is_sound() {
    // The step with a start-up section, the step without one, the cylinder and the heated
    // cavity, and the box of the cavity, at every preset.
    for id in TEMPLATES {
        for mesh in MeshPreset::ALL {
            let options = TemplateOptions {
                mesh,
                ..TemplateOptions::default()
            };
            let name = format!("{id} at {mesh:?}");
            let study = template(id, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
            let r = resolve(&study).unwrap_or_else(|e| panic!("{name}: {e}"));
            let layout = layout_of(&r).unwrap_or_else(|e| panic!("{name}: {e}"));
            let (domain, shape, _) = layout_inputs(&r);
            let found = problems(&layout, &domain, &shape);
            assert!(found.is_empty(), "{name}: {found:?}");
            assert_eq!(r.cell_count(), Some(layout.cell_count), "{name}");
        }
    }
}
