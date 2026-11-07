//! The turbulence models, the potential-flow start, the graded box and the widened capabilities.

mod common;

use cfdcraft_backend::{Backend, CompileOptions};
use cfdcraft_backend_esi::{Dialect, compile as compile_esi};
use cfdcraft_model::{
    InitMode, RawBackend, RawOverride, ResolvedStudy, TurbulenceModel, diagnose, resolve,
};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice};
use common::{
    authored, backend, cavity, compiled, cylinder, graded_box, heated, names, options, sst_step,
    step, text, turbulent_step,
};

fn row() -> &'static dyn Backend {
    backend()
}

#[test]
fn each_model_writes_its_momentum_transport_and_fields() {
    for (model, word, second, other) in [
        (TurbulenceModel::KOmegaSst, "kOmegaSST", "omega", "epsilon"),
        (
            TurbulenceModel::RealizableKe,
            "realizableKE",
            "epsilon",
            "omega",
        ),
        (
            TurbulenceModel::RngKEpsilon,
            "RNGkEpsilon",
            "epsilon",
            "omega",
        ),
    ] {
        let study = turbulent_step(model, None);
        let tree = compiled(&study);
        let transport = text(&tree, "constant/momentumTransport");
        assert!(transport.contains("simulationType RAS;"), "{word}");
        assert!(transport.contains(&format!("model {word};")), "{word}");
        assert!(transport.contains("turbulence on;"), "{word}");
        let files = names(&tree);
        let has = |name: &str| files.contains(&format!("0/{name}"));
        assert!(has("k") && has("nut") && has(second), "{word}: {files:?}");
        assert!(!has(other), "{word}: {files:?}");
        // The wall functions and inflow values are the older writer's for the same study.
        let older = compile_esi(&study, Dialect::EsiV2512).unwrap();
        for field in ["k", "nut", second] {
            let name = format!("0/{field}");
            assert_eq!(text(&tree, &name), text(&older, &name), "{word} {name}");
        }
        assert_eq!(
            row().run_plan(&study, 1).final_fields,
            ["U", "p", "k", second, "nut"]
        );
        assert_eq!(row().read_back(&study, &tree), vec![], "{word}");
        assert!(row().scan(&tree).is_empty(), "{word}");
    }
    let laminar = compiled(&step());
    assert!(text(&laminar, "constant/momentumTransport").contains("simulationType laminar;"));
    for field in ["k", "omega", "epsilon", "nut"] {
        assert!(!names(&laminar).contains(&format!("0/{field}")));
    }
}

#[test]
fn the_potential_start_adds_the_standalone_step() {
    let study = turbulent_step(TurbulenceModel::KOmegaSst, Some(InitMode::Potential));
    let plan = row().run_plan(&study, 1);
    let steps: Vec<(&str, &str)> = plan
        .steps
        .iter()
        .map(|s| (s.command.as_str(), s.log.as_str()))
        .collect();
    assert_eq!(
        steps,
        [
            ("blockMesh", "log.blockMesh"),
            ("checkMesh", "log.checkMesh"),
            ("potentialFoam -initialiseUBCs", "log.potentialFoam"),
            ("foamRun", "log.foamRun"),
        ]
    );
    assert!(plan.potential);
    assert_eq!(
        plan.judged_logs,
        [
            "log.blockMesh",
            "log.checkMesh",
            "log.potentialFoam",
            "log.foamRun"
        ]
    );
    let tree = compiled(&study);
    let solution = text(&tree, "system/fvSolution");
    assert!(solution.contains("potentialFlow"));
    assert!(solution.contains("Phi"));
    assert_eq!(row().read_back(&study, &tree), vec![]);
    // In parallel the potential step runs before the decomposition.
    let commands: Vec<String> = row()
        .run_plan(&study, 2)
        .steps
        .into_iter()
        .map(|s| s.command)
        .collect();
    assert_eq!(commands[2], "potentialFoam -initialiseUBCs");
    assert_eq!(commands[3], "decomposePar -force");
    // The laminar step with a potential start is read back too.
    let laminar = common::study(
        "backward-step-laminar",
        &TemplateOptions {
            init: Some(InitMode::Potential),
            ..options(MeshPreset::Coarse)
        },
    );
    let laminar_tree = compiled(&laminar);
    assert!(text(&laminar_tree, "system/fvSolution").contains("potentialFlow"));
    assert_eq!(row().read_back(&laminar, &laminar_tree), vec![]);
}

