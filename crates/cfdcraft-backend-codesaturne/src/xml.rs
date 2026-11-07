//! A small reader for the XML subset a case setup uses: elements, attributes in single or double
//! quotes, text, comments, the five predefined entities and one leading declaration.
//!
//! Everything else is refused by name: a document type declaration, any other entity reference,
//! a processing instruction, a CDATA section, nesting deeper than [`MAX_DEPTH`] and a document
//! larger than [`MAX_BYTES`]. The reader never opens a file or follows a reference, so a setup
//! cannot make it read anything but the text it is given.

use thiserror::Error;

/// The largest document the reader accepts, in bytes.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// The deepest nesting the reader accepts; the root element is level 1.
pub const MAX_DEPTH: usize = 32;

/// One element with its attributes in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// The text directly inside the element, entities resolved and surrounding white space cut.
    pub text: String,
    /// The line the start tag is on, from 1.
    pub line: u32,
}

/// Why a document was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum XmlError {
    #[error("line {line}: a document type declaration is not accepted")]
    Doctype { line: u32 },
    #[error("line {line}: the entity reference {name} is not one of the five predefined ones")]
    Entity { line: u32, name: String },
    #[error("line {line}: a processing instruction is not accepted")]
    ProcessingInstruction { line: u32 },
    #[error("line {line}: a CDATA section is not accepted")]
    CData { line: u32 },
    #[error("line {line}: elements are nested deeper than {limit} levels")]
    TooDeep { line: u32, limit: usize },
    #[error("the document is larger than {limit} bytes")]
    TextTooLong { limit: usize },
    #[error("line {line}: {what}")]
    Malformed { line: u32, what: &'static str },
}

impl Element {
    /// The value of an attribute.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first child with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    /// Every child with this name, in document order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The element at a path of child names, taking the first child of each name.
    pub fn find(&self, path: &[&str]) -> Option<&Element> {
        path.iter()
            .try_fold(self, |element, name| element.child(name))
    }

    /// Visit this element and every element below it, parents first.
    pub fn walk(&self, visit: &mut impl FnMut(&Element)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }
}

