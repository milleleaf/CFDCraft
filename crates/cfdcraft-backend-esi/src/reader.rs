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

use crate::case::CaseTree;

/// Largest file the reader accepts.
const FILE_MAX: usize = 16 * 1024 * 1024;
/// Deepest nesting of braces and brackets the parser follows.
const DEPTH_MAX: usize = 32;

/// The written files as the reader takes them: paths relative to the case root with `/`
/// separators.
pub fn source_files(tree: &CaseTree) -> Vec<SourceFile> {
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
    /// `constant/turbulenceProperties`, `system/controlDict` and `system/fvSolution`. It does not
    /// read `system/fvSchemes`: the schemes are numerics the study does not parameterise, and the
    /// golden case bytes cover them.
    fn read(&self, files: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError> {
        let block_mesh = Doc::parse(files, "system/blockMeshDict")?;
        let velocity = Doc::parse(files, "0/U")?;
        let pressure = Doc::parse(files, "0/p")?;
        let transport = Doc::parse(files, "constant/transportProperties")?;
        let turbulence = Doc::parse(files, "constant/turbulenceProperties")?;
        let control = Doc::parse(files, "system/controlDict")?;
        let solution = Doc::parse(files, "system/fvSolution")?;
        let mut facts = BTreeMap::new();
        mesh_and_boundary_facts(&block_mesh, &velocity, &pressure, &mut facts)?;
        transport_facts(&transport, &mut facts)?;
        turbulence_facts(&turbulence, &mut facts)?;
        control_facts(&control, &mut facts)?;
        solution_facts(&solution, &mut facts)?;
        Ok(facts)
    }
}

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

fn transport_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    let Some(nu) = doc.top("nu") else {
        return Ok(());
    };
    // `nu [dimensions] value;`
    let value = match doc.items(nu)? {
        [Item::List(dims), Item::Atom(value)]
            if dims.open == b'['
                && dims.items.len() == 7
                && dims
                    .items
                    .iter()
                    .all(|d| matches!(d, Item::Atom(a) if parse_number(&a.text).is_some())) =>
        {
            value
        }
        _ => {
            return Err(doc.err(
                nu.sp,
                "\"nu\" should be a seven-number dimension set in square brackets followed by a number",
            ));
        }
    };
    facts.insert(
        "/fluid/kinematic_viscosity".into(),
        fact(
            FactValue::Number(doc.num(value)?.v),
            vec![doc.span(value.sp)],
            doc.span(nu.sp),
        ),
    );
    Ok(())
}

// ---- constant/turbulenceProperties --------------------------------------------------------

