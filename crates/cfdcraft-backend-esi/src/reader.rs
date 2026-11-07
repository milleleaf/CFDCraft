//! Reads the dictionaries the writer produces back into facts, with a tokeniser and parser of its
//! own. It shares no code with the writer or with the safety scan, so a fault in either cannot
//! hide a fault here.
//!
//! The reader fails closed: a file it cannot tokenise or fit to the shape it expects is an error,
//! never a silently skipped entry. An entry that is simply absent is an absent fact.

use cfdcraft_model::{
    BoundaryKind, Face,
    diagnostics::kind_name,
    roundtrip::{CaseReader, EMPTY_KIND, Fact, FactValue, ReadError, SourceFile, Span},
    speed_from_vector,
};
use std::collections::{BTreeMap, BTreeSet};

/// The expected side of the shaped studies; the only part of the reader that knows the patch table.
pub mod expected;
/// The multi-block mesh dictionary and the conditions of its patches.
mod layout;
/// The time step, the initialisation, buoyancy, inflow values, thermal and profile conditions.
mod physics;

/// Largest file the reader accepts.
const FILE_MAX: usize = 16 * 1024 * 1024;
/// Deepest nesting of braces and brackets the parser follows.
const DEPTH_MAX: usize = 32;

/// The written files as the reader takes them: paths relative to the case root with `/`
/// separators.
pub fn source_files(tree: &crate::case::CaseTree) -> Vec<SourceFile> {
    tree.files
        .iter()
        .map(|file| SourceFile {
            path: file
                .rel_path
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            bytes: file.bytes.clone(),
        })
        .collect()
}

/// Reads an ESI v2512 case.
#[derive(Debug, Clone, Copy, Default)]
pub struct EsiReader;

impl CaseReader for EsiReader {
    /// Reads `system/blockMeshDict`, `0/U`, `0/p`, `constant/transportProperties`,
    /// `constant/turbulenceProperties`, `system/controlDict` and `system/fvSolution`, and, when the
    /// case has them, `constant/g`, `0/p_rgh`, `0/T`, `0/k`, `0/omega` and `0/epsilon` (a case
    /// without them is a case without buoyancy or turbulence). It does not read
    /// `system/fvSchemes`: the schemes are numerics the study does not parameterise, and the
    /// golden case bytes cover them.
    fn read(&self, files: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError> {
        let block_mesh = Doc::parse(files, "system/blockMeshDict")?;
        let velocity = Doc::parse(files, "0/U")?;
        let plain_pressure = Doc::parse(files, "0/p")?;
        let flux_pressure = Doc::parse_optional(files, "0/p_rgh")?;
        let temperature = Doc::parse_optional(files, "0/T")?;
        let transport = Doc::parse(files, "constant/transportProperties")?;
        let gravity = Doc::parse_optional(files, "constant/g")?;
        let turbulence = Doc::parse(files, "constant/turbulenceProperties")?;
        let control = Doc::parse(files, "system/controlDict")?;
        let solution = Doc::parse(files, "system/fvSolution")?;
        let inflow = physics::Inflow {
            k: Doc::parse_optional(files, "0/k")?,
            omega: Doc::parse_optional(files, "0/omega")?,
            epsilon: Doc::parse_optional(files, "0/epsilon")?,
        };
        // A buoyant case solves for `p_rgh`, so that file says what each patch does to the
        // pressure; the plain `p` it also holds is worked out by the solver.
        let fields = Fields {
            velocity: &velocity,
            pressure: flux_pressure.as_ref().unwrap_or(&plain_pressure),
            temperature: temperature.as_ref(),
            buoyant: flux_pressure.is_some(),
        };
        let mut facts = BTreeMap::new();
        let mut kinds = BTreeMap::new();
        mesh_and_boundary_facts(&block_mesh, &fields, &mut facts, &mut kinds)?;
        transport_facts(&transport, &mut facts)?;
        physics::buoyancy_facts(&transport, gravity.as_ref(), &mut facts)?;
        physics::inflow_facts(&inflow, &mut facts)?;
        turbulence_facts(&turbulence, &mut facts)?;
        control_facts(&control, &kinds, &mut facts)?;
        solution_facts(&solution, &mut facts)?;
        Ok(facts)
    }
}

/// The field files that say what each patch of the mesh is.
struct Fields<'a> {
    velocity: &'a Doc,
    /// `p`, or `p_rgh` for a buoyant case.
    pressure: &'a Doc,
    temperature: Option<&'a Doc>,
    /// The pressure file is `p_rgh`, whose patches take flux where `p` is zero-gradient.
    buoyant: bool,
}

/// What each written patch is, by its name, in the study's words; only the patches the field
/// files let the reader recognise.
type PatchKinds = BTreeMap<String, String>;

// ---- syntax tree --------------------------------------------------------------------------

/// A byte range and the line it starts on.
#[derive(Debug, Clone, Copy)]
struct Sp {
    start: usize,
    end: usize,
    line: u64,
}

impl Sp {
    fn join(self, other: Sp) -> Sp {
        Sp {
            start: self.start,
            end: other.end,
            line: self.line,
        }
    }
}

#[derive(Debug, Clone)]
struct Atom {
    text: String,
    sp: Sp,
}

#[derive(Debug, Clone)]
struct List {
    open: u8,
    items: Vec<Item>,
    sp: Sp,
}

#[derive(Debug, Clone)]
struct Block {
    entries: Vec<Entry>,
    sp: Sp,
}

#[derive(Debug, Clone)]
enum Item {
    Atom(Atom),
    List(List),
    Block(Block),
}

impl Item {
    fn sp(&self) -> Sp {
        match self {
            Item::Atom(a) => a.sp,
            Item::List(l) => l.sp,
            Item::Block(b) => b.sp,
        }
    }
}

#[derive(Debug, Clone)]
enum Body {
    Items(Vec<Item>),
    Block(Block),
}

#[derive(Debug, Clone)]
struct Entry {
    key: String,
    body: Body,
    sp: Sp,
}

// ---- tokeniser ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Quoted,
    Punct(u8),
}

#[derive(Debug, Clone)]
struct Tok {
    kind: Kind,
    text: String,
    sp: Sp,
}

fn is_punct(b: u8) -> bool {
    matches!(b, b'{' | b'}' | b'(' | b')' | b'[' | b']' | b';')
}

