//! The tokenizer: words, quoted strings, `#{ ... #}` verbatim text and the punctuation
//! `{ } ( ) [ ] ;`, each with the line it starts on. Comments are skipped.

use crate::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Str,
    Verbatim,
    Punct(char),
}

/// One token as a byte range of the text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lex {
    pub kind: Kind,
    pub line: u32,
    pub start: usize,
    pub end: usize,
}

pub(crate) struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    count: usize,
    max_tokens: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(text: &'a str, max_tokens: usize) -> Self {
        Self {
            bytes: text.as_bytes(),
            pos: 0,
            line: 1,
            count: 0,
            max_tokens,
        }
    }

    /// The next token, or `None` at the end of the text.
    pub(crate) fn next_lex(&mut self) -> Result<Option<Lex>, ParseError> {
        self.skip_blank()?;
        let Some(&b) = self.bytes.get(self.pos) else {
            return Ok(None);
        };
        let start = self.pos;
        let line = self.line;
        let kind = match b {
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b';' => {
                self.pos += 1;
                Kind::Punct(char::from(b))
            }
            b'"' => {
                self.scan_string(line)?;
                Kind::Str
            }
            b'#' if self.bytes.get(self.pos + 1) == Some(&b'{') => {
                self.scan_verbatim(line)?;
                Kind::Verbatim
            }
            _ => {
                self.scan_word();
                Kind::Word
            }
        };
        self.count += 1;
        if self.count > self.max_tokens {
            return Err(ParseError::TooManyTokens {
                max: self.max_tokens,
            });
        }
        Ok(Some(Lex {
            kind,
            line,
            start,
            end: self.pos,
        }))
    }

    fn newline(&mut self) {
        self.line = self.line.saturating_add(1);
    }

    fn skip_blank(&mut self) -> Result<(), ParseError> {
        while let Some(&b) = self.bytes.get(self.pos) {
            match b {
                b'\n' => {
                    self.newline();
                    self.pos += 1;
                }
                _ if b.is_ascii_whitespace() => self.pos += 1,
                b'/' => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        while self.bytes.get(self.pos).is_some_and(|&c| c != b'\n') {
                            self.pos += 1;
                        }
                    }
                    Some(b'*') => self.skip_block_comment()?,
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    fn skip_block_comment(&mut self) -> Result<(), ParseError> {
        let open_line = self.line;
        self.pos += 2;
        loop {
            match self.bytes.get(self.pos) {
                None => return Err(ParseError::Unterminated { line: open_line }),
                Some(b'*') if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                    self.pos += 2;
                    return Ok(());
                }
                Some(b'\n') => {
                    self.newline();
                    self.pos += 1;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A quoted string; a backslash escapes the next byte, and a string may span lines.
    fn scan_string(&mut self, open_line: u32) -> Result<(), ParseError> {
        self.pos += 1;
        loop {
            match self.bytes.get(self.pos) {
                None => return Err(ParseError::Unterminated { line: open_line }),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    if self.bytes.get(self.pos + 1) == Some(&b'\n') {
                        self.newline();
                    }
                    self.pos += 2;
                }
                Some(b'\n') => {
                    self.newline();
                    self.pos += 1;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Verbatim text from `#{` to `#}`; braces and semicolons inside it mean nothing.
    fn scan_verbatim(&mut self, open_line: u32) -> Result<(), ParseError> {
        self.pos += 2;
        loop {
            match self.bytes.get(self.pos) {
                None => return Err(ParseError::Unterminated { line: open_line }),
                Some(b'#') if self.bytes.get(self.pos + 1) == Some(&b'}') => {
                    self.pos += 2;
                    return Ok(());
                }
                Some(b'\n') => {
                    self.newline();
                    self.pos += 1;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A word runs to whitespace, a brace, a bracket, a semicolon, a quote or the start of a
    /// comment. Round brackets that follow word characters stay inside the word while they are
    /// balanced, so `div(phi,U)` is one word; a bracket that starts a token is punctuation.
    fn scan_word(&mut self) {
        let mut open = 0usize;
        while let Some(&b) = self.bytes.get(self.pos) {
            match b {
                b'(' => open += 1,
                b')' => {
                    if open == 0 {
                        break;
                    }
                    open -= 1;
                }
                b'{' | b'}' | b'[' | b']' | b';' | b'"' => break,
                b'/' if matches!(self.bytes.get(self.pos + 1), Some(b'/' | b'*')) => break,
                _ if b.is_ascii_whitespace() => break,
                _ => {}
            }
            self.pos += 1;
        }
    }
}