/// Read one document.
pub fn parse(text: &str) -> Result<Element, XmlError> {
    if text.len() > MAX_BYTES {
        return Err(XmlError::TextTooLong { limit: MAX_BYTES });
    }
    let mut parser = Parser { src: text, pos: 0 };
    parser.skip_declaration()?;
    parser.skip_misc()?;
    let root = parser.element(1)?;
    parser.skip_misc()?;
    if parser.pos < text.len() {
        return Err(parser.malformed("content after the root element"));
    }
    Ok(root)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn line_at(&self, pos: usize) -> u32 {
        let end = pos.min(self.src.len());
        let newlines = self.src.as_bytes()[..end]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        u32::try_from(newlines + 1).unwrap_or(u32::MAX)
    }

    fn malformed(&self, what: &'static str) -> XmlError {
        XmlError::Malformed {
            line: self.line_at(self.pos),
            what,
        }
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    /// Skip white space; true when any was skipped.
    fn skip_ws(&mut self) -> bool {
        let start = self.pos;
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
        self.pos > start
    }

    /// The declaration is accepted only at the very start of the document.
    fn skip_declaration(&mut self) -> Result<(), XmlError> {
        let rest = self.rest();
        let declared = rest.starts_with("<?xml")
            && matches!(
                rest.as_bytes().get(5),
                Some(b' ' | b'\t' | b'\r' | b'\n' | b'?')
            );
        if declared {
            let Some(end) = rest.find("?>") else {
                return Err(self.malformed("the declaration is not closed"));
            };
            self.pos += end + 2;
        }
        Ok(())
    }

    /// Skip white space and comments, refusing everything else that starts with `<!` or `<?`.
    fn skip_misc(&mut self) -> Result<(), XmlError> {
        loop {
            self.skip_ws();
            let rest = self.rest();
            if rest.starts_with("<!--") {
                self.skip_comment()?;
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                return Err(self.refuse_markup());
            } else {
                return Ok(());
            }
        }
    }

    /// The error for a construct that starts with `<!` or `<?` and is not a comment.
    fn refuse_markup(&self) -> XmlError {
        let line = self.line_at(self.pos);
        let rest = self.rest();
        if rest.starts_with("<![CDATA[") {
            XmlError::CData { line }
        } else if rest.starts_with("<?") {
            XmlError::ProcessingInstruction { line }
        } else if rest.starts_with("<!DOCTYPE") {
            XmlError::Doctype { line }
        } else {
            XmlError::Malformed {
                line,
                what: "a markup declaration is not accepted",
            }
        }
    }

    fn skip_comment(&mut self) -> Result<(), XmlError> {
        let Some(end) = self.rest()[4..].find("-->") else {
            return Err(self.malformed("the comment is not closed"));
        };
        self.pos += 4 + end + 3;
        Ok(())
    }

    fn name(&mut self) -> Result<String, XmlError> {
        let start = self.pos;
        let first = self.peek();
        if !matches!(first, Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b':')) {
            return Err(self.malformed("a name was expected"));
        }
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b':' | b'.' | b'-')
        ) {
            self.pos += 1;
        }
        Ok(self.src[start..self.pos].to_owned())
    }

    fn expect(&mut self, byte: u8, what: &'static str) -> Result<(), XmlError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.malformed(what))
        }
    }

    /// Resolve the five predefined entities in `raw`, which starts at byte `at` of the document.
    fn decode(&self, raw: &str, at: usize) -> Result<String, XmlError> {
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            let after = &rest[amp + 1..];
            let Some(semi) = after.find(';').filter(|semi| *semi <= 32) else {
                return Err(XmlError::Malformed {
                    line: self.line_at(at),
                    what: "an entity reference is not closed",
                });
            };
            let name = &after[..semi];
            out.push(match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                other => {
                    return Err(XmlError::Entity {
                        line: self.line_at(at),
                        name: other.to_owned(),
                    });
                }
            });
            rest = &after[semi + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn attribute_value(&mut self) -> Result<String, XmlError> {
        let quote = match self.peek() {
            Some(q @ (b'"' | b'\'')) => q,
            _ => return Err(self.malformed("a quoted attribute value was expected")),
        };
        self.pos += 1;
        let start = self.pos;
        let Some(len) = self.rest().find(char::from(quote)) else {
            return Err(self.malformed("the attribute value is not closed"));
        };
        let raw = &self.src[start..start + len];
        if raw.contains('<') {
            return Err(self.malformed("an attribute value holds a less-than sign"));
        }
        let value = self.decode(raw, start)?;
        self.pos = start + len + 1;
        Ok(value)
    }

    fn element(&mut self, depth: usize) -> Result<Element, XmlError> {
        let line = self.line_at(self.pos);
        if depth > MAX_DEPTH {
            return Err(XmlError::TooDeep {
                line,
                limit: MAX_DEPTH,
            });
        }
        self.expect(b'<', "an element was expected")?;
        let name = self.name()?;
        let mut attributes: Vec<(String, String)> = Vec::new();
        loop {
            let spaced = self.skip_ws();
            match self.peek() {
                Some(b'/') => {
                    self.pos += 1;
                    self.expect(b'>', "the empty element tag is not closed")?;
                    return Ok(Element {
                        name,
                        attributes,
                        children: Vec::new(),
                        text: String::new(),
                        line,
                    });
                }
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(_) if spaced => {
                    let key = self.name()?;
                    self.skip_ws();
                    self.expect(b'=', "an equals sign was expected after the attribute name")?;
                    self.skip_ws();
                    let value = self.attribute_value()?;
                    if attributes.iter().any(|(existing, _)| *existing == key) {
                        return Err(self.malformed("an attribute is given twice"));
                    }
                    attributes.push((key, value));
                }
                _ => return Err(self.malformed("an attribute or the end of the tag was expected")),
            }
        }
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            let Some(lt) = self.rest().find('<') else {
                return Err(self.malformed("the element is not closed"));
            };
            let piece = self.decode(&self.rest()[..lt], self.pos)?;
            text.push_str(&piece);
            self.pos += lt;
            let rest = self.rest();
            if rest.starts_with("</") {
                self.pos += 2;
                let closing = self.name()?;
                self.skip_ws();
                self.expect(b'>', "the closing tag is not closed")?;
                if closing != name {
                    return Err(self.malformed("the closing tag does not match the start tag"));
                }
                break;
            } else if rest.starts_with("<!--") {
                self.skip_comment()?;
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                return Err(self.refuse_markup());
            } else {
                children.push(self.element(depth + 1)?);
            }
        }
        Ok(Element {
            name,
            attributes,
            children,
            text: text.trim().to_owned(),
            line,
        })
    }
}
