//! Studies and helpers the tests share.
#![allow(dead_code)]

use cfdcraft_backend::{Backend, CaseFile, CaseTree, CompileOptions};
use cfdcraft_backend_su2::backends;
use cfdcraft_model::{ResolvedStudy, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};

/// The golden folder and the template of each of the five benchmark studies.
pub const FIVE: [(&str, &str); 5] = [
    ("cavity-su2", "lid-cavity"),
    ("step-su2", "backward-step-laminar"),
    ("step-sst-su2", "backward-step"),
    ("cylinder-su2", "cylinder"),
    ("heated-su2", "heated-cavity"),
];

pub fn backend() -> &'static dyn Backend {
    backends()[0]
}

pub fn options(mesh: MeshPreset) -> TemplateOptions {
    TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    }
}

pub fn study(id: &str, options: &TemplateOptions) -> ResolvedStudy {
    resolve(&build(id, options).unwrap()).unwrap()
}

/// A template at its coarsest mesh preset.
pub fn coarse(id: &str) -> ResolvedStudy {
    study(id, &options(MeshPreset::Coarse))
}

/// The lid-driven cavity on a mesh of four cells by four.
pub fn cavity4() -> ResolvedStudy {
    let mut s = coarse("lid-cavity");
    s.mesh.cells_x = 4;
    s.mesh.cells_y = 4;
    s
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

/// The tree with `from` replaced by `to` in one file; the file must hold `from`.
pub fn edited(tree: &CaseTree, name: &str, from: &str, to: &str) -> CaseTree {
    let original = text(tree, name);
    assert!(original.contains(from), "{name} has no {from}");
    let files = tree
        .files
        .iter()
        .map(|f| {
            if f.rel_path.to_str() == Some(name) {
                CaseFile {
                    rel_path: f.rel_path.clone(),
                    bytes: original.replace(from, to).into_bytes(),
                }
            } else {
                f.clone()
            }
        })
        .collect();
    CaseTree { files }
}

/// The value of a configuration key.
pub fn value(tree: &CaseTree, key: &str) -> String {
    let config = text(tree, "config.cfg");
    config
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}= ")))
        .unwrap_or_else(|| panic!("the configuration has no {key}"))
        .to_owned()
}

/// Whether the configuration has the key.
pub fn has_key(tree: &CaseTree, key: &str) -> bool {
    text(tree, "config.cfg")
        .lines()
        .any(|l| l.starts_with(&format!("{key}= ")))
}

/// The keys of the configuration, in file order.
pub fn keys(tree: &CaseTree) -> Vec<String> {
    text(tree, "config.cfg")
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.to_owned()))
        .collect()
}

/// A mesh file read by code that shares nothing with the writer.
pub struct Mesh {
    pub nodes: Vec<[f64; 2]>,
    pub cells: Vec<[usize; 4]>,
    /// The tag and the edges of each marker.
    pub markers: Vec<(String, Vec<[usize; 2]>)>,
}

impl Mesh {
    /// The area of cell `i` by the shoelace formula; positive for a counter-clockwise cell.
    pub fn area(&self, i: usize) -> f64 {
        let c = self.cells[i];
        let p: Vec<[f64; 2]> = c.iter().map(|n| self.nodes[*n]).collect();
        (0..4)
            .map(|k| p[k][0] * p[(k + 1) % 4][1] - p[(k + 1) % 4][0] * p[k][1])
            .sum::<f64>()
            / 2.0
    }
}

pub fn parse_mesh(source: &str) -> Mesh {
    let mut lines = source.lines();
    let mut header = |key: &str| -> usize {
        let line = lines.next().unwrap();
        line.strip_prefix(&format!("{key}= "))
            .unwrap_or_else(|| panic!("expected {key}, found {line}"))
            .parse()
            .unwrap()
    };
    assert_eq!(header("NDIME"), 2);
    let cells = header("NELEM");
    let mut mesh = Mesh {
        nodes: Vec::new(),
        cells: Vec::new(),
        markers: Vec::new(),
    };
    for _ in 0..cells {
        let t: Vec<usize> = lines
            .next()
            .unwrap()
            .split(' ')
            .map(|w| w.parse().unwrap())
            .collect();
        assert_eq!(t[0], 9);
        mesh.cells.push([t[1], t[2], t[3], t[4]]);
    }
    let nodes = {
        let line = lines.next().unwrap();
        line.strip_prefix("NPOIN= ")
            .unwrap()
            .parse::<usize>()
            .unwrap()
    };
    for _ in 0..nodes {
        let t: Vec<f64> = lines
            .next()
            .unwrap()
            .split(' ')
            .map(|w| w.parse().unwrap())
            .collect();
        mesh.nodes.push([t[0], t[1]]);
    }
    let markers = {
        let line = lines.next().unwrap();
        line.strip_prefix("NMARK= ")
            .unwrap()
            .parse::<usize>()
            .unwrap()
    };
    for _ in 0..markers {
        let tag = lines
            .next()
            .unwrap()
            .strip_prefix("MARKER_TAG= ")
            .unwrap()
            .to_owned();
        let count: usize = lines
            .next()
            .unwrap()
            .strip_prefix("MARKER_ELEMS= ")
            .unwrap()
            .parse()
            .unwrap();
        let edges = (0..count)
            .map(|_| {
                let t: Vec<&str> = lines.next().unwrap().split(' ').collect();
                assert_eq!(t[0], "3");
                [t[1].parse().unwrap(), t[2].parse().unwrap()]
            })
            .collect();
        mesh.markers.push((tag, edges));
    }
    assert!(lines.next().is_none(), "the mesh text has trailing lines");
    mesh
}

