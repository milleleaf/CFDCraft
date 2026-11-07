//! Listing the benchmark templates and making a complete study from one: the words that choose
//! the options, the refusals that name an option, and the promise that the operation is pure.

use cfdcraft_gate::Benchmark;
use cfdcraft_model::{InitMode, ResolvedTime, Study, TurbulenceModel, resolve};
use cfdcraft_ops::benchmark_ops::parse_options;
use cfdcraft_ops::{
    BenchmarkOptions, BenchmarksData, ExitClass, Report, Repro, benchmark_create, benchmark_list,
    created_line, ids, select,
};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice};
use cfdcraft_text::{Line, Text};

/// Each template and the benchmark that judges its runs, in the order the list gives them.
const TEMPLATES: [(&str, &str); 5] = [
    ("lid-cavity", "ghia"),
    ("backward-step", "step"),
    ("backward-step-laminar", "armaly"),
    ("cylinder", "cylinder"),
    ("heated-cavity", "dvd"),
];

fn say(line: &Line) -> String {
    Text::english().render(line)
}

fn id_of(benchmark: &dyn Benchmark) -> &'static str {
    benchmark.id()
}

fn words(
    mesh: Option<&str>,
    turbulence: Option<&str>,
    time: Option<&str>,
    init: Option<&str>,
) -> BenchmarkOptions {
    let owned = |word: Option<&str>| word.map(str::to_owned);
    BenchmarkOptions {
        mesh: owned(mesh),
        turbulence: owned(turbulence),
        time: owned(time),
        init: owned(init),
        ..BenchmarkOptions::default()
    }
}

fn listed(report: &Report) -> &BenchmarksData {
    match &report.repro {
        Some(Repro::Benchmarks(data)) => data,
        other => panic!("expected the list of benchmarks, found {other:?}"),
    }
}

/// The study text of a creation that was accepted.
fn made(id: &str, options: &BenchmarkOptions) -> String {
    let (report, text) = benchmark_create(id, options);
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{id}: {}",
        report.render_text()
    );
    text.unwrap_or_else(|| panic!("{id}: no study came back"))
}

/// A creation that was refused: exit class 2, one finding, no study. Returns its sentence.
fn refusal(id: &str, options: &BenchmarkOptions) -> String {
    let (report, text) = benchmark_create(id, options);
    assert_eq!(report.class(), ExitClass::StudyInvalid, "{id}");
    assert_eq!(report.exit_class, 2);
    assert_eq!(report.diagnostics.len(), 1, "{}", report.render_text());
    assert_eq!(report.diagnostics[0].code, "CFD-CLI-001");
    assert!(text.is_none());
    report.diagnostics[0].message.clone()
}

#[test]
fn the_list_names_every_template() {
    let report = benchmark_list();
    assert_eq!(report.class(), ExitClass::Passed);
    assert_eq!(report.operation, "benchmark-list");
    let data = listed(&report);
    assert_eq!(data.benchmarks.len(), 5);
    for (entry, (template, benchmark)) in data.benchmarks.iter().zip(TEMPLATES) {
        assert_eq!(entry.id, template);
        assert_eq!(entry.benchmark, benchmark);
        assert!(ids().contains(&benchmark));
        assert!(!entry.title.is_empty() && !entry.reference.is_empty());
        assert!(!entry.defaults.is_empty());
        assert!(
            entry
                .headline_quantities
                .iter()
                .all(|name| name.starts_with(&format!("bench.{benchmark}."))),
            "{:?}",
            entry.headline_quantities
        );
        assert!(!entry.headline_quantities.is_empty());
    }

    let lines = data.lines();
    assert_eq!(lines.len(), 5);
    for (line, entry) in lines.iter().zip(&data.benchmarks) {
        assert_eq!(
            line,
            &Line::BenchmarkListed {
                id: entry.id.clone(),
                title: entry.title.clone(),
                reference: entry.reference.clone(),
            }
        );
    }
    assert_eq!(
        say(&lines[0]),
        format!(
            "{}: {} ({}).",
            data.benchmarks[0].id, data.benchmarks[0].title, data.benchmarks[0].reference
        )
    );

    let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
    assert_eq!(json["repro"]["kind"], "benchmarks");
    assert_eq!(json["repro"]["benchmarks"][3]["id"], "cylinder");
}