fn tokenize(path: &str, text: &str) -> Result<Vec<Tok>, ReadError> {
    let bytes = text.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut line = 1u64;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
        } else if b.is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            let at = line;
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return Err(fail(path, at, "a comment is never closed"));
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                if bytes[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
        } else if b == b'"' {
            let (start, at) = (i, line);
            i += 1;
            loop {
                match bytes.get(i) {
                    None => return Err(fail(path, at, "a string is never closed")),
                    Some(b'\\') => i += 2,
                    Some(b'"') => break,
                    Some(b'\n') => {
                        line += 1;
                        i += 1;
                    }
                    Some(_) => i += 1,
                }
            }
            toks.push(Tok {
                kind: Kind::Quoted,
                text: text[start + 1..i].to_owned(),
                sp: Sp {
                    start,
                    end: i + 1,
                    line: at,
                },
            });
            i += 1;
        } else if is_punct(b) {
            toks.push(Tok {
                kind: Kind::Punct(b),
                text: String::new(),
                sp: Sp {
                    start: i,
                    end: i + 1,
                    line,
                },
            });
            i += 1;
        } else {
            let start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !is_punct(bytes[i])
                && bytes[i] != b'"'
            {
                i += 1;
            }
            toks.push(Tok {
                kind: Kind::Word,
                text: text[start..i].to_owned(),
                sp: Sp {
                    start,
                    end: i,
                    line,
                },
            });
        }
    }
    Ok(toks)
}

fn fail(path: &str, line: u64, message: impl Into<String>) -> ReadError {
    ReadError {
        file: path.to_owned(),
        line,
        message: message.into(),
    }
}

// ---- parser -------------------------------------------------------------------------------

struct Parser<'a> {
    path: &'a str,
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, line: u64, message: impl Into<String>) -> ReadError {
        fail(self.path, line, message)
    }

    fn last_line(&self) -> u64 {
        self.toks.last().map_or(1, |t| t.sp.line)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// Entries up to the closing brace (`nested`) or the end of the file.
    fn entries(
        &mut self,
        nested: bool,
        depth: usize,
    ) -> Result<(Vec<Entry>, Option<Sp>), ReadError> {
        let mut entries: Vec<Entry> = Vec::new();
        loop {
            match self.peek() {
                None if nested => {
                    return Err(self.err(self.last_line(), "a brace is never closed"));
                }
                None => return Ok((entries, None)),
                Some(t) if t.kind == Kind::Punct(b'}') => {
                    if !nested {
                        return Err(self.err(t.sp.line, "a closing brace has no opening brace"));
                    }
                    let sp = t.sp;
                    self.pos += 1;
                    return Ok((entries, Some(sp)));
                }
                Some(_) => {
                    let entry = self.entry(depth)?;
                    if entries.iter().any(|e| e.key == entry.key) {
                        return Err(self.err(
                            entry.sp.line,
                            format!(
                                "the entry \"{}\" appears twice in one dictionary",
                                entry.key
                            ),
                        ));
                    }
                    entries.push(entry);
                }
            }
        }
    }

    fn entry(&mut self, depth: usize) -> Result<Entry, ReadError> {
        if depth > DEPTH_MAX {
            return Err(self.err(self.last_line(), "nesting is too deep"));
        }
        let key = self.toks[self.pos].clone();
        if matches!(key.kind, Kind::Punct(_)) {
            return Err(self.err(key.sp.line, "an entry name was expected"));
        }
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            let Some(tok) = self.peek().cloned() else {
                return Err(self.err(
                    key.sp.line,
                    format!("the entry \"{}\" is never ended", key.text),
                ));
            };
            match tok.kind {
                Kind::Punct(b';') => {
                    self.pos += 1;
                    return Ok(Entry {
                        key: key.text,
                        body: Body::Items(items),
                        sp: key.sp.join(tok.sp),
                    });
                }
                Kind::Punct(b'{') if items.is_empty() => {
                    let block = self.block(depth + 1)?;
                    let sp = key.sp.join(block.sp);
                    return Ok(Entry {
                        key: key.text,
                        body: Body::Block(block),
                        sp,
                    });
                }
                Kind::Punct(b'}') => {
                    return Err(self.err(
                        tok.sp.line,
                        format!("the entry \"{}\" is not ended with a semicolon", key.text),
                    ));
                }
                _ => items.push(self.item(depth + 1)?),
            }
        }
    }

    fn block(&mut self, depth: usize) -> Result<Block, ReadError> {
        let open = self.toks[self.pos].sp;
        self.pos += 1;
        let (entries, close) = self.entries(true, depth)?;
        let close = close.unwrap_or(open);
        Ok(Block {
            entries,
            sp: open.join(close),
        })
    }

    fn item(&mut self, depth: usize) -> Result<Item, ReadError> {
        if depth > DEPTH_MAX {
            return Err(self.err(self.last_line(), "nesting is too deep"));
        }
        let tok = self.toks[self.pos].clone();
        match tok.kind {
            Kind::Word | Kind::Quoted => {
                self.pos += 1;
                Ok(Item::Atom(Atom {
                    text: tok.text,
                    sp: tok.sp,
                }))
            }
            Kind::Punct(open @ (b'(' | b'[')) => {
                self.pos += 1;
                let close = if open == b'(' { b')' } else { b']' };
                let mut items = Vec::new();
                loop {
                    let Some(next) = self.peek().cloned() else {
                        return Err(self.err(tok.sp.line, "a bracket is never closed"));
                    };
                    match next.kind {
                        Kind::Punct(c) if c == close => {
                            self.pos += 1;
                            return Ok(Item::List(List {
                                open,
                                items,
                                sp: tok.sp.join(next.sp),
                            }));
                        }
                        Kind::Punct(b'{') => items.push(Item::Block(self.block(depth + 1)?)),
                        Kind::Punct(b')' | b']' | b'}' | b';') => {
                            return Err(self
                                .err(next.sp.line, "a bracket is closed by the wrong character"));
                        }
                        _ => items.push(self.item(depth + 1)?),
                    }
                }
            }
            Kind::Punct(_) => Err(self.err(tok.sp.line, "an unexpected character")),
        }
    }
}

// ---- documents and accessors --------------------------------------------------------------

struct Doc {
    path: String,
    entries: Vec<Entry>,
}

/// A number token with where it sits.
#[derive(Debug, Clone)]
struct Num {
    v: f64,
    text: String,
    sp: Sp,
}

impl Doc {
    fn parse(files: &[SourceFile], path: &str) -> Result<Doc, ReadError> {
        let mut matching = files.iter().filter(|f| f.path == path);
        let file = matching
            .next()
            .ok_or_else(|| fail(path, 0, "the file is missing from the written case"))?;
        if matching.next().is_some() {
            return Err(fail(path, 0, "the file appears twice in the written case"));
        }
        if file.bytes.len() > FILE_MAX {
            return Err(fail(path, 0, "the file is too large to read back"));
        }
        let text =
            std::str::from_utf8(&file.bytes).map_err(|_| fail(path, 0, "the file is not text"))?;
        let toks = tokenize(path, text)?;
        let mut parser = Parser { path, toks, pos: 0 };
        let (entries, _) = parser.entries(false, 0)?;
        Ok(Doc {
            path: path.to_owned(),
            entries,
        })
    }

    /// A file that a case may or may not have; one that is there is read like any other.
    fn parse_optional(files: &[SourceFile], path: &str) -> Result<Option<Doc>, ReadError> {
        if files.iter().any(|f| f.path == path) {
            Doc::parse(files, path).map(Some)
        } else {
            Ok(None)
        }
    }

