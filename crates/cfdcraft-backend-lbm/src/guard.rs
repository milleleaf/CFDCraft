//! The safety scan of a written case. Nothing is passed on because it looks harmless: a command
//! stream line is admitted only when its method, its tool, every command and every parameter are on
//! a reviewed list, and every text value is a plain name or a relative path inside the case.
//!
//! `CFD-GUARD-014` says a message, a tool or a command is not on the list. `CFD-GUARD-015` says a
//! parameter is outside the allowed set, a text value could name something outside the case, or a
//! file does not belong to a case.

use crate::{
    markers::{
        BATCH_TOOL, EXECUTE_TOOL, FIELD_VELOCITY, LINE_COUNT, LINE_FIELD, LINE_FROM, LINE_TO,
        MAX_FRAMES, MAX_LINE_COUNT, MAX_PROBE_POINTS, MAX_PROBE_WALLS, PARAM_FORCES, PARAM_FRAME,
        PARAM_HEAT, PARAM_LINE, PARAM_POINTS, PARAM_STEP, PROBE_COMMAND, PROTOCOL_VERSION,
        STEP_FRAMES,
    },
    plan::{self, COMMANDS_FILE},
};
use cfdcraft_backend::{CaseTree, Refusal};
use serde_json::Value;
use std::path::{Component, Path};

/// The longest line of the command stream, in bytes.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The longest text value.
const MAX_TEXT_BYTES: usize = 128;

/// The longest piece of offending text a refusal repeats.
const MAX_ENTRY_CHARS: usize = 40;

/// What a missing member reads as.
static NULL: Value = Value::Null;

const NOT_ON_THE_LIST: &str = "CFD-GUARD-014";
const OUTSIDE_THE_SET: &str = "CFD-GUARD-015";

/// The JSON-RPC methods a line may call.
pub const ALLOWED_METHODS: [&str; 2] = ["initialize", "tools/call"];

/// The tools a `tools/call` line may name.
pub const ALLOWED_TOOLS: [&str; 2] = [BATCH_TOOL, EXECUTE_TOOL];

/// What a value of a command may be. An object holds only the members it lists, and a member the
/// object does not list is refused.
enum Rule {
    Number,
    /// A plain name or a relative path (see [`unsafe_text`]).
    Text,
    /// One of these words.
    Keyword(&'static [&'static str]),
    /// This many numbers.
    Vector(usize),
    /// A list of plain names.
    Names,
    /// A file of an obstacle: `obstacles/<name>.3mf`, relative, one folder deep.
    ObstaclePath,
    /// A whole number from the first to the second, both included.
    Count(u64, u64),
    /// A list of one to this many entries, each of the rule.
    List(&'static Rule, usize),
    /// A wall named in a probe: a side of the box, or a body and one of its faces as
    /// `<body>/<face>`, each a plain word.
    Wall,
    Object(&'static [(&'static str, Rule)]),
}

/// The sides of the box, as the solver names them.
const SIDES: [&str; 4] = ["x_min", "x_max", "y_min", "y_max"];

const POINT: Rule = Rule::Vector(3);
const WALL: Rule = Rule::Wall;
const SIDE: Rule = Rule::Keyword(&SIDES);

/// What a line probe holds: its two ends, the number of sample points and a velocity component.
const LINE: Rule = Rule::Object(&[
    (LINE_FROM, Rule::Vector(3)),
    (LINE_TO, Rule::Vector(3)),
    (LINE_COUNT, Rule::Count(2, MAX_LINE_COUNT)),
    (LINE_FIELD, Rule::Keyword(&FIELD_VELOCITY)),
]);

/// The four forms of a probe call. A call holds exactly one of them, and `frame` goes with the
/// points only.
const PROBE_FORMS: [&str; 4] = [PARAM_POINTS, PARAM_LINE, PARAM_FORCES, PARAM_HEAT];