/// A compiled study is clean under the scan and the value round trip.
fn clean(label: &str, study: &ResolvedStudy) -> cfdcraft_backend::CaseTree {
    let tree = compiled(study);
    assert!(
        row().scan(&tree).is_empty(),
        "{label}: {:?}",
        row().scan(&tree)
    );
    assert_eq!(row().read_back(study, &tree), vec![], "{label}");
    tree
}

#[test]
fn all_five_benchmarks_and_the_graded_box_compile_and_round_trip() {
    clean("cavity", &cavity());
    clean("laminar step", &step());
    let sst = clean("sst step", &sst_step());
    clean("cylinder", &cylinder());
    clean("heated cavity", &heated());
    let graded = clean("graded box", &graded_box());
    // The graded box carries its grading in the mesh dictionary: it differs from the plain one.
    assert_ne!(
        text(&graded, "system/blockMeshDict"),
        text(&compiled(&cavity()), "system/blockMeshDict")
    );
    for (folder, tree) in [("step-sst-of14", &sst), ("graded-box-of14", &graded)] {
        let misses = common::golden_misses(folder, tree);
        assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
    }
}

#[test]
fn the_capabilities_offer_everything_the_writer_does() {
    let capabilities = row().capabilities();
    for model in [
        TurbulenceModel::Laminar,
        TurbulenceModel::KOmegaSst,
        TurbulenceModel::RealizableKe,
        TurbulenceModel::RngKEpsilon,
    ] {
        assert!(capabilities.turbulence_models.contains(&model), "{model:?}");
    }
    assert!(capabilities.transient);
    assert!(capabilities.buoyancy);
    assert!(capabilities.potential_init);
    assert!(capabilities.accepts_raw);
    assert_eq!(capabilities.raw_backend, Some(RawBackend::Foundation));

    // None of the settings the first version refused is refused now, by the model or the writer.
    let transient = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..options(MeshPreset::Coarse)
    };
    let potential = TemplateOptions {
        init: Some(InitMode::Potential),
        ..options(MeshPreset::Coarse)
    };
    for (id, template) in [
        ("backward-step", options(MeshPreset::Coarse)),
        ("lid-cavity", transient),
        ("heated-cavity", options(MeshPreset::Coarse)),
        ("backward-step-laminar", potential),
        ("cylinder", options(MeshPreset::Coarse)),
    ] {
        let study = authored(id, &template);
        let codes: Vec<String> = diagnose(&study, &capabilities)
            .into_iter()
            .map(|d| d.code)
            .filter(|c| c.starts_with("CFD-BACKEND-"))
            .collect();
        assert_eq!(codes, Vec::<String>::new(), "{id}");
        let resolved = resolve(&study).unwrap();
        assert!(
            row().compile(&resolved, &CompileOptions::default()).is_ok(),
            "{id}"
        );
    }

    // A raw override tagged for this line is accepted.
    let mut tagged = authored("lid-cavity", &options(MeshPreset::Coarse));
    tagged.raw_overrides.insert(
        "tolerance".into(),
        RawOverride {
            backend: RawBackend::Foundation,
            file: "system/fvSolution".into(),
            entry: "relaxationFactors/equations/U".into(),
            value: "0.5".into(),
        },
    );
    let tree = compiled(&resolve(&tagged).unwrap());
    assert!(text(&tree, "system/fvSolution").contains("U 0.5;"));
}
