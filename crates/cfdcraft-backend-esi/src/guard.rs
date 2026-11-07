//! Refuses every construct that can make an OpenFOAM process run code or load files from outside
//! the case. The pinned image enables system operations, so this scan is the only defence.
//!
//! The scan is a token scan, not a dictionary parse, and it fails closed: when a file cannot be
//! tokenised with confidence it is refused. It is independent of the round-trip reader on purpose.

use crate::case::CaseTree;
use std::{
    fmt, io,
    path::{Component, Path},
};

/// Longest entry text kept in a refusal.
const ENTRY_MAX: usize = 80;
/// Largest dictionary file the scan accepts.
const FILE_MAX: usize = 16 * 1024 * 1024;
/// Deepest directory nesting `scan_dir` walks.
const DEPTH_MAX: usize = 16;
/// Most refusals returned for one file; a final refusal says the list was cut.
pub const REFUSALS_MAX: usize = 1000;
/// Library names a case may load, bare. Extending this list is a reviewed change to this constant.
/// `sampling` holds the probes and sets; `fieldFunctionObjects` holds the flux and average
/// function objects that watch the patches; `forces` holds the force and force coefficient
/// function objects (the library file is `libforces`, source `src/functionObjects/forces`).
const ALLOWED_LIBS: [&str; 6] = [
    "sampling",
    "libsampling.so",
    "fieldFunctionObjects",
    "libfieldFunctionObjects.so",
    "forces",
    "libforces.so",
];

/// Which kind of construct a refusal is about. `code()` is stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A `#` directive other than a literal relative include, or a verbatim block.
    Directive,
    /// A coded boundary-condition, function or source type, or a code keyword.
    Coded,
    /// A system-call function object or one of its command keywords.
    SystemCall,
    /// A library that is not on the allowlist.
    Library,
    /// A switch block or the system-operations switch.
    SwitchBlock,
    /// An include that is not a literal relative path inside the case.
    Include,
    /// A file or path the case may not contain.
    Tree,
    /// A string, comment or verbatim block that never ends.
    Unterminated,
    /// Variable expansion other than a plain whole-token `$name`.
    Expansion,
}

impl Rule {
    pub fn code(self) -> &'static str {
        match self {
            Rule::Directive => "CFD-GUARD-001",
            Rule::Coded => "CFD-GUARD-002",
            Rule::SystemCall => "CFD-GUARD-003",
            Rule::Library => "CFD-GUARD-004",
            Rule::SwitchBlock => "CFD-GUARD-005",
            Rule::Include => "CFD-GUARD-006",
            Rule::Tree => "CFD-GUARD-007",
            Rule::Unterminated => "CFD-GUARD-008",
            Rule::Expansion => "CFD-GUARD-009",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Rule::Directive => "directives other than a relative include can run code",
            Rule::Coded => "coded types and code keywords compile and run code",
            Rule::SystemCall => "system calls run commands",
            Rule::Library => "only the listed libraries may be loaded",
            Rule::SwitchBlock => "switch blocks can enable code execution",
            Rule::Include => "an include must be a literal relative path inside the case",
            Rule::Tree => "this file or path is not allowed in a case",
            Rule::Unterminated => "unterminated construct, the file cannot be checked",
            Rule::Expansion => {
                "only a plain $name may be used; other expansion can build forbidden words"
            }
        }
    }
}

/// One file to scan. `path` is relative to the case root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// One refused construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub file: String,
    /// 1-based.
    pub line: usize,
    /// The offending token, at most 80 characters.
    pub entry: String,
    pub rule: Rule,
    pub message: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} `{}`: {}",
            self.file,
            self.line,
            self.rule.code(),
            self.entry,
            self.message
        )
    }
}