const PROBE: Rule = Rule::Object(&[
    (PARAM_STEP, Rule::Text),
    (PARAM_FRAME, Rule::Count(1, MAX_FRAMES)),
    (PARAM_POINTS, Rule::List(&POINT, MAX_PROBE_POINTS)),
    (PARAM_LINE, LINE),
    (PARAM_FORCES, Rule::List(&WALL, MAX_PROBE_WALLS)),
    (PARAM_HEAT, Rule::List(&SIDE, SIDES.len())),
]);

const FACE: Rule = Rule::Object(&[
    (
        "type",
        Rule::Keyword(&[
            "wall",
            "moving_wall",
            "slip",
            "velocity",
            "pressure",
            "periodic",
        ]),
    ),
    ("velocity", Rule::Vector(3)),
    ("profile", Rule::Keyword(&["uniform", "parabolic"])),
    ("pressure", Rule::Number),
    ("outflow", Rule::Keyword(&["uniform", "convective"])),
]);

/// A wall held at a temperature; every other face keeps the solver's adiabatic default and is not
/// written.
const HELD_FACE: Rule = Rule::Object(&[
    ("type", Rule::Keyword(&["temperature"])),
    ("value", Rule::Number),
]);

const THERMAL_FACES: Rule = Rule::Object(&[
    ("x_min", HELD_FACE),
    ("x_max", HELD_FACE),
    ("y_min", HELD_FACE),
    ("y_max", HELD_FACE),
]);

const THERMAL: Rule = Rule::Object(&[
    ("initial_temperature", Rule::Number),
    ("gravity", Rule::Vector(3)),
    ("expansion", Rule::Number),
    ("temperature_zero", Rule::Number),
    ("faces", THERMAL_FACES),
]);

const FACES: Rule = Rule::Object(&[
    ("x_min", FACE),
    ("x_max", FACE),
    ("y_min", FACE),
    ("y_max", FACE),
    ("z_min", FACE),
    ("z_max", FACE),
]);

const INITIALIZE: Rule = Rule::Object(&[
    ("protocolVersion", Rule::Keyword(&[PROTOCOL_VERSION])),
    ("capabilities", Rule::Object(&[])),
    (
        "clientInfo",
        Rule::Object(&[("name", Rule::Text), ("version", Rule::Text)]),
    ),
]);

/// Every command the writer can emit, with the parameters each takes.
const COMMANDS: &[(&str, Rule)] = &[
    (
        "model.units",
        Rule::Object(&[("system", Rule::Keyword(&["m-kg-s-N-Pa"]))]),
    ),
    (
        "model.material.create",
        Rule::Object(&[
            ("name", Rule::Text),
            ("density", Rule::Number),
            (
                "fluid",
                Rule::Object(&[
                    ("type", Rule::Keyword(&["newtonian"])),
                    ("viscosity", Rule::Number),
                ]),
            ),
            (
                "thermal",
                Rule::Object(&[
                    ("conductivity", Rule::Number),
                    ("specific_heat", Rule::Number),
                ]),
            ),
        ]),
    ),
    (
        "geometry.import.mesh",
        Rule::Object(&[("path", Rule::ObstaclePath)]),
    ),
    (
        "model.fluid_domain.create",
        Rule::Object(&[
            ("name", Rule::Text),
            ("material", Rule::Text),
            ("min", Rule::Vector(3)),
            ("max", Rule::Vector(3)),
            ("spacing", Rule::Number),
            ("faces", FACES),
            ("obstacles", Rule::Names),
            ("thermal", THERMAL),
        ]),
    ),
    (
        "model.step.create",
        Rule::Object(&[
            ("name", Rule::Text),
            (
                "procedure",
                Rule::Object(&[
                    ("type", Rule::Keyword(&["fluid"])),
                    ("domain", Rule::Text),
                    ("end_time", Rule::Number),
                    ("time_step", Rule::Number),
                    (STEP_FRAMES, Rule::Count(1, MAX_FRAMES)),
                    ("steady", Rule::Object(&[("tolerance", Rule::Number)])),
                ]),
            ),
        ]),
    ),
    (
        "solve.fluid",
        Rule::Object(&[
            ("step", Rule::Text),
            // The processor is never the GPU: it is shared, and it computes in single precision.
            ("backend", Rule::Keyword(&["cpu"])),
        ]),
    ),
    (PROBE_COMMAND, PROBE),
];