#[test]
fn create_returns_a_complete_study_text() {
    let options = words(Some("fine"), Some("rng-k-epsilon"), None, Some("potential"));
    let text = made("backward-step", &options);
    assert!(text.ends_with('\n'));

    let study = Study::from_json(&text).expect("the text is a study file");
    let resolved = resolve(&study).expect("the study has no findings");
    assert_eq!(resolved.mesh.level, 2);
    let physics = resolved.physics.expect("the options are physics");
    assert_eq!(physics.turbulence.model, TurbulenceModel::RngKEpsilon);
    assert_eq!(physics.init, InitMode::Potential);
    assert!(matches!(physics.time, ResolvedTime::Steady));

    let line = created_line("backward-step", &options, "-");
    assert_eq!(
        line,
        Line::BenchmarkCreated {
            benchmark: "backward-step".to_owned(),
            mesh: "fine".to_owned(),
            path: "-".to_owned(),
        }
    );
    assert_eq!(
        say(&line),
        "Wrote the backward-step study at the fine mesh to -."
    );
    // With no word for the mesh the sentence says the default.
    let plain = created_line("cylinder", &BenchmarkOptions::default(), "cyl.json");
    assert!(say(&plain).contains("at the medium mesh"));

    let (report, _) = benchmark_create("backward-step", &options);
    assert!(report.study_hash.is_some());
    assert_eq!(report.operation, "benchmark-create");
}

#[test]
fn option_words_are_checked() {
    // Every word is read in any case, and the numbers come through as they are.
    let all = BenchmarkOptions {
        mesh: Some("FINE".to_owned()),
        turbulence: Some("K-Omega-SST".to_owned()),
        time: Some("Transient".to_owned()),
        delta_t: Some(0.001),
        init: Some("Potential".to_owned()),
        rayleigh: None,
        reynolds: None,
    };
    assert_eq!(
        parse_options(&all),
        Ok(TemplateOptions {
            mesh: MeshPreset::Fine,
            turbulence: Some(TurbulenceModel::KOmegaSst),
            time: Some(TimeChoice::Transient),
            delta_t: Some(0.001),
            init: Some(InitMode::Potential),
            rayleigh: None,
            reynolds: None,
        })
    );
    assert_eq!(
        parse_options(&BenchmarkOptions::default()),
        Ok(TemplateOptions::default())
    );
    for word in ["coarse", "medium", "Fine"] {
        assert!(parse_options(&words(Some(word), None, None, None)).is_ok());
    }
    for word in ["laminar", "k-omega-sst", "realizable-ke", "rng-k-epsilon"] {
        assert!(parse_options(&words(None, Some(word), None, None)).is_ok());
    }
    for word in ["steady", "transient"] {
        assert!(parse_options(&words(None, None, Some(word), None)).is_ok());
    }
    for word in ["uniform", "potential"] {
        assert!(parse_options(&words(None, None, None, Some(word))).is_ok());
    }

    // Any other word is refused with a sentence that names the option and shows the word.
    let bad = [
        ("mesh", words(Some("huge"), None, None, None), "huge"),
        ("turbulence", words(None, Some("les"), None, None), "les"),
        ("time", words(None, None, Some("weekly"), None), "weekly"),
        ("init", words(None, None, None, Some("random")), "random"),
    ];
    for (option, options, shown) in bad {
        let sentence = refusal("backward-step", &options);
        assert!(sentence.contains(option), "{sentence}");
        assert!(sentence.contains(shown), "{sentence}");
        assert_eq!(parse_options(&options).unwrap_err(), sentence);
    }
    let sentence = refusal("lid-cavity", &words(Some("fi\nne"), None, None, None));
    assert!(!sentence.chars().any(char::is_control), "{sentence:?}");

    // A step must be a number above zero, and only a transient run has one.
    for step in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        let options = BenchmarkOptions {
            time: Some("transient".to_owned()),
            delta_t: Some(step),
            ..BenchmarkOptions::default()
        };
        assert!(refusal("cylinder", &options).contains("delta_t"));
    }
    let steady = BenchmarkOptions {
        delta_t: Some(0.001),
        ..BenchmarkOptions::default()
    };
    assert!(refusal("backward-step", &steady).contains("delta_t"));

    // A number a template does not take is refused, and so is a Reynolds or Rayleigh number that
    // is not above zero.
    let rayleigh = BenchmarkOptions {
        rayleigh: Some(1e5),
        ..BenchmarkOptions::default()
    };
    assert!(refusal("cylinder", &rayleigh).contains("rayleigh"));
    let reynolds = BenchmarkOptions {
        reynolds: Some(200.0),
        ..BenchmarkOptions::default()
    };
    assert!(refusal("lid-cavity", &reynolds).contains("reynolds"));
    let negative = BenchmarkOptions {
        reynolds: Some(-3.0),
        ..BenchmarkOptions::default()
    };
    assert!(refusal("backward-step-laminar", &negative).contains("reynolds"));
    // The two that apply are taken.
    made("heated-cavity", &rayleigh);
    made("backward-step-laminar", &reynolds);

    // An unknown template is refused and the known ones are listed.
    let sentence = refusal("nope", &BenchmarkOptions::default());
    assert!(sentence.contains("nope"), "{sentence}");
    assert!(sentence.contains("lid-cavity"), "{sentence}");
}