fn refuse(out: &mut Vec<Refusal>, file: &str, line: usize, entry: &str, rule: Rule) {
    out.push(Refusal {
        file: file.to_string(),
        line,
        entry: entry.chars().take(ENTRY_MAX).collect(),
        rule,
        message: rule.message().to_string(),
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind<'a> {
    Word(&'a str),
    /// Content between the quotes, escapes kept as written.
    Str(&'a str),
    Punct(u8),
    Verbatim,
}

#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    line: usize,
}

fn is_word_end(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(
            b,
            b';' | b'{' | b'}' | b'(' | b')' | b'[' | b']' | b'"' | b','
        )
}

/// Splits `src` into tokens. Comments are skipped, strings are opaque, `#{ ... #}` is one token.
/// An unterminated construct is reported and ends the scan of the file.
fn tokenize<'a>(file: &str, src: &'a str, out: &mut Vec<Refusal>) -> Vec<Token<'a>> {
    let b = src.as_bytes();
    let mut tokens = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let start = line;
            i += 2;
            loop {
                if i >= b.len() {
                    refuse(out, file, start, "/*", Rule::Unterminated);
                    return tokens;
                }
                if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    i += 2;
                    break;
                }
                if b[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
        } else if c == b'"' {
            let (start, from) = (line, i + 1);
            i += 1;
            loop {
                match b.get(i) {
                    None => {
                        refuse(out, file, start, "\"", Rule::Unterminated);
                        return tokens;
                    }
                    Some(b'\\') => {
                        if b.get(i + 1) == Some(&b'\n') {
                            line += 1;
                        }
                        i += 2;
                    }
                    Some(b'"') => break,
                    Some(b'\n') => {
                        line += 1;
                        i += 1;
                    }
                    Some(_) => i += 1,
                }
            }
            tokens.push(Token {
                kind: Kind::Str(&src[from..i]),
                line: start,
            });
            i += 1;
        } else if c == b'#' && b.get(i + 1) == Some(&b'{') {
            let start = line;
            i += 2;
            loop {
                if i >= b.len() {
                    refuse(out, file, start, "#{", Rule::Unterminated);
                    return tokens;
                }
                if b[i] == b'#' && b.get(i + 1) == Some(&b'}') {
                    i += 2;
                    break;
                }
                if b[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            tokens.push(Token {
                kind: Kind::Verbatim,
                line: start,
            });
        } else if matches!(c, b';' | b'{' | b'}' | b'(' | b')' | b'[' | b']' | b',') {
            tokens.push(Token {
                kind: Kind::Punct(c),
                line,
            });
            i += 1;
        } else {
            let from = i;
            while i < b.len() && !is_word_end(b[i]) {
                i += 1;
            }
            // `{` is a word end, so adjacency to a `$` is only visible here, in the bytes.
            let mut to = i;
            if to > from && b[to - 1] == b'$' && b.get(i) == Some(&b'{') {
                refuse(out, file, line, "${", Rule::Expansion);
                to -= 1;
            }
            if to > from {
                tokens.push(Token {
                    kind: Kind::Word(&src[from..to]),
                    line,
                });
            }
        }
    }
    tokens
}

/// How OpenFOAM reads a quoted string where a word is expected: characters that cannot be part of
/// a word are dropped. A quoted type name is therefore the same type name.
fn as_word(s: &str) -> String {
    s.chars()
        .filter(|c| !(c.is_whitespace() || matches!(c, '"' | '\'' | '/' | ';' | '{' | '}')))
        .collect()
}

/// Keyword rules, applied to the lowercase form of every word and quoted string, key or value.
fn keyword_rule(word: &str) -> Option<Rule> {
    let w = word.to_ascii_lowercase();
    if w.starts_with("code") || w.contains("coded") || w == "redirecttype" {
        Some(Rule::Coded)
    } else if matches!(
        w.as_str(),
        "systemcall" | "executecalls" | "writecalls" | "endcalls"
    ) {
        Some(Rule::SystemCall)
    } else if matches!(
        w.as_str(),
        "allowsystemoperations" | "infoswitches" | "debugswitches" | "optimisationswitches"
    ) {
        Some(Rule::SwitchBlock)
    } else {
        None
    }
}

fn scan_content(file: &str, bytes: &[u8], out: &mut Vec<Refusal>) {
    if bytes.len() > FILE_MAX {
        refuse(out, file, 1, "file over 16 MiB", Rule::Tree);
        return;
    }
    let line_of = |upto: usize| bytes[..upto].iter().filter(|&&c| c == b'\n').count() + 1;
    if let Some(at) = bytes.iter().position(|&c| c == 0) {
        refuse(out, file, line_of(at), "NUL byte", Rule::Tree);
        return;
    }
    let src = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            refuse(
                out,
                file,
                line_of(e.valid_up_to()),
                "invalid UTF-8",
                Rule::Tree,
            );
            return;
        }
    };
    let tokens = tokenize(file, src, out);
    let (start, mut libs_done, mut cut) = (out.len(), 0, None);
    for i in 0..tokens.len() {
        if out.len() - start >= REFUSALS_MAX {
            cut = Some(tokens[i].line);
            break;
        }
        check_token(file, &tokens, i, &mut libs_done, out);
    }
    if let Some(line) = cut {
        out.truncate(start + REFUSALS_MAX);
        refuse(out, file, line, "refusal list cut", Rule::Tree);
    }
}

