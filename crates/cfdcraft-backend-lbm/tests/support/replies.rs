//! Synthetic requests and replies of a lattice run, and an in-memory folder over them. No solver
//! ran: every number is made by the test that asks for it.

#![allow(dead_code)]

use cfdcraft_backend::RunFiles;
use cfdcraft_gate::ResultFiles;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A folder in memory. A size can be given that differs from the text, to stand for a long file.
#[derive(Default, Clone)]
pub struct Mem {
    pub files: BTreeMap<String, String>,
    pub sizes: BTreeMap<String, u64>,
}

impl Mem {
    pub fn without(mut self, rel: &str) -> Self {
        self.files.remove(rel);
        self
    }

    pub fn with(mut self, rel: &str, text: &str) -> Self {
        self.files.insert(rel.to_owned(), text.to_owned());
        self
    }

    pub fn with_size(mut self, rel: &str, size: u64) -> Self {
        self.sizes.insert(rel.to_owned(), size);
        self
    }

    fn names_below(&self, dir: &str) -> Vec<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

impl RunFiles for Mem {
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

impl ResultFiles for Mem {
    fn read(&self, rel: &str) -> Option<String> {
        self.files.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.names_below(rel_dir)
    }
}

/// The answer of a good solve: 1200 steps ended at the end time, a negligible mass change, no
/// warnings, and a change measure that falls.
pub fn good_solve() -> Value {
    json!({
        "steps": 1200,
        "stop": "end_time",
        "mass": {"relative_change": 1e-9},
        "warnings": [],
        "change": [1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10, 1e-11],
    })
}

/// A reply with its answer carried as the text of a content item.
fn text_reply(id: u64, answer: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": answer.to_string()}], "isError": false},
    })
}

/// A reply with its answer carried as structured content.
fn structured_reply(id: u64, answer: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [], "structuredContent": answer, "isError": false},
    })
}

/// The requests and the replies of a run: the opening, the setup, the solve, then one request per
/// probe, each its own `execute` call.
pub struct Stream {
    pub requests: Vec<Value>,
    pub replies: Vec<Value>,
    pub log: String,
}

impl Stream {
    /// A good run whose setup names `steps` when given, and whose solve answers with `solve`.
    pub fn run(steps: Option<u64>, solve: &Value) -> Self {
        let mut procedure = json!({"type": "fluid", "domain": "Flow", "end_time": 1.0});
        if let Some(steps) = steps {
            procedure["steps"] = json!(steps);
        }
        let setup = json!({"commands": [
            {"command": "model.units", "params": {"system": "m-kg-s-N-Pa"}},
            {"command": "model.step.create", "params": {"name": "Run", "procedure": procedure}},
        ]});
        let call = |id: u64, tool: &str, arguments: Value| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            })
        };
        let opening = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let solve_call = call(
            3,
            "execute",
            json!({"command": "solve.fluid", "params": {"step": "Run", "backend": "cpu"}}),
        );
        Self {
            requests: vec![opening, call(2, "batch", setup), solve_call],
            replies: vec![
                json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": "2025-06-18"}}),
                text_reply(2, &json!({"ok": true})),
                structured_reply(3, solve),
            ],
            log: "fieldcraft-cli: server started\n".to_owned(),
        }
    }

    pub fn good() -> Self {
        Self::run(None, &good_solve())
    }

    /// Adds one `solve.probe` request with its answer; returns the request number.
    pub fn probe(&mut self, params: Value, answer: &Value) -> u64 {
        let id = self.requests.len() as u64 + 1;
        self.requests.push(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": "execute", "arguments": {"command": "solve.probe", "params": params}},
        }));
        self.replies.push(text_reply(id, answer));
        id
    }

    /// The three velocity components along a line, sampled at `count` evenly spaced points from
    /// `from` to `to` (both ends included) at the height `z`, from the field `u(x, y)`.
    pub fn velocity_line(
        &mut self,
        from: [f64; 2],
        to: [f64; 2],
        z: f64,
        count: usize,
        u: impl Fn(f64, f64) -> [f64; 3],
    ) {
        let xyz: Vec<[f64; 3]> = (0..count)
            .map(|i| {
                let t = i as f64 / (count - 1) as f64;
                [
                    from[0] + t * (to[0] - from[0]),
                    from[1] + t * (to[1] - from[1]),
                    z,
                ]
            })
            .collect();
        for (component, field) in ["velocity_x", "velocity_y", "velocity_z"]
            .iter()
            .enumerate()
        {
            let values: Vec<f64> = xyz.iter().map(|p| u(p[0], p[1])[component]).collect();
            self.probe(
                json!({"step": "Run", "line": {
                    "from": [from[0], from[1], z], "to": [to[0], to[1], z],
                    "count": count, "field": field,
                }}),
                &json!({"xyz": xyz, "values": values}),
            );
        }
    }

    /// The commands file and the replies file, one message per line, and the log.
    pub fn files(&self) -> Mem {
        let join = |messages: &[Value]| {
            let mut text = String::new();
            for message in messages {
                text.push_str(&message.to_string());
                text.push('\n');
            }
            text
        };
        Mem::default()
            .with("commands.jsonl", &join(&self.requests))
            .with("replies.jsonl", &join(&self.replies))
            .with("log.fieldcraft", &self.log)
    }
}