/// The names of the commands on the list, in the order the writer emits them.
pub fn allowed_commands() -> impl Iterator<Item = &'static str> {
    COMMANDS.iter().map(|(name, _)| *name)
}

/// Why a text value could name something outside the case, or `None` when it is a plain name or a
/// relative path.
fn unsafe_text(text: &str) -> Option<&'static str> {
    let bytes = text.as_bytes();
    if text.len() > MAX_TEXT_BYTES {
        Some("is too long")
    } else if text.contains("..") {
        Some("holds `..`")
    } else if text.starts_with(['/', '\\', '~'])
        || (bytes.len() > 1 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
    {
        Some("is an absolute path")
    } else if text.contains('`') {
        Some("holds a backtick")
    } else if text.contains("$(") || text.contains("${") {
        Some("holds a command substitution")
    } else if text.chars().any(char::is_control) {
        Some("holds a control character")
    } else {
        None
    }
}

/// Whether `text` is `obstacles/<name>.3mf` with a plain name, the form of every obstacle file.
fn obstacle_path(text: &str) -> bool {
    text.strip_prefix("obstacles/")
        .and_then(|name| name.strip_suffix(".3mf"))
        .is_some_and(|name| {
            !name.is_empty() && unsafe_text(text).is_none() && !name.contains(['/', '\\'])
        })
}

/// Whether `text` is a wall a probe may name: a side of the box, or two plain words (ASCII
/// letters, digits, `_`, `-` and `:`) joined by one `/`, within the text limit.
fn wall_name(text: &str) -> bool {
    let plain = |word: &str| {
        !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':'))
    };
    text.len() <= MAX_TEXT_BYTES
        && (SIDES.contains(&text)
            || text
                .split_once('/')
                .is_some_and(|(body, face)| plain(body) && plain(face)))
}

/// The text shortened and with control characters written out, so a hostile value cannot flood or
/// confuse a report.
fn clip(text: &str) -> String {
    let shown: String = text
        .chars()
        .take(MAX_ENTRY_CHARS)
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect();
    if text.chars().count() > MAX_ENTRY_CHARS {
        format!("{shown}...")
    } else {
        shown
    }
}

fn refuse(out: &mut Vec<Refusal>, code: &'static str, line: u32, entry: &str, why: &str) {
    let shown = clip(entry);
    out.push(Refusal {
        code,
        file: COMMANDS_FILE.to_owned(),
        line,
        message: format!("{shown} {why}"),
        entry: shown,
    });
}

fn outside(out: &mut Vec<Refusal>, line: u32, path: &str, why: &str) {
    refuse(out, OUTSIDE_THE_SET, line, path, why);
}

fn check(value: &Value, rule: &Rule, path: &str, line: u32, out: &mut Vec<Refusal>) {
    match rule {
        Rule::Number => {
            if !value.is_number() {
                outside(out, line, path, "must be a number");
            }
        }
        Rule::Text => match value.as_str() {
            Some(text) => {
                if let Some(why) = unsafe_text(text) {
                    outside(out, line, path, &format!("is a text that {why}"));
                }
            }
            None => outside(out, line, path, "must be text"),
        },
        Rule::Keyword(words) => {
            if !value.as_str().is_some_and(|text| words.contains(&text)) {
                outside(out, line, path, "is not one of the reviewed values");
            }
        }
        Rule::Names => match value.as_array() {
            Some(items) => {
                for (index, item) in items.iter().enumerate() {
                    check(item, &Rule::Text, &format!("{path}[{index}]"), line, out);
                }
            }
            None => outside(out, line, path, "must be a list"),
        },
        Rule::ObstaclePath => match value.as_str() {
            Some(text) if obstacle_path(text) => {}
            _ => outside(out, line, path, "must be a file of an obstacle"),
        },
        Rule::Count(least, most) => {
            if !value
                .as_u64()
                .is_some_and(|count| (*least..=*most).contains(&count))
            {
                outside(
                    out,
                    line,
                    path,
                    &format!("must be a whole number from {least} to {most}"),
                );
            }
        }
        Rule::List(each, most) => match value.as_array() {
            Some(items) if items.is_empty() || items.len() > *most => outside(
                out,
                line,
                path,
                &format!("must hold from one to {most} entries"),
            ),
            Some(items) => {
                for (index, item) in items.iter().enumerate() {
                    check(item, each, &format!("{path}[{index}]"), line, out);
                }
            }
            None => outside(out, line, path, "must be a list"),
        },
        Rule::Wall => match value.as_str() {
            Some(text) if wall_name(text) => {}
            _ => outside(out, line, path, "must be a side or a body and its face"),
        },
        Rule::Vector(length) => {
            let ok = value
                .as_array()
                .is_some_and(|items| items.len() == *length && items.iter().all(Value::is_number));
            if !ok {
                outside(out, line, path, &format!("must be {length} numbers"));
            }
        }
        Rule::Object(members) => match value.as_object() {
            Some(map) => {
                for (key, member) in map {
                    let at = format!("{path}.{key}");
                    match members.iter().find(|(name, _)| *name == key.as_str()) {
                        Some((_, rule)) => check(member, rule, &at, line, out),
                        None => outside(out, line, &at, "is not a parameter of this command"),
                    }
                }
            }
            None => outside(out, line, path, "must be an object"),
        },
    }
}