/// Per-token rules. Further rules plug in here.
/// `libs_done` is the first token not yet covered by a `libs` scan, so no span is scanned twice.
fn check_token(
    file: &str,
    tokens: &[Token],
    i: usize,
    libs_done: &mut usize,
    out: &mut Vec<Refusal>,
) {
    let t = tokens[i];
    match t.kind {
        Kind::Verbatim => refuse(out, file, t.line, "#{", Rule::Directive),
        Kind::Word("#include") => check_include(file, tokens, i, out),
        // OpenFOAM starts a directive at a `#` that follows a number or a one-character
        // punctuation token without a space (`1#calc`, `-#calc`), so any other `#` in a word is
        // refused, not just a leading one.
        Kind::Word(w) if w.contains('#') => refuse(out, file, t.line, w, Rule::Directive),
        Kind::Word(w) => {
            if !dollar_ok(w) {
                refuse(out, file, t.line, w, Rule::Expansion);
            }
            if let Some(rule) = keyword_rule(w) {
                refuse(out, file, t.line, w, rule);
            }
            if matches!(
                w.to_ascii_lowercase().as_str(),
                "libs" | "functionobjectlibs"
            ) && i >= *libs_done
            {
                *libs_done = check_libs(file, tokens, i, out);
            }
        }
        Kind::Str(s) => {
            if !dollar_ok(s) {
                refuse(out, file, t.line, s, Rule::Expansion);
            }
            if let Some(rule) = keyword_rule(&as_word(s)) {
                refuse(out, file, t.line, s, rule);
            }
        }
        Kind::Punct(_) => {}
    }
}

/// A `$` is allowed only as the first character of a whole-token plain name (`$internalField`,
/// `$:scope.name`). Anything else (`co$x`, `$a$b`, a lone `$`, `"a $x"`) can expand to a word the
/// other rules would have refused, so it is refused here.
fn dollar_ok(s: &str) -> bool {
    match s.strip_prefix('$') {
        _ if !s.contains('$') => true,
        Some(name) => {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
        }
        None => false,
    }
}

fn has_drive(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// An include is allowed only as a literal relative path: no variable, home, absolute, drive,
/// backslash or parent component.
fn include_ok(p: &str) -> bool {
    !p.is_empty()
        && !p.contains(['$', '\\'])
        && !p.starts_with(['~', '/'])
        && !has_drive(p)
        && !p.split('/').any(|c| c == "..")
}

fn check_include(file: &str, tokens: &[Token], i: usize, out: &mut Vec<Refusal>) {
    let line = tokens[i].line;
    match tokens.get(i + 1).map(|t| t.kind) {
        Some(Kind::Word(p) | Kind::Str(p)) if include_ok(p) => {}
        Some(Kind::Word(p) | Kind::Str(p)) => refuse(out, file, line, p, Rule::Include),
        _ => refuse(out, file, line, "#include", Rule::Include),
    }
}

/// The value of `libs` runs up to the next `;`. Every item must be on the allowlist. Returns the
/// index of the token the scan stopped at. A value that reaches the end of the file still open
/// (no `;` at depth 0) is refused once, at the keyword, whatever its items were.
fn check_libs(file: &str, tokens: &[Token], i: usize, out: &mut Vec<Refusal>) -> usize {
    let (mut depth, start) = (0usize, out.len());
    for (k, t) in tokens.iter().enumerate().skip(i + 1) {
        match t.kind {
            Kind::Punct(b';') if depth == 0 => return k,
            Kind::Punct(b'(' | b'[') => depth += 1,
            Kind::Punct(b')' | b']') => depth = depth.saturating_sub(1),
            Kind::Punct(b'{' | b'}') => {
                let text = if t.kind == Kind::Punct(b'{') {
                    "{"
                } else {
                    "}"
                };
                refuse(out, file, t.line, text, Rule::Library);
                return k;
            }
            Kind::Word(w) | Kind::Str(w) if !ALLOWED_LIBS.contains(&w) => {
                refuse(out, file, t.line, w, Rule::Library)
            }
            Kind::Verbatim => return k,
            _ => {}
        }
    }
    out.truncate(start);
    let keyword = match tokens[i].kind {
        Kind::Word(w) => w,
        _ => "libs",
    };
    refuse(out, file, tokens[i].line, keyword, Rule::Library);
    tokens.len()
}

/// Path rules: no parent, absolute, drive or backslash paths, no compressed or shared-object
/// files, no generated-code directories.
fn path_rules(path: &str, out: &mut Vec<Refusal>) {
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    if path.contains('\\') || path.starts_with('/') || has_drive(path) || parts.contains(&"..") {
        refuse(out, path, 1, path, Rule::Tree);
    }
    let last = parts.last().copied().unwrap_or("").to_ascii_lowercase();
    if [".gz", ".so", ".dylib", ".dll"]
        .iter()
        .any(|e| last.ends_with(e))
    {
        refuse(out, path, 1, &last, Rule::Tree);
    }
    if let Some(c) = parts.iter().find(|c| {
        matches!(
            c.to_ascii_lowercase().as_str(),
            "dynamiccode" | "codetemplates"
        )
    }) {
        refuse(out, path, 1, c, Rule::Tree);
    }
}

/// Scans the given files. Refusals come back sorted by file, then line.
pub fn scan_files(files: &[GuardFile]) -> Vec<Refusal> {
    let mut out = Vec::new();
    for f in files {
        path_rules(&f.path, &mut out);
        scan_content(&f.path, &f.bytes, &mut out);
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    out
}

/// Scans a compiled case tree.
pub fn scan_tree(tree: &CaseTree) -> Vec<Refusal> {
    let files: Vec<GuardFile> = tree
        .files
        .iter()
        .map(|f| GuardFile {
            path: path_text(&f.rel_path),
            bytes: f.bytes.clone(),
        })
        .collect();
    scan_files(&files)
}

/// The path as `/`-separated text. Parent, root and prefix components stay visible so the path
/// rules can refuse them.
fn path_text(p: &std::path::Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        parts.push(match c {
            Component::Normal(s) => s.to_string_lossy().into_owned(),
            Component::ParentDir => "..".into(),
            Component::CurDir => ".".into(),
            Component::RootDir => String::new(),
            Component::Prefix(s) => s.as_os_str().to_string_lossy().into_owned(),
        });
    }
    parts.join("/")
}

/// Result of scanning a case directory on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DirScan {
    pub refusals: Vec<Refusal>,
    /// Relative paths of the files that were scanned.
    pub scanned: Vec<String>,
    /// Top-level entries that hold no case dictionaries and were not scanned.
    pub skipped: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Top,
    /// Inside a dictionary tree: every file is scanned.
    Dict,
    /// Inside any other directory: only links and special files are looked for.
    LinksOnly,
}