#[test]
fn the_created_study_selects_its_benchmark() {
    for (template, benchmark) in TEMPLATES {
        for mesh in ["coarse", "medium", "fine"] {
            let options = words(Some(mesh), None, None, None);
            let study = Study::from_json(&made(template, &options)).unwrap();
            let resolved = resolve(&study).unwrap();
            let chosen = select(&resolved, None)
                .unwrap_or_else(|rejected| panic!("{template} at {mesh}: {rejected:?}"));
            assert_eq!(id_of(chosen.as_ref()), benchmark, "{template} at {mesh}");
        }
    }
}

#[test]
fn every_choice_is_a_study_or_a_refusal_that_names_its_option() {
    for (template, _) in TEMPLATES {
        for turbulence in ["laminar", "k-omega-sst", "realizable-ke", "rng-k-epsilon"] {
            for time in ["steady", "transient"] {
                for init in ["uniform", "potential"] {
                    let options = words(Some("coarse"), Some(turbulence), Some(time), Some(init));
                    let (report, text) = benchmark_create(template, &options);
                    // A potential-flow start does not apply to the closed heated cavity; every
                    // other choice makes a study.
                    let refused = template == "heated-cavity" && init == "potential";
                    let label = format!("{template} {turbulence} {time} {init}");
                    match text {
                        Some(text) => {
                            assert!(!refused, "{label}");
                            assert_eq!(report.class(), ExitClass::Passed, "{label}");
                            assert!(Study::from_json(&text).is_ok(), "{label}");
                        }
                        None => {
                            assert!(refused, "{label}: {}", report.render_text());
                            assert_eq!(report.class(), ExitClass::StudyInvalid, "{label}");
                            let sentence = &report.diagnostics[0].message;
                            assert!(sentence.contains("init"), "{label}: {sentence}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn create_is_pure() {
    let options = words(Some("medium"), None, Some("transient"), None);
    let (first, text) = benchmark_create("cylinder", &options);
    let (second, again) = benchmark_create("cylinder", &options);
    assert_eq!(text, again, "the same words give the same study");
    assert_eq!(first, second);
    assert!(first.artifacts.is_empty(), "no file is named");
    assert!(first.run.is_none() && first.infra.is_none());

    // The text is the template's study as the model prints it, and nothing else.
    let template = cfdcraft_templates::build(
        "cylinder",
        &TemplateOptions {
            mesh: MeshPreset::Medium,
            time: Some(TimeChoice::Transient),
            ..TemplateOptions::default()
        },
    )
    .unwrap();
    let mut expected = serde_json::to_string_pretty(&template).unwrap();
    expected.push('\n');
    assert_eq!(text.as_deref(), Some(expected.as_str()));
}
