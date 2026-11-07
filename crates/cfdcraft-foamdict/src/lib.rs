//! A parser for OpenFOAM dictionary text.
//!
//! The text of a case from anywhere is untrusted, so parsing only builds a tree of data: it
//! never opens a file, never follows an include and never evaluates anything. A directive
//! (`#include`, `#includeEtc`, `#includeFunc`, `#calc`, `#codeStream`, ...) is recorded with its
//! arguments, and a `$macro` is kept as written. Text over the size cap, nested deeper than the
//! depth cap or holding more tokens than the token cap is refused with a named error before the
//! work is done.

mod parse;
mod query;
mod token;

use thiserror::Error;

/// The default cap on the size of the text, in bytes.
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024;
/// The default cap on how deeply blocks, lists and bodies nest.
pub const DEFAULT_MAX_DEPTH: usize = 64;
/// The default cap on the number of tokens (words, strings, brackets and semicolons).
pub const DEFAULT_MAX_TOKENS: usize = 4_000_000;

/// The caps that bound a parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The longest text that is read, in bytes; longer text is refused before it is tokenized.
    pub max_bytes: usize,
    /// The deepest nesting that is read (the parser itself never nests deeper than 256).
    pub max_depth: usize,
    /// The most tokens that are read.
    pub max_tokens: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }
}

/// Why a dictionary text could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    /// The text is larger than the size cap.
    #[error("the dictionary text is {size} bytes, more than the {max} that are read")]
    TooLarge { size: usize, max: usize },
    /// Blocks, lists or bodies nest deeper than the depth cap.
    #[error("the dictionary text nests deeper than {max} levels")]
    TooDeep { max: usize },
    /// The text holds more tokens than the token cap.
    #[error("the dictionary text holds more than {max} tokens")]
    TooManyTokens { max: usize },
    /// A brace, bracket, string, comment or entry opened on this 1-based line is never closed.
    #[error("something opened on line {line} is never closed")]
    Unterminated { line: u32 },
    /// A token is where it cannot be, on this 1-based line.
    #[error("unexpected {what} on line {line}")]
    Unexpected { line: u32, what: String },
}

/// The `FoamFile` header of a dictionary; a missing member is the empty text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub version: String,
    pub format: String,
    pub class: String,
    pub object: String,
}

/// A parsed dictionary text: the header (when there is a `FoamFile` block at the top level) and
/// the entries that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub header: Option<Header>,
    pub root: Dict,
}

/// An ordered list of entries. Duplicate keys are all kept, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dict {
    entries: Vec<Entry>,
}

/// One entry of a dictionary. `line` is the 1-based line of the entry's first token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// `key tokens...;` (the tokens may be empty).
    Value {
        key: String,
        tokens: Vec<Token>,
        line: u32,
    },
    /// `key { ... }`.
    Block { key: String, dict: Dict, line: u32 },
    /// A directive line; it is recorded and never followed or executed.
    Directive(Directive),
    /// `$name;` as a whole entry; the name is without the dollar sign and is never expanded.
    Macro { name: String, line: u32 },
}

/// One token of a value. A list `( ... )`, a bracket group `[ ... ]` and a brace body `{ ... }`
/// keep their contents and nesting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A word or number as written (`$name` and `#directive` words included).
    Word(String),
    /// A quoted string without its quotes, a backslash-quote and a double backslash undone.
    Str(String),
    /// The text between `#{` and `#}`, kept as data.
    Verbatim(String),
    List(Vec<Token>),
    /// `[ ... ]`, as in `dimensions [0 1 -1 0 0 0 0]`.
    Dims(Vec<Token>),
    Dict(Dict),
}

/// A `#...` directive with its arguments. Nothing is opened, included or run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The directive word with its `#`.
    pub word: String,
    /// The arguments as written: the same-line pieces of a directive entry, or the tokens that
    /// follow the word inside a value.
    pub args: Vec<String>,
    /// The argument text from its first to its last piece (entry directives are exact source
    /// text; directives inside a value are the pieces joined by a space).
    pub arg_text: String,
    /// The directive has a `{ ... }` body, which is skipped and never read as instructions.
    pub has_body: bool,
    /// The 1-based line of the directive (inside a value, the line of that entry).
    pub line: u32,
}

/// Parses dictionary text. The text is refused if it is over `limits.max_bytes` before any
/// tokenizing, and parsing stops with a named error at the depth and token caps.
pub fn parse(text: &str, limits: &Limits) -> Result<Document, ParseError> {
    parse::run(text, limits)
}
