use cfdcraft_model::resolve;
use cfdcraft_templates::{MeshPreset, TemplateOptions, build, list};

#[test]
fn list_names_every_template() {
    let found = list();
    let ids: Vec<&str> = found.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "lid-cavity",
            "backward-step",
            "backward-step-laminar",
            "cylinder",
            "heated-cavity"
        ]
    );
    let benchmarks: Vec<&str> = found.iter().map(|t| t.benchmark.as_str()).collect();
    assert_eq!(benchmarks, ["ghia", "step", "armaly", "cylinder", "dvd"]);
    for template in &found {
        assert!(!template.title.is_empty(), "{}", template.id);
        assert!(!template.reference.is_empty(), "{}", template.id);
        assert!(!template.defaults.is_empty(), "{}", template.id);
        assert!(!template.headline_quantities.is_empty(), "{}", template.id);
        for name in &template.headline_quantities {
            assert!(
                name.starts_with(&format!("bench.{}.", template.benchmark)),
                "{name}"
            );
        }
    }
    let step = &found[1];
    assert_eq!(step.defaults, "k-omega SST, steady");
    assert_eq!(step.headline_quantities, ["bench.step.reattachment"]);
    assert_eq!(found[0].headline_quantities, ["bench.ghia.u_centre"]);
    assert_eq!(found[2].headline_quantities, ["bench.armaly.reattachment"]);
    assert_eq!(
        found[3].headline_quantities,
        [
            "bench.cylinder.st",
            "bench.cylinder.cd_max",
            "bench.cylinder.cl_max"
        ]
    );
    assert_eq!(
        found[4].headline_quantities,
        ["bench.dvd.nu_hot", "bench.dvd.w_max"]
    );
}

#[test]
fn every_template_builds_at_every_preset() {
    for template in list() {
        for mesh in MeshPreset::ALL {
            let options = TemplateOptions {
                mesh,
                ..TemplateOptions::default()
            };
            let first = build(&template.id, &options).unwrap();
            let second = build(&template.id, &options).unwrap();
            assert_eq!(first, second, "{} is not deterministic", template.id);
            assert_eq!(first.mesh.level, mesh.level(), "{}", template.id);
            resolve(&first).unwrap_or_else(|error| panic!("{} {mesh:?}: {error}", template.id));
        }
    }
}