// Synthetic native output of a run. Nothing here is solver output: the names follow the markers of
// the crate, and the numbers are made up by the tests.

/// A run folder held in memory. `sizes` overrides the size a file reports, so a test can state a
/// file longer than the read limit without making one.
#[derive(Default, Clone)]
pub struct Mem {
    pub files: std::collections::BTreeMap<String, String>,
    pub sizes: std::collections::BTreeMap<String, u64>,
}

impl Mem {
    pub fn with(mut self, name: &str, text: impl Into<String>) -> Self {
        self.files.insert(name.to_owned(), text.into());
        self
    }

    pub fn sized(mut self, name: &str, size: u64) -> Self {
        self.sizes.insert(name.to_owned(), size);
        self
    }

    pub fn without(mut self, name: &str) -> Self {
        self.files.remove(name);
        self
    }

    fn names_below(&self, dir: &str) -> Vec<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{}/", dir.trim_end_matches('/'))
        };
        let names: std::collections::BTreeSet<String> = self
            .files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
            .collect();
        names.into_iter().collect()
    }
}

impl cfdcraft_backend::RunFiles for Mem {
    fn read_text(&self, rel: &str) -> Option<String> {
        self.files.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.names_below(rel_dir)
    }

    fn size(&self, rel: &str) -> Option<u64> {
        let text = self.files.get(rel)?;
        Some(self.sizes.get(rel).copied().unwrap_or(text.len() as u64))
    }
}

impl cfdcraft_gate::ResultFiles for Mem {
    fn read(&self, rel: &str) -> Option<String> {
        self.files.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.names_below(rel_dir)
    }
}

pub const LOG: &str = "log.SU2";
pub const HISTORY: &str = "history.csv";
pub const VOLUME: &str = "flow.csv";
pub const RESIDUALS: [&str; 3] = ["rms[P]", "rms[U]", "rms[V]"];

/// A log of a run that reached its end.
pub fn good_log() -> String {
    "Iteration 1\nIteration 2\n\nExit Success (SU2_CFD)\n".to_owned()
}

/// A history table: quoted header names, then `rows` rows of the values `row(i)` gives.
pub fn history_text(columns: &[&str], rows: usize, row: impl Fn(usize) -> Vec<f64>) -> String {
    let header = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let mut text = format!("{header}\n");
    for i in 0..rows {
        let values = row(i);
        assert_eq!(values.len(), columns.len());
        let line = values
            .iter()
            .map(|v| format!("{v:.12}"))
            .collect::<Vec<_>>()
            .join(",");
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// The number of history rows a finished run of the study has.
pub fn rows_needed(study: &ResolvedStudy) -> usize {
    use cfdcraft_model::ResolvedTime;
    let end = study.solver.end_time;
    let count = match study.physics.as_ref().map(|p| &p.time) {
        Some(ResolvedTime::Transient { delta_t }) => (end / delta_t).round(),
        _ => end.round(),
    };
    count as usize
}

/// The residual limits of a steady study, as the base-10 logarithms the history holds.
pub fn log_limits(study: &ResolvedStudy) -> (f64, f64) {
    (
        study.solver.convergence.p.log10(),
        study.solver.convergence.u.log10(),
    )
}

/// A steady history of the study's full length whose residuals end `margin` decades under the
/// limits and keep falling.
pub fn steady_history(study: &ResolvedStudy, margin: f64) -> String {
    let (p, u) = log_limits(study);
    let rows = rows_needed(study);
    history_text(
        &["Inner_Iter", RESIDUALS[0], RESIDUALS[1], RESIDUALS[2]],
        rows,
        |i| {
            let fall = (rows - i) as f64 / rows as f64;
            vec![
                i as f64,
                p - margin + fall,
                u - margin + fall,
                u - margin + fall,
            ]
        },
    )
}

/// A volume table of the study's mesh, with the velocity and temperature `field(x, y)` gives as
/// `[Ux, Uy, T]` at every node.
pub fn volume_text(study: &ResolvedStudy, field: impl Fn(f64, f64) -> [f64; 3]) -> String {
    use cfdcraft_backend_su2::mesh::{layout_of, mesh_of};
    let mesh = mesh_of(&layout_of(study).unwrap()).unwrap();
    let mut text = String::from(
        "\"PointID\",\"x\",\"y\",\"Pressure\",\"Velocity_x\",\"Velocity_y\",\"Temperature\"\n",
    );
    for (i, n) in mesh.nodes.iter().enumerate() {
        let [ux, uy, t] = field(n[0], n[1]);
        text.push_str(&format!(
            "{i},{:.12},{:.12},0.0,{ux:.12},{uy:.12},{t:.12}\n",
            n[0], n[1]
        ));
    }
    text
}

/// The files of a clean steady run of the study.
pub fn clean_steady(study: &ResolvedStudy) -> Mem {
    Mem::default()
        .with(LOG, good_log())
        .with(HISTORY, steady_history(study, 1.0))
        .with(VOLUME, volume_text(study, |x, y| [x + y, x - y, 300.0]))
}