/// One `{command, params}` entry, from a batch or from an `execute` call.
fn check_command(entry: &Value, path: &str, line: u32, out: &mut Vec<Refusal>) {
    let Some(map) = entry.as_object() else {
        refuse(out, OUTSIDE_THE_SET, line, path, "must be an object");
        return;
    };
    for key in map
        .keys()
        .filter(|key| !["command", "params"].contains(&key.as_str()))
    {
        refuse(
            out,
            OUTSIDE_THE_SET,
            line,
            &format!("{path}.{key}"),
            "is not a member of a command",
        );
    }
    let Some(name) = map.get("command").and_then(Value::as_str) else {
        refuse(out, NOT_ON_THE_LIST, line, path, "names no command");
        return;
    };
    let Some((_, rule)) = COMMANDS.iter().find(|(allowed, _)| *allowed == name) else {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            name,
            "is not a command on the allowlist",
        );
        return;
    };
    let params = map.get("params").unwrap_or(&NULL);
    check(params, rule, &format!("{name}.params"), line, out);
    if name == PROBE_COMMAND {
        check_probe_form(params, line, out);
    }
}

/// A probe call holds exactly one of its four forms, and `frame` only with the points.
fn check_probe_form(params: &Value, line: u32, out: &mut Vec<Refusal>) {
    let Some(map) = params.as_object() else {
        return;
    };
    let held = PROBE_FORMS
        .iter()
        .filter(|form| map.contains_key(**form))
        .count();
    let path = format!("{PROBE_COMMAND}.params");
    if held != 1 {
        outside(
            out,
            line,
            &path,
            "must hold exactly one of points, line, forces or heat",
        );
    } else if map.contains_key(PARAM_FRAME) && !map.contains_key(PARAM_POINTS) {
        outside(out, line, &path, "names a frame without points");
    }
}

fn check_tool_call(params: &Value, line: u32, out: &mut Vec<Refusal>) {
    let Some(map) = params.as_object() else {
        refuse(out, NOT_ON_THE_LIST, line, "params", "must be an object");
        return;
    };
    for key in map
        .keys()
        .filter(|key| !["name", "arguments"].contains(&key.as_str()))
    {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            key,
            "is not a member of a tool call",
        );
    }
    let Some(tool) = map.get("name").and_then(Value::as_str) else {
        refuse(out, NOT_ON_THE_LIST, line, "name", "names no tool");
        return;
    };
    if !ALLOWED_TOOLS.contains(&tool) {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            tool,
            "is not a tool on the allowlist",
        );
        return;
    }
    let arguments = map.get("arguments").unwrap_or(&NULL);
    if tool != BATCH_TOOL {
        check_command(arguments, "arguments", line, out);
        return;
    }
    let Some(batch) = arguments.as_object() else {
        refuse(out, OUTSIDE_THE_SET, line, "arguments", "must be an object");
        return;
    };
    for key in batch.keys().filter(|key| key.as_str() != "commands") {
        refuse(
            out,
            OUTSIDE_THE_SET,
            line,
            &format!("arguments.{key}"),
            "is not a parameter of a batch",
        );
    }
    match batch.get("commands").and_then(Value::as_array) {
        Some(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                check_command(entry, &format!("commands[{index}]"), line, out);
            }
        }
        None => refuse(
            out,
            OUTSIDE_THE_SET,
            line,
            "arguments.commands",
            "must be a list",
        ),
    }
}

