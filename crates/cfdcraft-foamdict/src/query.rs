//! Reading a parsed dictionary by key. Nothing here changes the tree.

use crate::{Dict, Directive, Document, Entry, Token};

impl Entry {
    /// The key of a value or block entry; a directive or a macro has none.
    pub fn key(&self) -> Option<&str> {
        match self {
            Entry::Value { key, .. } | Entry::Block { key, .. } => Some(key.as_str()),
            Entry::Directive(_) | Entry::Macro { .. } => None,
        }
    }

    /// The 1-based line where the entry starts.
    pub fn line(&self) -> u32 {
        match self {
            Entry::Value { line, .. } | Entry::Block { line, .. } | Entry::Macro { line, .. } => {
                *line
            }
            Entry::Directive(directive) => directive.line,
        }
    }
}

impl Token {
    /// The text of a word or a quoted string.
    pub fn as_word(&self) -> Option<&str> {
        match self {
            Token::Word(text) | Token::Str(text) => Some(text.as_str()),
            _ => None,
        }
    }

    /// The token written out on one line (a block as `{...}`, verbatim text as `#{...#}`).
    pub fn text(&self) -> String {
        match self {
            Token::Word(word) => word.clone(),
            Token::Str(text) => format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")),
            Token::Verbatim(_) => "#{...#}".to_string(),
            Token::List(tokens) => format!("({})", join(tokens)),
            Token::Dims(tokens) => format!("[{}]", join(tokens)),
            Token::Dict(_) => "{...}".to_string(),
        }
    }
}

fn join(tokens: &[Token]) -> String {
    tokens.iter().map(Token::text).collect::<Vec<_>>().join(" ")
}

impl Dict {
    /// Every entry, in file order, duplicates included.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The last entry with this key.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| e.key() == Some(key))
    }

    /// Every entry with this key, in file order.
    pub fn all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Entry> {
        self.entries.iter().filter(move |e| e.key() == Some(key))
    }

    /// The single word or string that is the value of the last entry with this key.
    pub fn get_word(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Entry::Value { tokens, .. }) => match tokens.as_slice() {
                [only] => only.as_word(),
                _ => None,
            },
            _ => None,
        }
    }

    /// The finite number that is the value of the last entry with this key.
    pub fn get_number(&self, key: &str) -> Option<f64> {
        self.get_word(key)?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }

    /// The body of the last block with this key.
    pub fn dict(&self, key: &str) -> Option<&Dict> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Block { key: k, dict, .. } if k.as_str() == key => Some(dict),
            _ => None,
        })
    }

    /// Every directive in the dictionary, in file order, nested blocks and values included.
    /// Each is a record only: nothing is followed or run.
    pub fn directives(&self) -> Vec<Directive> {
        let mut out = Vec::new();
        collect_dict(self, &mut out);
        out
    }
}

impl Document {
    /// The last top-level entry with this key.
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.root.get(key)
    }

    /// Every top-level entry with this key, in file order.
    pub fn all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Entry> {
        self.root.all(key)
    }

    pub fn get_word(&self, key: &str) -> Option<&str> {
        self.root.get_word(key)
    }

    pub fn get_number(&self, key: &str) -> Option<f64> {
        self.root.get_number(key)
    }

    pub fn dict(&self, key: &str) -> Option<&Dict> {
        self.root.dict(key)
    }

    pub fn directives(&self) -> Vec<Directive> {
        self.root.directives()
    }
}

fn collect_dict(dict: &Dict, out: &mut Vec<Directive>) {
    for entry in &dict.entries {
        match entry {
            Entry::Directive(directive) => out.push(directive.clone()),
            Entry::Block { dict, .. } => collect_dict(dict, out),
            Entry::Value { tokens, line, .. } => collect_tokens(tokens, *line, out),
            Entry::Macro { .. } => {}
        }
    }
}

fn finish(mut directive: Directive) -> Directive {
    directive.arg_text = directive.args.join(" ");
    directive
}

/// Directive words inside a value: the tokens after the word, up to the next directive word,
/// are its arguments, and a block among them is its body.
fn collect_tokens(tokens: &[Token], line: u32, out: &mut Vec<Directive>) {
    let mut current: Option<Directive> = None;
    for token in tokens {
        match token {
            Token::Word(word) if word.starts_with('#') => {
                out.extend(current.take().map(finish));
                current = Some(Directive {
                    word: word.clone(),
                    args: Vec::new(),
                    arg_text: String::new(),
                    has_body: false,
                    line,
                });
            }
            Token::Dict(dict) => {
                if let Some(directive) = current.as_mut() {
                    directive.has_body = true;
                }
                collect_dict(dict, out);
            }
            Token::List(inner) | Token::Dims(inner) => {
                if let Some(directive) = current.as_mut() {
                    directive.args.push(token.text());
                }
                collect_tokens(inner, line, out);
            }
            other => {
                if let Some(directive) = current.as_mut() {
                    directive.args.push(other.text());
                }
            }
        }
    }
    out.extend(current.map(finish));
}
