//! Minimal reading and editing of OpenFOAM-style dictionary text.
//!
//! An entry is addressed by its block names and then its own name, joined by `/`
//! (for example `relaxationFactors/equations/U`). Edits splice into the original text by
//! byte range, so everything outside the changed entry stays byte for byte as it was.
//! Nothing is guessed: a block that is not there, a target that is a whole block, a value
//! that could close or extend the entry, or a text that does not hold together is an error.

use thiserror::Error;

/// The longest dictionary text that is read, in bytes.
pub const MAX_TEXT: usize = 1024 * 1024;
/// The deepest nesting of braces that is read.
pub const MAX_DEPTH: usize = 32;
/// The longest value that is written, in characters.
pub const MAX_VALUE: usize = 200;
/// The most names an entry path may hold.
pub const MAX_SEGMENTS: usize = 6;
/// The longest single name in an entry path, in characters.
pub const MAX_SEGMENT: usize = 64;

/// Why a dictionary text could not be read or edited. The input is never changed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EditError {
    /// The entry path is empty, has more than six names, or a name is not a letter followed by letters, digits and underscores.
    #[error(
        "the entry path {entry:?} is not one to six names joined by '/', each starting with a letter and holding only letters, digits and underscores"
    )]
    BadEntry { entry: String },
    /// The value is blank, too long, or holds a character that could end the entry, open a block, start a comment or pull in other text.
    #[error("the value for {entry} is refused: {reason}")]
    BadValue { entry: String, reason: &'static str },
    /// The text is larger than the limit.
    #[error("the dictionary text is {size} bytes, more than the {max} that are read")]
    TooLarge { size: usize, max: usize },
    /// The braces are nested deeper than the limit.
    #[error("the dictionary text nests blocks deeper than {max} levels")]
    TooDeep { max: usize },
    /// A brace has no partner; the line is the 1-based line of the brace.
    #[error("the dictionary text has a brace without a partner on line {line}")]
    Unbalanced { line: usize },
    /// A string, a comment or an entry is not closed; the line is the 1-based line where it starts.
    #[error("the dictionary text has something that is never closed, starting on line {line}")]
    Unterminated { line: usize },
    /// A block named in the path is not in the text (or is a plain entry); a block is never created.
    #[error("the block on the path {entry} is not in the dictionary text")]
    NoSuchBlock { entry: String },
    /// The target is a whole block, which is not replaced as one value.
    #[error("{entry} is a whole block and not a single entry")]
    NotAnEntry { entry: String },
    /// The block that is searched holds the same name twice, so there is no single entry to use.
    #[error("{entry} appears more than once in the same block")]
    Ambiguous { entry: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Quoted,
    Open,
    Close,
    Semi,
}

#[derive(Clone, Copy)]
struct Tok {
    kind: Kind,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
enum Shape {
    /// A keyword, its value tokens and the closing `;` (a token index).
    Plain { semi: usize },
    /// A keyword followed directly by a braced body (token indexes of the two braces).
    Block { open: usize, close: usize },
}

#[derive(Clone, Copy)]
struct Entry {
    kw: usize,
    shape: Shape,
}

struct Located {
    found: Option<Entry>,
    /// Token indexes of the braces around the searched block; `None` at the top level.
    parent: Option<(usize, usize)>,
}

const NONE: usize = usize::MAX;

struct Doc<'a> {
    text: &'a str,
    toks: Vec<Tok>,
    /// For an opening brace, the index of its closing brace; `NONE` for any other token.
    close_of: Vec<usize>,
}

fn line_of(text: &str, off: usize) -> usize {
    1 + text.as_bytes()[..off]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

fn starts_comment(b: &[u8], i: usize) -> bool {
    b[i] == b'/' && matches!(b.get(i + 1), Some(b'/' | b'*'))
}

fn ends_word(b: &[u8], i: usize) -> bool {
    b[i].is_ascii_whitespace() || matches!(b[i], b'{' | b'}' | b';' | b'"') || starts_comment(b, i)
}

impl<'a> Doc<'a> {
    fn scan(text: &'a str) -> Result<Self, EditError> {
        if text.len() > MAX_TEXT {
            return Err(EditError::TooLarge {
                size: text.len(),
                max: MAX_TEXT,
            });
        }
        let b = text.as_bytes();
        let mut toks: Vec<Tok> = Vec::new();
        let mut close_of: Vec<usize> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            if starts_comment(b, i) {
                if b[i + 1] == b'/' {
                    i = text[i..].find('\n').map_or(b.len(), |n| i + n);
                } else {
                    match text[i + 2..].find("*/") {
                        Some(n) => i += n + 4,
                        None => {
                            return Err(EditError::Unterminated {
                                line: line_of(text, i),
                            });
                        }
                    }
                }
                continue;
            }
            let start = i;
            let kind = match c {
                b'{' => {
                    open.push(toks.len());
                    if open.len() > MAX_DEPTH {
                        return Err(EditError::TooDeep { max: MAX_DEPTH });
                    }
                    i += 1;
                    Kind::Open
                }
                b'}' => {
                    let Some(o) = open.pop() else {
                        return Err(EditError::Unbalanced {
                            line: line_of(text, i),
                        });
                    };
                    close_of[o] = toks.len();
                    i += 1;
                    Kind::Close
                }
                b';' => {
                    i += 1;
                    Kind::Semi
                }
                b'"' => {
                    i += 1;
                    loop {
                        match b.get(i) {
                            None => {
                                return Err(EditError::Unterminated {
                                    line: line_of(text, start),
                                });
                            }
                            Some(b'\\') => i += 2,
                            Some(b'"') => {
                                i += 1;
                                break;
                            }
                            Some(_) => i += 1,
                        }
                    }
                    Kind::Quoted
                }
                _ => {
                    while i < b.len() && !ends_word(b, i) {
                        i += 1;
                    }
                    Kind::Word
                }
            };
            toks.push(Tok {
                kind,
                start,
                end: i,
            });
            close_of.push(NONE);
        }
        if let Some(&o) = open.last() {
            return Err(EditError::Unbalanced {
                line: line_of(text, toks[o].start),
            });
        }
        Ok(Doc {
            text,
            toks,
            close_of,
        })
    }

    fn word(&self, tok: usize) -> &'a str {
        &self.text[self.toks[tok].start..self.toks[tok].end]
    }

    /// The entries directly inside the token range `from..to`, one linear pass.
    fn entries(&self, from: usize, to: usize) -> Result<Vec<Entry>, EditError> {
        let mut out = Vec::new();
        let mut i = from;
        while i < to {
            match self.toks[i].kind {
                Kind::Semi | Kind::Close => {
                    i += 1;
                    continue;
                }
                Kind::Open => {
                    i = self.close_of[i] + 1;
                    continue;
                }
                Kind::Word | Kind::Quoted => {}
            }
            let mut j = i + 1;
            if j < to && self.toks[j].kind == Kind::Open {
                let close = self.close_of[j];
                out.push(Entry {
                    kw: i,
                    shape: Shape::Block { open: j, close },
                });
                i = close + 1;
                continue;
            }
            // A plain entry runs to its `;`; a braced part after other tokens belongs to the value.
            loop {
                if j >= to {
                    return Err(EditError::Unterminated {
                        line: line_of(self.text, self.toks[i].start),
                    });
                }
                match self.toks[j].kind {
                    Kind::Semi => break,
                    Kind::Open => j = self.close_of[j] + 1,
                    _ => j += 1,
                }
            }
            out.push(Entry {
                kw: i,
                shape: Shape::Plain { semi: j },
            });
            i = j + 1;
        }
        Ok(out)
    }

    fn find(
        &self,
        range: (usize, usize),
        name: &str,
        entry: &str,
    ) -> Result<Option<Entry>, EditError> {
        let mut hit = None;
        for e in self.entries(range.0, range.1)? {
            if self.word(e.kw) == name {
                if hit.is_some() {
                    return Err(EditError::Ambiguous {
                        entry: entry.to_owned(),
                    });
                }
                hit = Some(e);
            }
        }
        Ok(hit)
    }

    fn locate(&self, segs: &[&str], entry: &str) -> Result<Located, EditError> {
        let mut range = (0, self.toks.len());
        let mut parent = None;
        let Some((last, path)) = segs.split_last() else {
            return Err(EditError::BadEntry {
                entry: entry.to_owned(),
            });
        };
        for seg in path {
            match self.find(range, seg, entry)? {
                Some(Entry {
                    shape: Shape::Block { open, close },
                    ..
                }) => {
                    range = (open + 1, close);
                    parent = Some((open, close));
                }
                _ => {
                    return Err(EditError::NoSuchBlock {
                        entry: entry.to_owned(),
                    });
                }
            }
        }
        Ok(Located {
            found: self.find(range, last, entry)?,
            parent,
        })
    }

    /// The text with `new` (`name value;`) added inside the parent block, in that block's layout.
    fn insert(&self, parent: Option<(usize, usize)>, new: &str) -> String {
        let text = self.text;
        let Some((open, close)) = parent else {
            let lead = if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            return [text, lead, new, "\n"].concat();
        };
        let (o, c) = (self.toks[open], self.toks[close]);
        let line_start = text[..c.start].rfind('\n').map_or(0, |n| n + 1);
        let close_indent = &text[line_start..c.start];
        let own_line = text[o.end..c.start].contains('\n') && is_blank(close_indent);
        if !own_line {
            // Opened and closed on one line (or the brace shares its line with an entry).
            let at = self.toks[close - 1].end;
            return [&text[..at], " ", new, &text[at..]].concat();
        }
        let indent = self.first_entry_indent(open, close, o.end);
        let indent = indent.map_or_else(|| format!("{close_indent}    "), str::to_owned);
        [&text[..line_start], &indent, new, "\n", &text[line_start..]].concat()
    }

    /// The indent of the block's first entry, when that entry starts its own line.
    fn first_entry_indent(&self, open: usize, close: usize, after_open: usize) -> Option<&'a str> {
        if open + 1 >= close {
            return None;
        }
        let start = self.toks[open + 1].start;
        let line_start = self.text[..start].rfind('\n').map_or(0, |n| n + 1);
        let prefix = &self.text[line_start..start];
        (line_start >= after_open && is_blank(prefix)).then_some(prefix)
    }
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| b == b' ' || b == b'\t')
}