    fn err(&self, sp: Sp, message: impl Into<String>) -> ReadError {
        fail(&self.path, sp.line, message)
    }

    fn span(&self, sp: Sp) -> Span {
        Span {
            file: self.path.clone(),
            line: sp.line,
            start: sp.start,
            end: sp.end,
        }
    }

    fn top(&self, key: &str) -> Option<&Entry> {
        sub(&self.entries, key)
    }

    /// The items of a plain entry, or an error when it is a dictionary.
    fn items<'a>(&self, entry: &'a Entry) -> Result<&'a [Item], ReadError> {
        match &entry.body {
            Body::Items(items) => Ok(items),
            Body::Block(_) => Err(self.err(
                entry.sp,
                format!("\"{}\" should hold a value, not a dictionary", entry.key),
            )),
        }
    }

    /// The dictionary an entry holds.
    fn block<'a>(&self, entry: &'a Entry) -> Result<&'a Block, ReadError> {
        match &entry.body {
            Body::Block(block) => Ok(block),
            Body::Items(_) => Err(self.err(
                entry.sp,
                format!("\"{}\" should be a dictionary", entry.key),
            )),
        }
    }

    /// The one word an entry holds.
    fn word<'a>(&self, entry: &'a Entry) -> Result<&'a Atom, ReadError> {
        match self.items(entry)? {
            [Item::Atom(atom)] => Ok(atom),
            _ => Err(self.err(entry.sp, format!("\"{}\" should hold one word", entry.key))),
        }
    }

    fn num(&self, atom: &Atom) -> Result<Num, ReadError> {
        parse_number(&atom.text)
            .map(|v| Num {
                v,
                text: atom.text.clone(),
                sp: atom.sp,
            })
            .ok_or_else(|| self.err(atom.sp, format!("\"{}\" is not a finite number", atom.text)))
    }

    fn count(&self, atom: &Atom) -> Result<u64, ReadError> {
        parse_count(&atom.text)
            .ok_or_else(|| self.err(atom.sp, format!("\"{}\" is not a whole number", atom.text)))
    }

    /// The numbers of a list with exactly `n` items.
    fn nums(&self, list: &List, n: usize) -> Result<Vec<Num>, ReadError> {
        if list.items.len() != n {
            return Err(self.err(list.sp, format!("{n} numbers were expected")));
        }
        list.items
            .iter()
            .map(|item| match item {
                Item::Atom(atom) => self.num(atom),
                other => Err(self.err(other.sp(), "a number was expected")),
            })
            .collect()
    }

    fn triple(&self, list: &List) -> Result<[Num; 3], ReadError> {
        <[Num; 3]>::try_from(self.nums(list, 3)?)
            .map_err(|_| self.err(list.sp, "three numbers were expected"))
    }

    /// The one bracketed list an entry holds, opened by `open`.
    fn list<'a>(&self, entry: &'a Entry, open: u8) -> Result<&'a List, ReadError> {
        match self.items(entry)? {
            [Item::List(list)] if list.open == open => Ok(list),
            _ => Err(self.err(
                entry.sp,
                format!("\"{}\" should hold one bracketed list", entry.key),
            )),
        }
    }

    /// `( name { ... } name { ... } )`: the named dictionaries of a list.
    fn named_blocks<'a>(&self, entry: &'a Entry) -> Result<Vec<(&'a Atom, &'a Block)>, ReadError> {
        let list = self.list(entry, b'(')?;
        let mut out: Vec<(&Atom, &Block)> = Vec::new();
        for pair in list.items.chunks(2) {
            let [Item::Atom(name), Item::Block(block)] = pair else {
                return Err(self.err(
                    pair[0].sp(),
                    format!("\"{}\" should alternate names and dictionaries", entry.key),
                ));
            };
            if out.iter().any(|(n, _)| n.text == name.text) {
                return Err(self.err(
                    name.sp,
                    format!("\"{}\" appears twice in \"{}\"", name.text, entry.key),
                ));
            }
            out.push((name, block));
        }
        Ok(out)
    }
}

fn sub<'a>(entries: &'a [Entry], key: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.key == key)
}

