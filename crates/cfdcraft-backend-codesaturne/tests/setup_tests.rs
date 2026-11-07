use cfdcraft_backend_codesaturne::{
    setup::{FORMAT_VERSION, ROOT, Z_FACES_GROUP, setup_xml},
    xml::{Element, parse},
};
use cfdcraft_model::{ResolvedStudy, Study, resolve};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn cavity() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap()
}

fn number(element: &Element) -> f64 {
    element.text.parse().unwrap()
}

fn property<'a>(root: &'a Element, name: &str) -> &'a Element {
    root.find(&["physical_properties", "fluid_properties"])
        .unwrap()
        .children_named("property")
        .find(|p| p.attr("name") == Some(name))
        .unwrap()
}

#[test]
fn the_minimal_cavity_setup_reads_back() {
    let study = cavity();
    let root = parse(&setup_xml(&study).unwrap()).unwrap();
    assert_eq!(root.name, ROOT);
    assert_eq!(root.attr("version"), Some(FORMAT_VERSION));
    let top: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        top,
        [
            "solution_domain",
            "analysis_control",
            "calculation_management",
            "thermophysical_models",
            "additional_scalars",
            "physical_properties",
            "boundary_conditions",
            "numerical_parameters",
        ]
    );

    let density = property(&root, "density").child("initial_value").unwrap();
    assert!((number(density) - 1.0).abs() <= 1e-15);
    let viscosity = property(&root, "molecular_viscosity")
        .child("initial_value")
        .unwrap();
    assert!(
        (number(viscosity) - study.fluid.kinematic_viscosity).abs() <= 1e-18,
        "{}",
        viscosity.text
    );
    let time = root.find(&["analysis_control", "time_parameters"]).unwrap();
    assert_eq!(time.child("iterations").unwrap().text, "5000");
    assert_eq!(time.child("time_passing").unwrap().text, "-1");
    assert_eq!(
        root.find(&["thermophysical_models", "turbulence"])
            .and_then(|t| t.attr("model")),
        Some("off")
    );

    let zones: Vec<(&str, &str)> = root
        .child("boundary_conditions")
        .unwrap()
        .children_named("boundary")
        .map(|b| (b.attr("label").unwrap(), b.attr("nature").unwrap()))
        .collect();
    assert_eq!(
        zones,
        [
            ("x_min", "wall"),
            ("x_max", "wall"),
            ("y_min", "wall"),
            ("y_max", "wall"),
            (Z_FACES_GROUP, "symmetry"),
        ]
    );
    let lid = root
        .child("boundary_conditions")
        .unwrap()
        .children_named("wall")
        .find(|w| w.attr("label") == Some("y_max"))
        .unwrap();
    let components: Vec<f64> = lid
        .child("velocity_pressure")
        .unwrap()
        .children_named("dirichlet")
        .map(number)
        .collect();
    assert_eq!(components.len(), 3);
    for (got, want) in components.iter().zip([0.1, 0.0, 0.0]) {
        assert!((got - want).abs() <= 1e-15, "{components:?}");
    }
    let fixed = root
        .child("boundary_conditions")
        .unwrap()
        .children_named("wall")
        .find(|w| w.attr("label") == Some("x_min"))
        .unwrap();
    assert_eq!(
        fixed.child("velocity_pressure").unwrap().attr("choice"),
        Some("off")
    );
    let mesh = root.find(&["solution_domain", "meshes_list"]).unwrap();
    assert_eq!(mesh.child("mesh").unwrap().attr("name"), Some("mesh.msh"));
}

#[test]
fn the_setup_holds_constants_only() {
    let text = setup_xml(&cavity()).unwrap();
    let root = parse(&text).unwrap();
    let banned = ["formula", "mei", "compute", "user", "function"];
    let mut seen = 0;
    root.walk(&mut |element| {
        seen += 1;
        let mut words = vec![element.name.to_lowercase()];
        for (key, value) in &element.attributes {
            words.push(key.to_lowercase());
            words.push(value.to_lowercase());
        }
        words.push(element.text.to_lowercase());
        for word in &banned {
            for text in &words {
                assert!(!text.contains(word), "{} holds {word}", element.name);
            }
        }
        assert!(
            !element.text.contains([';', '=']),
            "{} holds expression syntax",
            element.name
        );
    });
    assert!(seen > 20, "{seen}");
}

#[test]
fn a_setup_the_writer_cannot_express_is_refused_by_name() {
    let mut study = cavity();
    study.solver.end_time = 10.5;
    assert!(setup_xml(&study).is_err());
    let mut study = cavity();
    study.fluid.kinematic_viscosity = 0.0;
    assert!(setup_xml(&study).is_err());
}
