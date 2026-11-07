//! The parser: a recursive descent over the tokens, with a depth cap on every nesting.

use crate::token::{Kind, Lex, Lexer};
use crate::{Dict, Directive, Document, Entry, Header, Limits, ParseError, Token};

/// The deepest nesting the parser accepts whatever the caller asks for, so the recursion stays
/// within a thread's stack.
const HARD_MAX_DEPTH: usize = 256;

pub(crate) fn run(text: &str, limits: &Limits) -> Result<Document, ParseError> {
    if text.len() > limits.max_bytes {
        return Err(ParseError::TooLarge {
            size: text.len(),
            max: limits.max_bytes,
        });
    }
    let mut parser = Parser {
        text,
        lexer: Lexer::new(text, limits.max_tokens),
        peeked: None,
        max_depth: limits.max_depth.min(HARD_MAX_DEPTH),
    };
    let mut root = parser.dict(0, None)?;
    let header = take_header(&mut root);
    Ok(Document { header, root })
}

fn unexpected(line: u32, what: impl Into<String>) -> ParseError {
    ParseError::Unexpected {
        line,
        what: what.into(),
    }
}

/// The text of a quoted string without its quotes, with `\"` and `\\` undone.
fn unquote(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(next @ ('"' | '\\')) => out.push(next),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn take_header(root: &mut Dict) -> Option<Header> {
    let at = root
        .entries
        .iter()
        .position(|e| matches!(e, Entry::Block { key, .. } if key.as_str() == "FoamFile"))?;
    let Entry::Block { dict, .. } = root.entries.remove(at) else {
        return None;
    };
    let text = |key: &str| dict.get_word(key).unwrap_or_default().to_string();
    Some(Header {
        version: text("version"),
        format: text("format"),
        class: text("class"),
        object: text("object"),
    })
}

struct Parser<'a> {
    text: &'a str,
    lexer: Lexer<'a>,
    peeked: Option<Lex>,
    max_depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Result<Option<Lex>, ParseError> {
        if self.peeked.is_none() {
            self.peeked = self.lexer.next_lex()?;
        }
        Ok(self.peeked)
    }

    fn bump(&mut self) -> Result<Option<Lex>, ParseError> {
        let lex = self.peek()?;
        self.peeked = None;
        Ok(lex)
    }

    fn slice(&self, lex: Lex) -> &'a str {
        let text: &'a str = self.text;
        text.get(lex.start..lex.end).unwrap_or_default()
    }

    /// The depth of what opens inside something at `depth`, refused past the cap.
    fn deeper(&self, depth: usize) -> Result<usize, ParseError> {
        if depth >= self.max_depth {
            Err(ParseError::TooDeep {
                max: self.max_depth,
            })
        } else {
            Ok(depth + 1)
        }
    }

    /// The entries up to the closing brace (when `open_line` is set) or the end of the text.
    fn dict(&mut self, depth: usize, open_line: Option<u32>) -> Result<Dict, ParseError> {
        let mut entries = Vec::new();
        loop {
            let Some(lex) = self.bump()? else {
                return match open_line {
                    Some(line) => Err(ParseError::Unterminated { line }),
                    None => Ok(Dict { entries }),
                };
            };
            match lex.kind {
                Kind::Punct('}') if open_line.is_some() => return Ok(Dict { entries }),
                Kind::Punct(';') => {}
                Kind::Punct(c) => return Err(unexpected(lex.line, c)),
                Kind::Verbatim => return Err(unexpected(lex.line, "#{")),
                Kind::Word if self.slice(lex).starts_with('#') => {
                    let directive = self.directive(lex, depth)?;
                    entries.push(Entry::Directive(directive));
                }
                Kind::Word | Kind::Str => {
                    let entry = self.entry(lex, depth)?;
                    entries.push(entry);
                }
            }
        }
    }

    fn nested(&mut self, depth: usize, open_line: u32) -> Result<Dict, ParseError> {
        let inner = self.deeper(depth)?;
        self.dict(inner, Some(open_line))
    }

    fn entry(&mut self, key_lex: Lex, depth: usize) -> Result<Entry, ParseError> {
        let line = key_lex.line;
        let raw = self.slice(key_lex);
        let key = if key_lex.kind == Kind::Str {
            unquote(raw)
        } else {
            raw.to_string()
        };
        let Some(next) = self.peek()? else {
            return Err(ParseError::Unterminated { line });
        };
        match next.kind {
            Kind::Punct('{') => {
                self.bump()?;
                let dict = self.nested(depth, next.line)?;
                Ok(Entry::Block { key, dict, line })
            }
            Kind::Punct(';') if key_lex.kind == Kind::Word && key.starts_with('$') => {
                self.bump()?;
                let name = key.strip_prefix('$').unwrap_or(&key).to_string();
                Ok(Entry::Macro { name, line })
            }
            _ => {
                let tokens = self.tokens(depth, line)?;
                Ok(Entry::Value { key, tokens, line })
            }
        }
    }

    /// The tokens of a value, up to and including its semicolon.
    fn tokens(&mut self, depth: usize, entry_line: u32) -> Result<Vec<Token>, ParseError> {
        let mut out = Vec::new();
        loop {
            let Some(lex) = self.bump()? else {
                return Err(ParseError::Unterminated { line: entry_line });
            };
            if lex.kind == Kind::Punct(';') {
                return Ok(out);
            }
            out.push(self.token(lex, depth)?);
        }
    }

    fn token(&mut self, lex: Lex, depth: usize) -> Result<Token, ParseError> {
        let raw = self.slice(lex);
        match lex.kind {
            Kind::Word => Ok(Token::Word(raw.to_string())),
            Kind::Str => Ok(Token::Str(unquote(raw))),
            Kind::Verbatim => {
                let inner = raw
                    .strip_prefix("#{")
                    .and_then(|r| r.strip_suffix("#}"))
                    .unwrap_or(raw);
                Ok(Token::Verbatim(inner.to_string()))
            }
            Kind::Punct('(') => Ok(Token::List(self.group(')', depth, lex.line)?)),
            Kind::Punct('[') => Ok(Token::Dims(self.group(']', depth, lex.line)?)),
            Kind::Punct('{') => Ok(Token::Dict(self.nested(depth, lex.line)?)),
            Kind::Punct(c) => Err(unexpected(lex.line, c)),
        }
    }

    /// The tokens of a list or bracket group, up to and including its closing bracket.
    fn group(
        &mut self,
        close: char,
        depth: usize,
        open_line: u32,
    ) -> Result<Vec<Token>, ParseError> {
        let inner = self.deeper(depth)?;
        let mut out = Vec::new();
        loop {
            let Some(lex) = self.bump()? else {
                return Err(ParseError::Unterminated { line: open_line });
            };
            if lex.kind == Kind::Punct(close) {
                return Ok(out);
            }
            out.push(self.token(lex, inner)?);
        }
    }

    /// A directive entry: the pieces on its own line are its arguments; a `{ ... }` body (on
    /// the same line or the next) is skipped by brace matching and never read as entries.
    fn directive(&mut self, lex: Lex, depth: usize) -> Result<Directive, ParseError> {
        let line = lex.line;
        let word = self.slice(lex).to_string();
        let mut args = Vec::new();
        let mut span: Option<(usize, usize)> = None;
        let mut has_body = false;
        while let Some(next) = self.peek()? {
            if next.line != line {
                break;
            }
            match next.kind {
                Kind::Punct('{') => {
                    self.bump()?;
                    self.skip_body(depth, next.line)?;
                    has_body = true;
                    break;
                }
                Kind::Punct(';') => {
                    self.bump()?;
                    break;
                }
                Kind::Punct('}') => break,
                _ => {
                    self.bump()?;
                    args.push(self.slice(next).to_string());
                    span = Some((span.map_or(next.start, |s| s.0), next.end));
                }
            }
        }
        if !has_body
            && let Some(next) = self.peek()?
            && next.kind == Kind::Punct('{')
        {
            self.bump()?;
            self.skip_body(depth, next.line)?;
            has_body = true;
        }
        let text: &'a str = self.text;
        let arg_text = span
            .and_then(|(start, end)| text.get(start..end))
            .unwrap_or_default()
            .to_string();
        Ok(Directive {
            word,
            args,
            arg_text,
            has_body,
            line,
        })
    }

    /// Skips a body whose opening brace was just read, by brace matching within the depth cap.
    fn skip_body(&mut self, depth: usize, open_line: u32) -> Result<(), ParseError> {
        let base = self.deeper(depth)?;
        let mut extra = 0usize;
        loop {
            let Some(lex) = self.bump()? else {
                return Err(ParseError::Unterminated { line: open_line });
            };
            match lex.kind {
                Kind::Punct('{') => {
                    extra += 1;
                    if base + extra > self.max_depth {
                        return Err(ParseError::TooDeep {
                            max: self.max_depth,
                        });
                    }
                }
                Kind::Punct('}') => {
                    if extra == 0 {
                        return Ok(());
                    }
                    extra -= 1;
                }
                _ => {}
            }
        }
    }
}
