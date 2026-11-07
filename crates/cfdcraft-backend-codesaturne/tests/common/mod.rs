//! Studies and helpers the tests share.
#![allow(dead_code)]

use cfdcraft_backend::{Backend, CaseTree, CompileOptions};
use cfdcraft_backend_codesaturne::{
    backends,
    xml::{Element, parse},
};
use cfdcraft_model::{InitMode, ResolvedStudy, Study, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use std::collections::BTreeMap;

pub const CAVITY_V1: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");

pub fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// The options of a template at a mesh preset, with the flow started from a uniform field because
/// the solver cannot start from a potential-flow solution.
pub fn options(mesh: MeshPreset) -> TemplateOptions {
    TemplateOptions {
        mesh,
        init: Some(InitMode::Uniform),
        ..TemplateOptions::default()
    }
}

pub fn study_of(id: &str, options: &TemplateOptions) -> Study {
    build(id, options).unwrap()
}

pub fn resolved(id: &str, options: &TemplateOptions) -> ResolvedStudy {
    resolve(&study_of(id, options)).unwrap()
}

/// A template at its coarsest mesh preset.
pub fn coarse(id: &str) -> ResolvedStudy {
    resolved(id, &options(MeshPreset::Coarse))
}

/// The lid-driven cavity of the first schema fixture on a mesh of `cells` by `cells`.
pub fn cavity(cells: u64) -> ResolvedStudy {
    let mut study = resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap();
    study.mesh.cells_x = cells;
    study.mesh.cells_y = cells;
    study
}

pub fn compiled(study: &ResolvedStudy) -> CaseTree {
    backend()
        .compile(study, &CompileOptions::default())
        .unwrap()
}

pub fn text(tree: &CaseTree, name: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path.to_str() == Some(name))
        .unwrap_or_else(|| panic!("the case has no {name}"));
    String::from_utf8(file.bytes.clone()).unwrap()
}

pub fn setup_root(tree: &CaseTree) -> Element {
    parse(&text(tree, "DATA/setup.xml")).unwrap()
}

/// The tree with the first `from` of one file replaced by `to`.
pub fn edited(tree: &CaseTree, name: &str, from: &str, to: &str) -> CaseTree {
    let mut tree = tree.clone();
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path.to_str() == Some(name))
        .unwrap();
    let body = String::from_utf8(file.bytes.clone()).unwrap();
    assert!(body.contains(from), "{name} has no {from}");
    file.bytes = body.replacen(from, to, 1).into_bytes();
    tree
}

/// The tree with the number inside the first `<tag>...</tag>` of the setup changed by `change`.
pub fn with_number_changed(tree: &CaseTree, tag: &str, change: impl Fn(f64) -> f64) -> CaseTree {
    let body = text(tree, "DATA/setup.xml");
    let open = format!("<{tag}>");
    let start = body.find(&open).unwrap_or_else(|| panic!("no <{tag}>")) + open.len();
    let end = start + body[start..].find('<').unwrap();
    let old: f64 = body[start..end].parse().unwrap();
    edited(
        tree,
        "DATA/setup.xml",
        &format!("<{tag}>{}</{tag}>", &body[start..end]),
        &format!("<{tag}>{}</{tag}>", change(old)),
    )
}

/// The number inside the first `<tag>...</tag>` of an element's subtree.
pub fn number_of(element: &Element, tag: &str) -> f64 {
    let mut found = None;
    element.walk(&mut |e| {
        if found.is_none() && e.name == tag {
            found = Some(e.text.parse::<f64>().unwrap());
        }
    });
    found.unwrap_or_else(|| panic!("no <{tag}>"))
}

/// The pieces of a Gmsh 2.2 text the tests look at, read by a reader written for this file.
pub struct Msh {
    pub names: BTreeMap<u32, (u32, String)>,
    pub nodes: Vec<[f64; 3]>,
    /// (type, physical group, node numbers from 1)
    pub elements: Vec<(u32, u32, Vec<usize>)>,
}

pub fn read(text: &str) -> Msh {
    let mut lines = text.lines();
    let mut msh = Msh {
        names: BTreeMap::new(),
        nodes: Vec::new(),
        elements: Vec::new(),
    };
    while let Some(line) = lines.next() {
        match line {
            "$PhysicalNames" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let mut parts = lines.next().unwrap().splitn(3, ' ');
                    let dim: u32 = parts.next().unwrap().parse().unwrap();
                    let id: u32 = parts.next().unwrap().parse().unwrap();
                    let name = parts.next().unwrap().trim_matches('"').to_owned();
                    msh.names.insert(id, (dim, name));
                }
            }
            "$Nodes" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let parts: Vec<f64> = lines
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .map(|p| p.parse().unwrap())
                        .collect();
                    msh.nodes.push([parts[1], parts[2], parts[3]]);
                }
            }
            "$Elements" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let parts: Vec<usize> = lines
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .map(|p| p.parse().unwrap())
                        .collect();
                    let tags = parts[2];
                    msh.elements.push((
                        parts[1] as u32,
                        parts[3] as u32,
                        parts[3 + tags..].to_vec(),
                    ));
                }
            }
            _ => {}
        }
    }
    msh
}

impl Msh {
    pub fn group_id(&self, name: &str) -> Option<u32> {
        self.names
            .iter()
            .find(|(_, (_, n))| n == name)
            .map(|(id, _)| *id)
    }

    pub fn quads_of(&self, name: &str) -> Vec<&Vec<usize>> {
        let id = self
            .group_id(name)
            .unwrap_or_else(|| panic!("no group {name}"));
        self.elements
            .iter()
            .filter(|(kind, group, _)| *kind == 3 && *group == id)
            .map(|(_, _, nodes)| nodes)
            .collect()
    }

    pub fn node(&self, number: usize) -> [f64; 3] {
        self.nodes[number - 1]
    }
}