/// A finite decimal number: digits, an optional sign, point and exponent. Words such as `inf`
/// and `nan` are refused.
fn parse_number(text: &str) -> Option<f64> {
    let first = text.bytes().next()?;
    if !(first.is_ascii_digit() || matches!(first, b'-' | b'+' | b'.')) {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A whole number written with digits only.
fn parse_count(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn fact(value: FactValue, spans: Vec<Span>, entry: Span) -> Fact {
    Fact {
        value,
        value_spans: spans,
        entry_span: Some(entry),
    }
}

// ---- constant/transportProperties ---------------------------------------------------------

/// The number of `name [dimensions] value;`: seven numbers in square brackets, then the value.
fn dimensioned(doc: &Doc, entry: &Entry) -> Result<Num, ReadError> {
    match doc.items(entry)? {
        [Item::List(dims), Item::Atom(value)]
            if dims.open == b'['
                && dims.items.len() == 7
                && dims
                    .items
                    .iter()
                    .all(|d| matches!(d, Item::Atom(a) if parse_number(&a.text).is_some())) =>
        {
            doc.num(value)
        }
        _ => Err(doc.err(
            entry.sp,
            format!(
                "\"{}\" should be a seven-number dimension set in square brackets followed by a number",
                entry.key
            ),
        )),
    }
}

fn transport_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    let Some(nu) = doc.top("nu") else {
        return Ok(());
    };
    let value = dimensioned(doc, nu)?;
    facts.insert(
        "/fluid/kinematic_viscosity".into(),
        fact(
            FactValue::Number(value.v),
            vec![doc.span(value.sp)],
            doc.span(nu.sp),
        ),
    );
    Ok(())
}

// ---- constant/turbulenceProperties --------------------------------------------------------

fn turbulence_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    let Some(entry) = doc.top("simulationType") else {
        return Ok(());
    };
    let word = doc.word(entry)?;
    // A model is named inside the `RAS` dictionary; a laminar case names itself.
    let (name, at, held_by) = if word.text == "RAS" {
        let ras = doc
            .top("RAS")
            .ok_or_else(|| doc.err(entry.sp, "a RAS case has no \"RAS\" dictionary"))?;
        let model = required(doc, ras, doc.block(ras)?, "RASModel")?;
        let name = doc.word(model)?;
        (name.text.clone(), name.sp, model.sp)
    } else {
        (word.text.clone(), word.sp, entry.sp)
    };
    facts.insert(
        "/solver/turbulence".into(),
        fact(FactValue::Text(name), vec![doc.span(at)], doc.span(held_by)),
    );
    Ok(())
}

// ---- system/controlDict -------------------------------------------------------------------

fn control_facts(
    doc: &Doc,
    kinds: &PatchKinds,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    if let Some(entry) = doc.top("application") {
        let word = doc.word(entry)?;
        facts.insert(
            "/solver/application".into(),
            fact(
                FactValue::Text(word.text.clone()),
                vec![doc.span(word.sp)],
                doc.span(entry.sp),
            ),
        );
        physics::time_facts(doc, &word.text, facts)?;
    }
    for (key, name) in [
        ("endTime", "/solver/end_time"),
        ("writeInterval", "/solver/write_interval"),
    ] {
        if let Some(entry) = doc.top(key) {
            let word = doc.word(entry)?;
            facts.insert(
                name.into(),
                fact(
                    FactValue::Number(doc.num(word)?.v),
                    vec![doc.span(word.sp)],
                    doc.span(entry.sp),
                ),
            );
        }
    }
    let Some(functions) = doc.top("functions") else {
        return Ok(());
    };
    let functions = doc.block(functions)?;
    if let Some(centreline) = sub(&functions.entries, "centreline") {
        centreline_facts(doc, centreline, facts)?;
    }
    monitor_facts(doc, functions, kinds, facts)
}

/// The centreline profiles are the two sampled sets of the `centreline` function object.
fn centreline_facts(
    doc: &Doc,
    centreline: &Entry,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let Some(sets) = sub(&doc.block(centreline)?.entries, "sets") else {
        return Ok(());
    };
    for (name, block) in doc.named_blocks(sets)? {
        let profile = match name.text.as_str() {
            "vertical" | "horizontal" => name.text.as_str(),
            _ => continue,
        };
        if let Some(points) = sub(&block.entries, "nPoints") {
            let word = doc.word(points)?;
            facts.insert(
                format!("/monitors/centreline_u_sample_count/{profile}"),
                fact(
                    FactValue::Count(doc.count(word)?),
                    vec![doc.span(word.sp)],
                    doc.span(points.sp),
                ),
            );
        }
    }
    Ok(())
}

/// The kind and number of a function object named `probe_<n>`, `line_<n>`, `force_<n>` or
/// `surface_<n>`. Any other name (the centreline, the balances, the gradient) is not a monitor of
/// the study.
fn monitor_name(key: &str) -> Option<(&str, &str)> {
    let (kind, n) = key.split_once('_')?;
    let counted = parse_count(n).is_some_and(|v| v >= 1 && v.to_string() == n);
    (counted && matches!(kind, "probe" | "line" | "force" | "surface")).then_some((kind, n))
}

/// The probes, lines, forces and surfaces of the `functions` block. A recognised object with a
/// missing or malformed entry is an error; the names a study gave its monitors are not in the case.
fn monitor_facts(
    doc: &Doc,
    functions: &Block,
    kinds: &PatchKinds,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    for entry in &functions.entries {
        let Some((kind, n)) = monitor_name(&entry.key) else {
            continue;
        };
        let block = doc.block(entry)?;
        match kind {
            "probe" => probe_facts(doc, entry, block, n, facts)?,
            "line" => line_facts(doc, entry, block, n, facts)?,
            "force" => force_facts(doc, entry, block, n, facts)?,
            _ => surface_facts(doc, entry, block, n, kinds, facts)?,
        }
    }
    Ok(())
}

fn required<'a>(
    doc: &Doc,
    owner: &Entry,
    block: &'a Block,
    key: &str,
) -> Result<&'a Entry, ReadError> {
    sub(&block.entries, key).ok_or_else(|| {
        doc.err(
            owner.sp,
            format!("\"{}\" has no \"{key}\" entry", owner.key),
        )
    })
}

/// The one name an entry such as `fields (U);` lists.
fn listed_word<'a>(doc: &Doc, entry: &'a Entry) -> Result<&'a Atom, ReadError> {
    match doc.list(entry, b'(')?.items.as_slice() {
        [Item::Atom(atom)] => Ok(atom),
        _ => Err(doc.err(
            entry.sp,
            format!("\"{}\" should list exactly one name", entry.key),
        )),
    }
}

/// The monitored field of a probe or line, as the study words it.
fn monitor_field(doc: &Doc, owner: &Entry, block: &Block) -> Result<Fact, ReadError> {
    let entry = required(doc, owner, block, "fields")?;
    let atom = listed_word(doc, entry)?;
    let word = match atom.text.as_str() {
        "U" => "velocity",
        "p" => "pressure",
        other => {
            return Err(doc.err(atom.sp, format!("\"{other}\" is not a monitored field")));
        }
    };
    Ok(fact(
        FactValue::Text(word.into()),
        vec![doc.span(atom.sp)],
        doc.span(entry.sp),
    ))
}

/// The patch a monitor reads, from its `patches (name);` entry.
fn patch_fact(doc: &Doc, owner: &Entry, block: &Block) -> Result<Fact, ReadError> {
    let entry = required(doc, owner, block, "patches")?;
    let atom = listed_word(doc, entry)?;
    Ok(fact(
        FactValue::Text(atom.text.clone()),
        vec![doc.span(atom.sp)],
        doc.span(entry.sp),
    ))
}

/// A probe in the flow (`probes`) or on a wall (`patchProbes`, which also names its patch).
fn probe_facts(
    doc: &Doc,
    owner: &Entry,
    block: &Block,
    n: &str,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let kind = required(doc, owner, block, "type")?;
    let word = doc.word(kind)?;
    let on_boundary = match word.text.as_str() {
        "probes" => false,
        "patchProbes" => true,
        other => {
            return Err(doc.err(word.sp, format!("\"{other}\" is not a kind of probe")));
        }
    };
    let locations = required(doc, owner, block, "probeLocations")?;
    let outer = doc.list(locations, b'(')?;
    let [Item::List(point)] = outer.items.as_slice() else {
        return Err(doc.err(outer.sp, "a probe should list exactly one location"));
    };
    let [x, y, _] = doc.triple(point)?;
    for (axis, num) in [("x", &x), ("y", &y)] {
        facts.insert(
            format!("/monitors/probe/{n}/{axis}"),
            fact(
                FactValue::Number(num.v),
                vec![doc.span(num.sp)],
                doc.span(locations.sp),
            ),
        );
    }
    facts.insert(
        format!("/monitors/probe/{n}/field"),
        monitor_field(doc, owner, block)?,
    );
    if on_boundary {
        facts.insert(
            format!("/monitors/probe/{n}/on_boundary"),
            fact(
                FactValue::Flag(true),
                vec![doc.span(word.sp)],
                doc.span(kind.sp),
            ),
        );
        facts.insert(
            format!("/monitors/probe/{n}/patch"),
            patch_fact(doc, owner, block)?,
        );
    }
    Ok(())
}