fn is_case_dir(name: &str) -> bool {
    matches!(name, "constant" | "system" | "0.orig")
        || (!name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '-' | '+'))
            && name.parse::<f64>().is_ok())
}

/// Scans a case directory without following links. Links and special files are refused anywhere;
/// the dictionary rules run on `constant`, `system`, `0`, `0.orig` and time directories only, and
/// other top-level entries are listed as skipped. Paths are relative to `root`, `/` separated.
pub fn scan_dir(root: &Path) -> io::Result<DirScan> {
    let mut s = DirScan::default();
    walk(root, "", 0, Mode::Top, &mut s)?;
    s.refusals
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    s.scanned.sort();
    s.skipped.sort();
    Ok(s)
}

fn walk(dir: &Path, rel: &str, depth: usize, mode: Mode, s: &mut DirScan) -> io::Result<()> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir)? {
        names.push(e?.file_name());
    }
    names.sort();
    for name in names {
        let Some(n) = name.to_str() else {
            let lossy = name.to_string_lossy();
            let at = if rel.is_empty() {
                lossy.to_string()
            } else {
                format!("{rel}/{lossy}")
            };
            refuse(&mut s.refusals, &at, 1, &lossy, Rule::Tree);
            continue;
        };
        let child = if rel.is_empty() {
            n.to_string()
        } else {
            format!("{rel}/{n}")
        };
        let path = dir.join(&name);
        let md = std::fs::symlink_metadata(&path)?;
        let before = s.refusals.len();
        path_rules(&child, &mut s.refusals);
        let kind = md.file_type();
        if kind.is_symlink() || !(kind.is_file() || kind.is_dir()) {
            refuse(&mut s.refusals, &child, 1, n, Rule::Tree);
        } else if kind.is_dir() {
            if s.refusals.len() > before {
                continue;
            }
            if depth + 1 > DEPTH_MAX {
                refuse(
                    &mut s.refusals,
                    &child,
                    1,
                    "directory nested too deep",
                    Rule::Tree,
                );
                continue;
            }
            let next = match mode {
                Mode::Top if is_case_dir(n) => Mode::Dict,
                Mode::Top => {
                    s.skipped.push(child.clone());
                    Mode::LinksOnly
                }
                m => m,
            };
            walk(&path, &child, depth + 1, next, s)?;
        } else {
            match mode {
                Mode::Top => s.skipped.push(child),
                Mode::LinksOnly => {}
                Mode::Dict if md.len() > FILE_MAX as u64 => {
                    refuse(&mut s.refusals, &child, 1, "file over 16 MiB", Rule::Tree)
                }
                Mode::Dict => {
                    scan_content(&child, &std::fs::read(&path)?, &mut s.refusals);
                    s.scanned.push(child);
                }
            }
        }
    }
    Ok(())
}