fn split_entry(entry: &str) -> Result<Vec<&str>, EditError> {
    let segs: Vec<&str> = entry.split('/').collect();
    let plain = |s: &str| {
        let mut chars = s.chars();
        s.len() <= MAX_SEGMENT
            && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if segs.len() > MAX_SEGMENTS || !segs.iter().all(|s| plain(s)) {
        return Err(EditError::BadEntry {
            entry: entry.to_owned(),
        });
    }
    Ok(segs)
}

fn check_value(entry: &str, value: &str) -> Result<(), EditError> {
    let refuse = |reason| {
        Err(EditError::BadValue {
            entry: entry.to_owned(),
            reason,
        })
    };
    if value.trim().is_empty() {
        return refuse("it is empty");
    }
    if value.chars().count() > MAX_VALUE {
        return refuse("it is longer than 200 characters");
    }
    if value.chars().any(char::is_control) {
        return refuse("it holds a control character or a line break");
    }
    if value.contains(['"', '\\']) {
        return refuse("it holds a quote or a backslash");
    }
    if value.contains([';', '{', '}']) {
        return refuse("it holds a semicolon or a brace");
    }
    if value.contains(['#', '$']) {
        return refuse("it holds a directive or a variable reference");
    }
    if value.contains("//") || value.contains("/*") {
        return refuse("it would start a comment");
    }
    Ok(())
}

/// Replaces the value of the plain entry at `entry`, or adds the entry at the end of its block when absent.
///
/// Refused, with the text left as it was: a bad path or value, a block on the path that is
/// not there (a block is never created), a target that is a whole block, a repeated name in
/// a searched block, and a text that is over `MAX_TEXT`, nested deeper than `MAX_DEPTH` or
/// not balanced.
pub fn set_entry(text: &str, entry: &str, value: &str) -> Result<String, EditError> {
    let segs = split_entry(entry)?;
    check_value(entry, value)?;
    let doc = Doc::scan(text)?;
    let at = doc.locate(&segs, entry)?;
    let value = value.trim();
    match at.found {
        Some(Entry {
            shape: Shape::Block { .. },
            ..
        }) => Err(EditError::NotAnEntry {
            entry: entry.to_owned(),
        }),
        Some(Entry {
            kw,
            shape: Shape::Plain { semi },
        }) => Ok([
            &text[..doc.toks[kw].end],
            " ",
            value,
            ";",
            &text[doc.toks[semi].end..],
        ]
        .concat()),
        None => {
            let name = segs.last().copied().unwrap_or_default();
            Ok(doc.insert(at.parent, &format!("{name} {value};")))
        }
    }
}

/// Reads the value text of the plain entry at `entry`; `None` when the entry is absent from a block that exists.
///
/// Refused like `set_entry`: a bad path, a missing block, a whole block as target, a repeated name or a bad text.
pub fn get_entry(text: &str, entry: &str) -> Result<Option<String>, EditError> {
    let segs = split_entry(entry)?;
    let doc = Doc::scan(text)?;
    match doc.locate(&segs, entry)?.found {
        None => Ok(None),
        Some(Entry {
            shape: Shape::Block { .. },
            ..
        }) => Err(EditError::NotAnEntry {
            entry: entry.to_owned(),
        }),
        Some(Entry {
            kw,
            shape: Shape::Plain { semi },
        }) => Ok(Some(if semi == kw + 1 {
            String::new()
        } else {
            text[doc.toks[kw + 1].start..doc.toks[semi - 1].end].to_owned()
        })),
    }
}