/// The entry `key (a b c);` of a sampled line and its three numbers.
fn point_of<'a>(
    doc: &Doc,
    owner: &Entry,
    block: &'a Block,
    key: &str,
) -> Result<(&'a Entry, [Num; 3]), ReadError> {
    let entry = required(doc, owner, block, key)?;
    let numbers = doc.triple(doc.list(entry, b'(')?)?;
    Ok((entry, numbers))
}

fn line_facts(
    doc: &Doc,
    owner: &Entry,
    block: &Block,
    n: &str,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let sets = required(doc, owner, block, "sets")?;
    let named = doc.named_blocks(sets)?;
    let [(_, line)] = named.as_slice() else {
        return Err(doc.err(sets.sp, "a line object should hold exactly one set"));
    };
    let (start, [x0, y0, _]) = point_of(doc, owner, line, "start")?;
    let (end, [x1, y1, _]) = point_of(doc, owner, line, "end")?;
    for (name, num, entry) in [
        ("x0", &x0, start),
        ("y0", &y0, start),
        ("x1", &x1, end),
        ("y1", &y1, end),
    ] {
        facts.insert(
            format!("/monitors/line/{n}/{name}"),
            fact(
                FactValue::Number(num.v),
                vec![doc.span(num.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    let points = required(doc, owner, line, "nPoints")?;
    let word = doc.word(points)?;
    facts.insert(
        format!("/monitors/line/{n}/samples"),
        fact(
            FactValue::Count(doc.count(word)?),
            vec![doc.span(word.sp)],
            doc.span(points.sp),
        ),
    );
    facts.insert(
        format!("/monitors/line/{n}/field"),
        monitor_field(doc, owner, block)?,
    );
    Ok(())
}

/// The force coefficients of one wall: the reference speed and length, the axis the lift is taken
/// along and the patch.
fn force_facts(
    doc: &Doc,
    owner: &Entry,
    block: &Block,
    n: &str,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let kind = required(doc, owner, block, "type")?;
    if doc.word(kind)?.text != "forceCoeffs" {
        return Err(doc.err(kind.sp, "a force object should be of the kind forceCoeffs"));
    }
    for (key, name) in [
        ("magUInf", "reference_velocity"),
        ("lRef", "reference_length"),
    ] {
        let entry = required(doc, owner, block, key)?;
        let word = doc.word(entry)?;
        facts.insert(
            format!("/monitors/force/{n}/{name}"),
            fact(
                FactValue::Number(doc.num(word)?.v),
                vec![doc.span(word.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    let (lift, e3) = point_of(doc, owner, block, "e3")?;
    let axis = match [e3[0].v, e3[1].v, e3[2].v] {
        [x, y, z] if x == 1.0 && y == 0.0 && z == 0.0 => "x",
        [x, y, z] if x == 0.0 && y == 1.0 && z == 0.0 => "y",
        [x, y, z] if x == 0.0 && y == 0.0 && z == 1.0 => "z",
        _ => {
            return Err(doc.err(lift.sp, "the lift direction should be an axis of the case"));
        }
    };
    facts.insert(
        format!("/monitors/force/{n}/lift_axis"),
        fact(
            FactValue::Text(axis.into()),
            e3.iter().map(|v| doc.span(v.sp)).collect(),
            doc.span(lift.sp),
        ),
    );
    facts.insert(
        format!("/monitors/force/{n}/patch"),
        patch_fact(doc, owner, block)?,
    );
    Ok(())
}

fn surface_facts(
    doc: &Doc,
    owner: &Entry,
    block: &Block,
    n: &str,
    kinds: &PatchKinds,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let name = required(doc, owner, block, "name")?;
    let patch = doc.word(name)?;
    let kind = kinds.get(&patch.text).ok_or_else(|| {
        doc.err(
            patch.sp,
            format!("\"{}\" is not a patch of the case", patch.text),
        )
    })?;
    facts.insert(
        format!("/monitors/surface/{n}/kind"),
        fact(
            FactValue::Text(kind.clone()),
            vec![doc.span(patch.sp)],
            doc.span(name.sp),
        ),
    );
    let operation_entry = required(doc, owner, block, "operation")?;
    let operation = doc.word(operation_entry)?;
    let field_entry = required(doc, owner, block, "fields")?;
    let field = listed_word(doc, field_entry)?;
    let quantity = match (operation.text.as_str(), field.text.as_str()) {
        ("sum", "phi") => "flux",
        ("areaAverage", "p") => "pressure_average",
        ("areaIntegrate", "p") => "pressure_integral",
        ("areaIntegrate", "gradT") => "heat_flux",
        (op, f) => {
            return Err(doc.err(
                operation_entry.sp,
                format!("the operation \"{op}\" on \"{f}\" is not a monitored quantity"),
            ));
        }
    };
    facts.insert(
        format!("/monitors/surface/{n}/quantity"),
        fact(
            FactValue::Text(quantity.into()),
            vec![doc.span(operation.sp), doc.span(field.sp)],
            doc.span(operation_entry.sp),
        ),
    );
    Ok(())
}

// ---- system/fvSolution --------------------------------------------------------------------

/// A closed domain has no fixed-pressure boundary, so the solver settings carry a pressure
/// reference; an open domain has none. A steady run keeps its settings under `SIMPLE`, a transient
/// one under `PIMPLE`.
fn solution_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    physics::init_fact(doc, facts);
    let Some(algorithm) = doc.top("SIMPLE").or_else(|| doc.top("PIMPLE")) else {
        return Ok(());
    };
    let block = doc.block(algorithm)?;
    let (open, held_by) = match sub(&block.entries, "pRefCell") {
        Some(reference) => (false, reference.sp),
        None => (true, algorithm.sp),
    };
    facts.insert(
        "/domain/open".into(),
        fact(FactValue::Flag(open), vec![], doc.span(held_by)),
    );
    Ok(())
}

// ---- system/blockMeshDict, 0/U and 0/p ----------------------------------------------------

/// A side of the box, found from vertex coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    XMin,
    XMax,
    YMin,
    YMax,
    ZMin,
    ZMax,
}

impl Side {
    fn of(axis: usize, high: bool) -> Side {
        match (axis, high) {
            (0, false) => Side::XMin,
            (0, true) => Side::XMax,
            (1, false) => Side::YMin,
            (1, true) => Side::YMax,
            (_, false) => Side::ZMin,
            (_, true) => Side::ZMax,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Side::XMin => "XMin",
            Side::XMax => "XMax",
            Side::YMin => "YMin",
            Side::YMax => "YMax",
            Side::ZMin => "ZMin",
            Side::ZMax => "ZMax",
        }
    }

    /// The model's face, for the four sides a boundary can be on.
    fn face(self) -> Option<Face> {
        match self {
            Side::XMin => Some(Face::XMin),
            Side::XMax => Some(Face::XMax),
            Side::YMin => Some(Face::YMin),
            Side::YMax => Some(Face::YMax),
            Side::ZMin | Side::ZMax => None,
        }
    }
}

/// The eight corners of the block, in the order a hexahedron lists them, as read.
struct Geometry {
    corners: Vec<[Num; 3]>,
    lo: [f64; 3],
    hi: [f64; 3],
}

/// Which end of each axis the eight corners of a hexahedron sit at, in listing order.
const CORNER_ENDS: [[bool; 3]; 8] = [
    [false, false, false],
    [true, false, false],
    [true, true, false],
    [false, true, false],
    [false, false, true],
    [true, false, true],
    [true, true, true],
    [false, true, true],
];

fn geometry(doc: &Doc, vertices: &Entry) -> Result<Geometry, ReadError> {
    let outer = doc.list(vertices, b'(')?;
    if outer.items.len() != 8 {
        return Err(doc.err(outer.sp, "the block needs exactly eight vertices"));
    }
    let mut corners = Vec::new();
    for item in &outer.items {
        let Item::List(point) = item else {
            return Err(doc.err(item.sp(), "a vertex should be three numbers in brackets"));
        };
        if point.open != b'(' {
            return Err(doc.err(point.sp, "a vertex should be in round brackets"));
        }
        corners.push(doc.triple(point)?);
    }
    let end = |corner: usize, axis: usize| corners[corner][axis].v;
    // Corner 0 is the low end of every axis; corners 1, 3 and 4 are the high end of one axis.
    let lo = [end(0, 0), end(0, 1), end(0, 2)];
    let hi = [end(1, 0), end(3, 1), end(4, 2)];
    for axis in 0..3 {
        if lo[axis] >= hi[axis] {
            return Err(doc.err(
                vertices.sp,
                "the vertices do not span a box with a positive length on every axis",
            ));
        }
    }
    for (index, ends) in CORNER_ENDS.iter().enumerate() {
        for axis in 0..3 {
            let want = if ends[axis] { hi[axis] } else { lo[axis] };
            if end(index, axis) != want {
                return Err(doc.err(
                    corners[index][axis].sp,
                    "the vertices are not the corners of an axis-aligned box in the order the block lists them",
                ));
            }
        }
    }
    Ok(Geometry { corners, lo, hi })
}

/// Which side of the box the four vertices of a face lie on.
fn side_of(doc: &Doc, geo: &Geometry, tuple: &List) -> Result<Side, ReadError> {
    if tuple.open != b'(' || tuple.items.len() != 4 {
        return Err(doc.err(tuple.sp, "a face should be four vertex numbers in brackets"));
    }
    let mut index = Vec::new();
    for item in &tuple.items {
        let Item::Atom(atom) = item else {
            return Err(doc.err(item.sp(), "a vertex number was expected"));
        };
        let i = parse_count(&atom.text)
            .ok_or_else(|| doc.err(atom.sp, format!("\"{}\" is not a vertex number", atom.text)))?;
        if i >= 8 {
            return Err(doc.err(
                atom.sp,
                format!("vertex {i} does not exist, the block has eight"),
            ));
        }
        index.push(i as usize);
    }
    if index.iter().collect::<BTreeSet<_>>().len() != 4 {
        return Err(doc.err(tuple.sp, "a face needs four different vertices"));
    }
    let flat: Vec<(usize, f64)> = (0..3)
        .filter_map(|axis| {
            let c = geo.corners[index[0]][axis].v;
            index
                .iter()
                .all(|&i| geo.corners[i][axis].v == c)
                .then_some((axis, c))
        })
        .collect();
    let [(axis, at)] = flat[..] else {
        return Err(doc.err(tuple.sp, "a face does not lie on one plane of the box"));
    };
    if at == geo.lo[axis] {
        Ok(Side::of(axis, false))
    } else if at == geo.hi[axis] {
        Ok(Side::of(axis, true))
    } else {
        Err(doc.err(tuple.sp, "a face does not lie on a side of the box"))
    }
}

struct MeshPatch<'a> {
    name: &'a Atom,
    mesh_type: &'a Atom,
    sides: Vec<(Side, Sp)>,
    entry: Sp,
}

fn mesh_patches<'a>(
    doc: &Doc,
    boundary: &'a Entry,
    geo: &Geometry,
) -> Result<Vec<MeshPatch<'a>>, ReadError> {
    let mut patches = Vec::new();
    for (name, block) in doc.named_blocks(boundary)? {
        let mesh_type = match sub(&block.entries, "type") {
            Some(entry) => doc.word(entry)?,
            None => {
                return Err(doc.err(name.sp, format!("the patch \"{}\" has no type", name.text)));
            }
        };
        let faces = match sub(&block.entries, "faces") {
            Some(entry) => doc.list(entry, b'(')?,
            None => {
                return Err(doc.err(name.sp, format!("the patch \"{}\" has no faces", name.text)));
            }
        };
        let mut sides = Vec::new();
        for item in &faces.items {
            let Item::List(tuple) = item else {
                return Err(doc.err(item.sp(), "each face should be a list of vertex numbers"));
            };
            sides.push((side_of(doc, geo, tuple)?, tuple.sp));
        }
        patches.push(MeshPatch {
            name,
            mesh_type,
            sides,
            entry: name.sp.join(block.sp),
        });
    }
    Ok(patches)
}

/// One patch of a field file: its type, its `value` entry if any, and the dictionary it is in.
struct FieldPatch<'a> {
    ty: &'a Atom,
    value: Option<&'a Entry>,
    block: &'a Block,
    entry: Sp,
}

type FieldPatches<'a> = BTreeMap<String, FieldPatch<'a>>;

fn field_patches(doc: &Doc) -> Result<FieldPatches<'_>, ReadError> {
    let mut out = BTreeMap::new();
    let Some(field) = doc.top("boundaryField") else {
        return Ok(out);
    };
    for entry in &doc.block(field)?.entries {
        let block = doc.block(entry)?;
        let ty = match sub(&block.entries, "type") {
            Some(t) => doc.word(t)?,
            None => {
                return Err(doc.err(entry.sp, format!("the patch \"{}\" has no type", entry.key)));
            }
        };
        out.insert(
            entry.key.clone(),
            FieldPatch {
                ty,
                value: sub(&block.entries, "value"),
                block,
                entry: entry.sp,
            },
        );
    }
    Ok(out)
}

/// The mesh and the field files must name the same patches: `names` are the patches of the mesh
/// dictionary in the order it lists them, `sets` each field file with the patches it holds.
fn match_patch_names(
    doc: &Doc,
    names: &[&Atom],
    sets: &[(&Doc, &FieldPatches<'_>, &str)],
) -> Result<(), ReadError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name.text.as_str()) {
            return Err(doc.err(
                name.sp,
                format!("the patch \"{}\" appears twice", name.text),
            ));
        }
        for (_, field, label) in sets {
            if !field.contains_key(&name.text) {
                return Err(doc.err(
                    name.sp,
                    format!("the patch \"{}\" has no entry in {label}", name.text),
                ));
            }
        }
    }
    for (field_doc, field, _) in sets {
        for (name, patch) in *field {
            if !seen.contains(name.as_str()) {
                return Err(field_doc.err(
                    patch.ty.sp,
                    format!(
                        "the patch \"{name}\" is not a patch of the mesh in {}",
                        doc.path
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// What a patch is, from its mesh type and its velocity and pressure field types.
enum Recovered {
    Known(BoundaryKind),
    Empty,
    Other(String),
}

/// `buoyant` says the pressure file is `p_rgh`, whose patches take flux with `fixedFluxPressure`
/// where `p` holds `zeroGradient`; a buoyant case never reads `zeroGradient` there, and a case
/// that is not buoyant never reads `fixedFluxPressure`. An inlet is held either at a uniform value
/// or along a profile.
fn classify(
    mesh: &str,
    velocity: &str,
    pressure: &str,
    pressure_is_zero: bool,
    buoyant: bool,
) -> Recovered {
    let flux = if buoyant {
        "fixedFluxPressure"
    } else {
        "zeroGradient"
    };
    let takes_flux = pressure == flux;
    match (mesh, velocity, pressure) {
        ("patch", "fixedValue" | "fixedProfile", _) if takes_flux => {
            Recovered::Known(BoundaryKind::VelocityInlet)
        }
        ("patch", "zeroGradient", "fixedValue") if pressure_is_zero => {
            Recovered::Known(BoundaryKind::PressureOutlet)
        }
        ("wall", "noSlip", _) if takes_flux => Recovered::Known(BoundaryKind::NoSlipWall),
        ("wall", "fixedValue", _) if takes_flux => Recovered::Known(BoundaryKind::MovingWall),
        ("wall", "slip", _) if takes_flux => Recovered::Known(BoundaryKind::SlipWall),
        ("symmetryPlane", "symmetryPlane", "symmetryPlane")
        | ("symmetry", "symmetry", "symmetry") => Recovered::Known(BoundaryKind::Symmetry),
        ("empty", "empty", "empty") => Recovered::Empty,
        _ => Recovered::Other(format!(
            "unrecognised combination ({mesh}, {velocity}, {pressure}){}",
            if (mesh, velocity, pressure) == ("patch", "zeroGradient", "fixedValue") {
                " with a pressure value other than uniform 0"
            } else {
                ""
            }
        )),
    }
}

/// `value uniform 0;` of a pressure patch.
fn pressure_is_uniform_zero(doc: &Doc, patch: &FieldPatch) -> Result<bool, ReadError> {
    let Some(entry) = patch.value else {
        return Ok(false);
    };
    Ok(match doc.items(entry)? {
        [Item::Atom(kind), Item::Atom(value)] if kind.text == "uniform" => doc.num(value)?.v == 0.0,
        _ => false,
    })
}

/// `value uniform (a b c);` of a velocity patch.
fn uniform_vector(doc: &Doc, entry: &Entry) -> Result<[Num; 3], ReadError> {
    match doc.items(entry)? {
        [Item::Atom(kind), Item::List(vector)] if kind.text == "uniform" && vector.open == b'(' => {
            doc.triple(vector)
        }
        _ => Err(doc.err(
            entry.sp,
            "a velocity value should be uniform and three numbers",
        )),
    }
}

/// What a patch recovers as: the words of its kind, and the kind itself when it is one the study
/// can state. `pressure` is the file the patches of the case are judged on.
fn recover(
    fields: &Fields<'_>,
    mesh_type: &str,
    velocity: &FieldPatch<'_>,
    pressure: &FieldPatch<'_>,
) -> Result<(Recovered, String), ReadError> {
    let recovered = classify(
        mesh_type,
        &velocity.ty.text,
        &pressure.ty.text,
        pressure_is_uniform_zero(fields.pressure, pressure)?,
        fields.buoyant,
    );
    let text = match &recovered {
        Recovered::Known(kind) => kind_name(*kind).to_owned(),
        Recovered::Empty => EMPTY_KIND.to_owned(),
        Recovered::Other(text) => text.clone(),
    };
    Ok((recovered, text))
}

/// The speed an inlet or a moving wall imposes, for a patch that lies on `faces`. A fixed value
/// gives the component the kind allows on the face; a profiled inlet gives its mean speed, worked
/// out for an inlet that spans `extent` along y. A vector the faces cannot read, or faces that
/// disagree, give the vector as written.
fn speed_fact(
    doc: &Doc,
    kind: BoundaryKind,
    faces: &[Face],
    patch: &FieldPatch<'_>,
    extent: Option<(f64, f64)>,
) -> Result<Option<Fact>, ReadError> {
    if !matches!(kind, BoundaryKind::VelocityInlet | BoundaryKind::MovingWall) {
        return Ok(None);
    }
    // The vector as written, the tokens that carry it and the entry that holds it.
    let (raw, written, tokens, entry) = if patch.ty.text == "fixedProfile" {
        let Some((lo, hi)) = extent else {
            return Err(doc.err(patch.entry, "the extent of a profiled inlet is unknown"));
        };
        let mean = physics::profile_mean(doc, patch, lo, hi)?;
        (
            [mean.value, 0.0, 0.0],
            format!("({} 0 0)", mean.value),
            mean.spans,
            doc.span(mean.entry),
        )
    } else if let Some(value) = patch.value {
        let vector = uniform_vector(doc, value)?;
        (
            [vector[0].v, vector[1].v, vector[2].v],
            format!("({} {} {})", vector[0].text, vector[1].text, vector[2].text),
            vector.iter().map(|n| doc.span(n.sp)).collect(),
            doc.span(value.sp),
        )
    } else {
        return Ok(None);
    };
    let speeds: Vec<Option<f64>> = faces
        .iter()
        .map(|face| speed_from_vector(kind, *face, raw))
        .collect();
    let agreed = match speeds.first() {
        Some(first) if first.is_some() && speeds.iter().all(|s| s == first) => *first,
        _ => None,
    };
    Ok(Some(match (agreed, faces.first()) {
        (Some(speed), Some(face)) => {
            // The component the kind allows on this face is the one that carries the speed.
            let axis = (0..3)
                .find(|&i| {
                    let mut unit = [0.0; 3];
                    unit[i] = 1.0;
                    speed_from_vector(kind, *face, unit).is_some()
                })
                .unwrap_or(0);
            let carrier = tokens.get(axis).or(tokens.first()).cloned();
            fact(
                FactValue::Number(speed),
                carrier.into_iter().collect(),
                entry,
            )
        }
        _ => fact(FactValue::Text(written), tokens, entry),
    }))
}

/// A profiled inlet is a parabolic one.
fn profile_fact(doc: &Doc, kind: BoundaryKind, patch: &FieldPatch<'_>) -> Option<Fact> {
    (kind == BoundaryKind::VelocityInlet && patch.ty.text == "fixedProfile").then(|| {
        fact(
            FactValue::Text("parabolic".into()),
            vec![doc.span(patch.ty.sp)],
            doc.span(patch.entry),
        )
    })
}

fn mesh_and_boundary_facts(
    block_mesh: &Doc,
    fields: &Fields<'_>,
    facts: &mut BTreeMap<String, Fact>,
    kinds: &mut PatchKinds,
) -> Result<(), ReadError> {
    let doc = block_mesh;
    if layout::is_multi_block(doc)? {
        return layout::layout_facts(doc, fields, facts, kinds);
    }
    let (velocity, pressure) = (fields.velocity, fields.pressure);
    let geo = match doc.top("vertices") {
        Some(vertices) => {
            let geo = geometry(doc, vertices)?;
            for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
                let spans_at = |hi: bool| -> Vec<Span> {
                    CORNER_ENDS
                        .iter()
                        .enumerate()
                        .filter(|(_, ends)| ends[axis] == hi)
                        .map(|(i, _)| doc.span(geo.corners[i][axis].sp))
                        .collect()
                };
                facts.insert(
                    format!("/domain/length_{name}"),
                    fact(
                        FactValue::Number(geo.hi[axis]),
                        spans_at(true),
                        doc.span(vertices.sp),
                    ),
                );
                facts.insert(
                    format!("/domain/origin_{name}"),
                    fact(
                        FactValue::Number(geo.lo[axis]),
                        spans_at(false),
                        doc.span(vertices.sp),
                    ),
                );
            }
            Some(geo)
        }
        None => None,
    };
    if let Some(blocks) = doc.top("blocks") {
        block_facts(doc, blocks, facts)?;
    }

    let mesh_patches_list = match (doc.top("boundary"), &geo) {
        (Some(boundary), Some(geo)) => mesh_patches(doc, boundary, geo)?,
        (Some(boundary), None) => {
            return Err(doc.err(
                boundary.sp,
                "the patches refer to vertices, but the vertices are missing",
            ));
        }
        (None, _) => Vec::new(),
    };
    let u = field_patches(velocity)?;
    let p = field_patches(pressure)?;
    let t = fields.temperature.map(field_patches).transpose()?;

    // The mesh and every field file must name the same patches.
    let names: Vec<&Atom> = mesh_patches_list.iter().map(|patch| patch.name).collect();
    let pressure_label = if fields.buoyant { "0/p_rgh" } else { "0/p" };
    let mut sets = vec![(velocity, &u, "0/U"), (pressure, &p, pressure_label)];
    if let (Some(doc_t), Some(t)) = (fields.temperature, &t) {
        sets.push((doc_t, t, "0/T"));
    }
    match_patch_names(doc, &names, &sets)?;

    // Each side of the box belongs to one patch.
    let mut claimed: BTreeMap<Side, usize> = BTreeMap::new();
    for (index, patch) in mesh_patches_list.iter().enumerate() {
        for (side, sp) in &patch.sides {
            if let Some(other) = claimed.insert(*side, index) {
                return Err(doc.err(
                    *sp,
                    format!(
                        "the {} side of the box is listed by more than one patch (\"{}\" and \"{}\")",
                        side.name(),
                        mesh_patches_list[other].name.text,
                        patch.name.text
                    ),
                ));
            }
        }
    }

    let extent = geo.as_ref().map(|g| (g.lo[1], g.hi[1]));
    for patch in &mesh_patches_list {
        let (fu, fp) = (&u[&patch.name.text], &p[&patch.name.text]);
        let (recovered, text) = recover(fields, &patch.mesh_type.text, fu, fp)?;
        // Every patch has a kind as written, known or not, so a surface monitor on it reads that
        // kind and the mismatch is reported once, on the patch, not as an unreadable monitor.
        kinds.insert(patch.name.text.clone(), text.clone());
        let ft = t.as_ref().and_then(|t| t.get(&patch.name.text));
        for (side, tuple) in &patch.sides {
            let by_face =
                |name: &str| format!("/boundary_conditions/by_face/{}/{name}", side.name());
            facts.insert(
                by_face("kind"),
                fact(
                    FactValue::Text(text.clone()),
                    vec![
                        doc.span(patch.mesh_type.sp),
                        velocity.span(fu.ty.sp),
                        pressure.span(fp.ty.sp),
                        doc.span(*tuple),
                    ],
                    doc.span(patch.entry),
                ),
            );
            let (Recovered::Known(kind), Some(face)) = (&recovered, side.face()) else {
                continue;
            };
            if let Some(speed) = speed_fact(velocity, *kind, &[face], fu, extent)? {
                facts.insert(by_face("velocity"), speed);
            }
            if let Some(profile) = profile_fact(velocity, *kind, fu) {
                facts.insert(by_face("profile"), profile);
            }
            if let (Some(doc_t), Some(ft)) = (fields.temperature, ft) {
                for (name, thermal) in physics::thermal_facts(doc_t, *kind, ft)? {
                    facts.insert(by_face(name), thermal);
                }
            }
        }
    }
    Ok(())
}

fn block_facts(
    doc: &Doc,
    blocks: &Entry,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    // `( hex (0 1 2 3 4 5 6 7) (nx ny nz) simpleGrading (gx gy gz) )`: one box block.
    let outer = doc.list(blocks, b'(')?;
    let [
        Item::Atom(hex),
        Item::List(corners),
        Item::List(cells),
        Item::Atom(grading_kind),
        Item::List(grading),
    ] = &outer.items[..]
    else {
        return Err(doc.err(
            blocks.sp,
            "\"blocks\" should hold one hex block with simple grading",
        ));
    };
    if hex.text != "hex" || grading_kind.text != "simpleGrading" {
        return Err(doc.err(
            blocks.sp,
            "\"blocks\" should hold one hex block with simple grading",
        ));
    }
    let order: Vec<u64> = corners
        .items
        .iter()
        .map(|item| match item {
            Item::Atom(a) => doc.count(a),
            other => Err(doc.err(other.sp(), "a vertex number was expected")),
        })
        .collect::<Result<_, _>>()?;
    if order != [0, 1, 2, 3, 4, 5, 6, 7] {
        return Err(doc.err(
            corners.sp,
            "the block should list its vertices in order, 0 to 7",
        ));
    }
    if cells.items.len() != 3 {
        return Err(doc.err(cells.sp, "three cell counts were expected"));
    }
    for (item, axis) in cells.items.iter().zip(["x", "y", "z"]) {
        let Item::Atom(atom) = item else {
            return Err(doc.err(item.sp(), "a cell count was expected"));
        };
        facts.insert(
            format!("/mesh/cells_{axis}"),
            fact(
                FactValue::Count(doc.count(atom)?),
                vec![doc.span(atom.sp)],
                doc.span(blocks.sp),
            ),
        );
    }
    let (text, spans) = layout::grading_axes(doc, grading)?;
    facts.insert(
        "/mesh/grading".into(),
        fact(FactValue::Text(text), spans, doc.span(blocks.sp)),
    );
    Ok(())
}