fn turbulence_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    if let Some(entry) = doc.top("simulationType") {
        let word = doc.word(entry)?;
        facts.insert(
            "/solver/turbulence".into(),
            fact(
                FactValue::Text(word.text.clone()),
                vec![doc.span(word.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    Ok(())
}

// ---- system/controlDict -------------------------------------------------------------------

fn control_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
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
    // The centreline profiles are the two sampled sets of the `centreline` function object.
    let Some(functions) = doc.top("functions") else {
        return Ok(());
    };
    let Some(centreline) = sub(&doc.block(functions)?.entries, "centreline") else {
        return Ok(());
    };
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

// ---- system/fvSolution --------------------------------------------------------------------

/// A closed domain has no fixed-pressure boundary, so the solver settings carry a pressure
/// reference; an open domain has none.
fn solution_facts(doc: &Doc, facts: &mut BTreeMap<String, Fact>) -> Result<(), ReadError> {
    let Some(simple) = doc.top("SIMPLE") else {
        return Ok(());
    };
    let block = doc.block(simple)?;
    let (open, held_by) = match sub(&block.entries, "pRefCell") {
        Some(reference) => (false, reference.sp),
        None => (true, simple.sp),
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

/// One patch of a field file: its type and its `value` entry, if any.
struct FieldPatch<'a> {
    ty: &'a Atom,
    value: Option<&'a Entry>,
}

fn field_patches(doc: &Doc) -> Result<BTreeMap<String, FieldPatch<'_>>, ReadError> {
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
            },
        );
    }
    Ok(out)
}

/// What a patch is, from its mesh type and its velocity and pressure field types.
enum Recovered {
    Known(BoundaryKind),
    Empty,
    Other(String),
}

fn classify(mesh: &str, velocity: &str, pressure: &str, pressure_is_zero: bool) -> Recovered {
    match (mesh, velocity, pressure) {
        ("patch", "fixedValue", "zeroGradient") => Recovered::Known(BoundaryKind::VelocityInlet),
        ("patch", "zeroGradient", "fixedValue") if pressure_is_zero => {
            Recovered::Known(BoundaryKind::PressureOutlet)
        }
        ("wall", "noSlip", "zeroGradient") => Recovered::Known(BoundaryKind::NoSlipWall),
        ("wall", "fixedValue", "zeroGradient") => Recovered::Known(BoundaryKind::MovingWall),
        ("wall", "slip", "zeroGradient") => Recovered::Known(BoundaryKind::SlipWall),
        ("symmetryPlane", "symmetryPlane", "symmetryPlane") => {
            Recovered::Known(BoundaryKind::Symmetry)
        }
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

fn mesh_and_boundary_facts(
    block_mesh: &Doc,
    velocity: &Doc,
    pressure: &Doc,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let doc = block_mesh;
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

    // The mesh and both field files must name the same patches.
    let mut seen = BTreeSet::new();
    for patch in &mesh_patches_list {
        if !seen.insert(patch.name.text.as_str()) {
            return Err(doc.err(
                patch.name.sp,
                format!("the patch \"{}\" appears twice", patch.name.text),
            ));
        }
        for (field, label) in [(&u, "0/U"), (&p, "0/p")] {
            if !field.contains_key(&patch.name.text) {
                return Err(doc.err(
                    patch.name.sp,
                    format!("the patch \"{}\" has no entry in {label}", patch.name.text),
                ));
            }
        }
    }
    for (field_doc, field) in [(velocity, &u), (pressure, &p)] {
        for (name, patch) in field {
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

    for patch in &mesh_patches_list {
        let (fu, fp) = (&u[&patch.name.text], &p[&patch.name.text]);
        let recovered = classify(
            &patch.mesh_type.text,
            &fu.ty.text,
            &fp.ty.text,
            pressure_is_uniform_zero(pressure, fp)?,
        );
        let text = match &recovered {
            Recovered::Known(kind) => kind_name(*kind).to_owned(),
            Recovered::Empty => EMPTY_KIND.to_owned(),
            Recovered::Other(text) => text.clone(),
        };
        for (side, tuple) in &patch.sides {
            facts.insert(
                format!("/boundary_conditions/by_face/{}/kind", side.name()),
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
            let (Recovered::Known(kind), Some(face), Some(value)) =
                (&recovered, side.face(), fu.value)
            else {
                continue;
            };
            if !matches!(kind, BoundaryKind::VelocityInlet | BoundaryKind::MovingWall) {
                continue;
            }
            let vector = uniform_vector(velocity, value)?;
            let raw = [vector[0].v, vector[1].v, vector[2].v];
            let key = format!("/boundary_conditions/by_face/{}/velocity", side.name());
            let entry = velocity.span(value.sp);
            match speed_from_vector(*kind, face, raw) {
                Some(speed) => {
                    // The component the kind allows on this face is the one that carries the speed.
                    let axis = (0..3)
                        .find(|&i| {
                            let mut unit = [0.0; 3];
                            unit[i] = 1.0;
                            speed_from_vector(*kind, face, unit).is_some()
                        })
                        .unwrap_or(0);
                    facts.insert(
                        key,
                        fact(
                            FactValue::Number(speed),
                            vec![velocity.span(vector[axis].sp)],
                            entry,
                        ),
                    );
                }
                None => {
                    let written =
                        format!("({} {} {})", vector[0].text, vector[1].text, vector[2].text);
                    facts.insert(
                        key,
                        fact(
                            FactValue::Text(written),
                            vector.iter().map(|n| velocity.span(n.sp)).collect(),
                            entry,
                        ),
                    );
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
    let numbers = doc.nums(grading, 3)?;
    let text = numbers
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    facts.insert(
        "/mesh/grading".into(),
        fact(
            FactValue::Text(text),
            numbers.iter().map(|n| doc.span(n.sp)).collect(),
            doc.span(blocks.sp),
        ),
    );
    Ok(())
}
