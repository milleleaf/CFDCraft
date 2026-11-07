//! The only way raw solver text may reach a person.
//!
//! A solver log can name the work folder on the compute host, the container, and long ids. Every
//! line shown to a person goes through [`redact`] first. It keeps the words of the message and
//! the paths inside the case (`/w` and below), and hides the rest.

/// The longest a line may be after redaction, in characters.
pub const MAX_LINE_CHARS: usize = 200;

/// The text with host paths, run names and long ids hidden, lines cut at [`MAX_LINE_CHARS`]
/// characters, and control characters other than the tab dropped. The cost is proportional to
/// the text, so pass a tail (see [`tail_lines`]) rather than a whole log.
pub fn redact(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

/// Like [`redact`], but a line is not cut: for a sentence built from text that was already
/// redacted line by line (so each part is bounded), where a cut would lose the end of the tail.
pub fn redact_sentence(text: &str) -> String {
    text.lines()
        .map(|line| {
            let clean: String = line
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            hide_ids(&hide_runs(&hide_paths(&clean)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last `n` lines of `text`, never part of a line. `n` larger than the text gives all of it
/// and 0 gives nothing. A final newline stays with the last line.
pub fn tail_lines(text: &str, n: usize) -> &str {
    if n == 0 {
        return &text[text.len()..];
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    for (count, (index, _)) in body.rmatch_indices('\n').enumerate() {
        if count + 1 == n {
            return &text[index + 1..];
        }
    }
    text
}

fn redact_line(line: &str) -> String {
    let cut: String = line
        .chars()
        .take(MAX_LINE_CHARS)
        .filter(|c| !c.is_control() || *c == '\t')
        .collect();
    hide_ids(&hide_runs(&hide_paths(&cut)))
}

/// A path starts at a `/` that begins a word or follows an opening quote or separator.
fn starts_path(before: u8) -> bool {
    before.is_ascii_whitespace() || b"\"'(<[{=,:;".contains(&before)
}

fn ends_path(byte: u8) -> bool {
    byte.is_ascii_whitespace() || b"\"')>]},;:".contains(&byte)
}

/// A path inside the case: `/w` or below it, never climbing out.
fn inside_case(path: &str) -> bool {
    (path == "/w" || path.starts_with("/w/")) && !path.contains("..")
}

fn hide_paths(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && (i == 0 || starts_path(bytes[i - 1])) {
            let end = (i..bytes.len())
                .find(|&j| ends_path(bytes[j]))
                .unwrap_or(bytes.len());
            let path = &line[i..end];
            out.push_str(&line[last..i]);
            out.push_str(if inside_case(path) { path } else { "<path>" });
            last = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&line[last..]);
    out
}

fn hide_runs(line: &str) -> String {
    const PREFIX: &str = "cfdcraft-";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(PREFIX) {
        let after = &rest[pos + PREFIX.len()..];
        let name = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
            .count();
        out.push_str(&rest[..pos]);
        if name > 0 {
            out.push_str("<run>");
            rest = &after[name..];
        } else {
            out.push_str(PREFIX);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// A digit run next to a decimal point belongs to a number, not to an id.
fn in_number(bytes: &[u8], start: usize, end: usize) -> bool {
    let before = start >= 2 && bytes[start - 1] == b'.' && bytes[start - 2].is_ascii_digit();
    let after = end < bytes.len()
        && bytes[end] == b'.'
        && bytes.get(end + 1).is_some_and(u8::is_ascii_digit);
    before || after
}

fn hide_ids(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let word = &bytes[start..i];
            if word.len() >= 12
                && word.iter().all(u8::is_ascii_hexdigit)
                && !in_number(bytes, start, i)
            {
                out.push_str(&line[last..start]);
                out.push_str("<id>");
                last = i;
            }
        } else {
            i += 1;
        }
    }
    out.push_str(&line[last..]);
    out
}
