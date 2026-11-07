//! The sentences the document layer shows a person, looked up through the shared Craft
//! translation library.
//!
//! The document, store and pack crates return typed results and errors. The layer above maps
//! them to a [`Line`], which carries data only, and [`Text`] turns it into a sentence. The
//! wording lives here and nowhere else.
//!
//! The language is chosen by the caller and defaults to English: nothing here reads the
//! machine's locale, so results do not depend on the machine. Output is for terminals and logs:
//! values go in without direction marks (bidirectional control characters in a value are
//! dropped) and counts use plain digits.
//!
//! No translated catalog ships yet. When translations exist, an app embeds them as a list of
//! `(locale, shared catalog text, app catalog text)` built with `include_str!`, and builds a
//! [`Text`] over it with [`Text::with_catalogs`]. A missing row shows the English sentence.

mod lines;

pub use craft_i18n::Embedded;
pub use lines::{Line, SOURCES};

use craft_i18n::{I18n, Lang, fmt_plain};

/// Renders [`Line`]s in one language.
pub struct Text {
    i18n: I18n,
    lang: Lang,
}

/// No catalog is embedded in the real build.
static NO_CATALOGS: &Embedded = &[];

impl Text {
    /// English, the source texts themselves.
    pub fn english() -> Text {
        let (i18n, _) = I18n::new(NO_CATALOGS, &[]);
        Text {
            i18n,
            lang: Lang::EN,
        }
    }

    /// The pseudo locale, which marks every looked-up text and records it. For tests.
    #[doc(hidden)]
    pub fn pseudo() -> Text {
        let (i18n, _) = I18n::new(NO_CATALOGS, &[]);
        Text {
            i18n,
            lang: Lang::PSEUDO,
        }
    }

    /// A text over `embedded` in the offered language `code`, or `None` when that language is
    /// not offered.
    pub fn with_catalogs(
        embedded: &'static Embedded,
        offered: &[&str],
        code: &str,
    ) -> Option<Text> {
        let (i18n, _) = I18n::new(embedded, offered);
        let lang = i18n.from_code(code)?;
        Some(Text { i18n, lang })
    }

    /// Every problem in the embedded catalogs, for an audit.
    pub fn check_all(&self) -> Vec<String> {
        self.i18n.check_all()
    }

    /// The source texts looked up in the pseudo locale so far (plurals are not recorded).
    pub fn looked_up(&self) -> std::collections::BTreeSet<String> {
        self.i18n.looked_up()
    }

    /// The sentence for `line`.
    pub fn render(&self, line: &Line) -> String {
        let template = match (line.count(), line.plural_source()) {
            (Some(n), Some(other)) => self.i18n.trn_plain(self.lang, n, line.source(), other),
            _ => self.i18n.t(self.lang, line.source()).to_string(),
        };
        let mut args: Vec<(&str, String)> = line.args();
        if let Line::Migrated { summary, .. } = line {
            // The model's summary is itself a sentence a catalog may translate.
            let translated = self.i18n.t(self.lang, summary).to_string();
            args.retain(|(k, _)| *k != "summary");
            args.push(("summary", translated));
        }
        let args: Vec<(&str, String)> = args.into_iter().map(|(k, v)| (k, plain(&v))).collect();
        let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
        fmt_plain(&template, &args)
    }
}

/// `value` without direction marks and embeddings, so a hostile name cannot reorder the words
/// around it in a terminal or log.
fn plain(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            !matches!(*c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}