fn check_line(raw: &str, line: u32, out: &mut Vec<Refusal>) {
    if raw.len() >= MAX_LINE_BYTES {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            "line",
            &format!(
                "is {} bytes, and at most {MAX_LINE_BYTES} are allowed",
                raw.len()
            ),
        );
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        refuse(out, NOT_ON_THE_LIST, line, "line", "is not valid JSON");
        return;
    };
    let Some(top) = value.as_object() else {
        refuse(out, NOT_ON_THE_LIST, line, "line", "is not a JSON object");
        return;
    };
    for key in top
        .keys()
        .filter(|key| !["jsonrpc", "id", "method", "params"].contains(&key.as_str()))
    {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            key,
            "is not a member of a message",
        );
    }
    if top.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        refuse(out, NOT_ON_THE_LIST, line, "jsonrpc", "is not 2.0");
    }
    if !top.get("id").is_some_and(Value::is_number) {
        refuse(out, NOT_ON_THE_LIST, line, "id", "is not a number");
    }
    let Some(method) = top.get("method").and_then(Value::as_str) else {
        refuse(out, NOT_ON_THE_LIST, line, "method", "names no method");
        return;
    };
    if !ALLOWED_METHODS.contains(&method) {
        refuse(
            out,
            NOT_ON_THE_LIST,
            line,
            method,
            "is not a method on the allowlist",
        );
        return;
    }
    let params = top.get("params").unwrap_or(&NULL);
    if method == "initialize" {
        check(params, &INITIALIZE, "params", line, out);
    } else {
        check_tool_call(params, line, out);
    }
}

fn scan_commands(text: &str, out: &mut Vec<Refusal>) {
    if text.is_empty() {
        return;
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    for (index, raw) in body.split('\n').enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        check_line(raw, line, out);
    }
}

/// Whether a file of this name belongs to a case: the stream, the run script, the README, and
/// closed surfaces under `obstacles/`.
fn file_allowed(rel: &Path) -> bool {
    let parts: Option<Vec<&str>> = rel
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    match parts.as_deref() {
        Some([name]) => matches!(*name, "commands.jsonl" | "Allrun" | "README.md"),
        Some(["obstacles", name]) => {
            name.len() > ".3mf".len()
                && name.ends_with(".3mf")
                && !name.chars().any(char::is_control)
        }
        _ => false,
    }
}

/// Every construct of a written case that the backend refuses to run; empty when it is clean.
pub fn scan_tree(tree: &CaseTree) -> Vec<Refusal> {
    let mut out = Vec::new();
    for file in &tree.files {
        let name = file.rel_path.to_string_lossy().replace('\\', "/");
        if !file_allowed(&file.rel_path) {
            out.push(Refusal {
                code: OUTSIDE_THE_SET,
                file: clip(&name),
                line: 1,
                entry: clip(&name),
                message: "this file does not belong to a case".to_owned(),
            });
        } else if name == COMMANDS_FILE {
            match std::str::from_utf8(&file.bytes) {
                Ok(text) => scan_commands(text, &mut out),
                Err(_) => refuse(&mut out, NOT_ON_THE_LIST, 1, "file", "is not text"),
            }
        } else if name == "Allrun" && file.bytes != plan::allrun_text().as_bytes() {
            out.push(Refusal {
                code: OUTSIDE_THE_SET,
                file: name,
                line: 1,
                entry: "Allrun".to_owned(),
                message: "the run script is not the one the backend writes".to_owned(),
            });
        }
    }
    out
}
