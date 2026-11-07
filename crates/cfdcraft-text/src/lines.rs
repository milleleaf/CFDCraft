//! The sentences the document layer shows a person, as data.
//!
//! A [`Line`] holds only values (names, paths, counts). The wording lives here, once, as an
//! English source text that doubles as the lookup key of the translation catalogs.

const MIGRATED: &str = "This file was written by an older version (schema {from}). It was updated to schema {to} in memory; the file on disk is unchanged until you save. {summary}";
const SAVED: &str = "Saved {path}.";
const OPENED: &str = "Opened {path}.";
const NEWER_SCHEMA: &str = "This file was written by a newer version of CFDCraft (schema {found}). This version reads schemas up to {supported}, so nothing was read from the file.";
const NOT_A_STUDY_FILE: &str = "{path} is not a CFDCraft study file: {reason}";
const LINK_MISSING: &str = "The linked file {name} is missing. It was expected at {expected}.";
const LINK_CHANGED: &str = "The linked file {name} has changed since it was linked.";
const LINK_REFUSED: &str = "The link to {name} cannot be followed: {reason}";
const LINK_ADDED: &str = "Linked {name} as {path}.";
const RESULT_MISSING: &str = "A stored result ({hash}) is missing from the store next to the file.";
const RESULT_CORRUPT: &str = "A stored result ({hash}) does not match its recorded fingerprint.";
const PACKED_ONE: &str = "Packed {n} stored result into {path}.";
const PACKED_OTHER: &str = "Packed {n} stored results into {path}.";
const UNPACKED_ONE: &str = "Unpacked {n} stored result into {path}.";
const UNPACKED_OTHER: &str = "Unpacked {n} stored results into {path}.";
const LINKS_NOT_PACKED_ONE: &str =
    "The pack does not include {n} linked file; copy it next to the unpacked study.";
const LINKS_NOT_PACKED_OTHER: &str =
    "The pack does not include {n} linked files; copy them next to the unpacked study.";
const TARGET_EXISTS: &str = "{path} already exists. Choose another name or remove it first.";
const PACK_MISSING: &str =
    "Stored result {hash} is not in the store, so the study cannot be packed.";
const PACK_DAMAGED: &str =
    "Stored result {hash} in the pack does not match its fingerprint. No study file was written.";
const PACK_INCOMPLETE: &str =
    "The pack is missing stored result {hash}. No study file was written.";
const NOT_WRITTEN: &str = "{path} could not be written: {reason}";
const LINK_NOT_ADDED: &str = "{name} could not be linked: {reason}";

/// Every English source text: the plain templates and both forms of each plural pair.
pub const SOURCES: &[&str] = &[
    MIGRATED,
    SAVED,
    OPENED,
    NEWER_SCHEMA,
    NOT_A_STUDY_FILE,
    LINK_MISSING,
    LINK_CHANGED,
    LINK_REFUSED,
    LINK_ADDED,
    RESULT_MISSING,
    RESULT_CORRUPT,
    PACKED_ONE,
    PACKED_OTHER,
    UNPACKED_ONE,
    UNPACKED_OTHER,
    LINKS_NOT_PACKED_ONE,
    LINKS_NOT_PACKED_OTHER,
    TARGET_EXISTS,
    PACK_MISSING,
    PACK_DAMAGED,
    PACK_INCOMPLETE,
    NOT_WRITTEN,
    LINK_NOT_ADDED,
];

/// One thing the document layer tells a person. Data only: the sentence is made by
/// [`Text::render`](crate::Text::render).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// An older file was brought up to the current schema in memory.
    Migrated {
        from: u32,
        to: u32,
        summary: String,
    },
    Saved {
        path: String,
    },
    Opened {
        path: String,
    },
    /// The file is from a newer version than this one reads.
    NewerSchema {
        found: u32,
        supported: u32,
    },
    NotAStudyFile {
        path: String,
        reason: String,
    },
    LinkMissing {
        name: String,
        expected: String,
    },
    LinkChanged {
        name: String,
    },
    LinkRefused {
        name: String,
        reason: String,
    },
    LinkAdded {
        name: String,
        path: String,
    },
    /// A stored result named by its fingerprint is not in the store.
    ResultMissing {
        hash: String,
    },
    /// A stored result does not match its fingerprint.
    ResultCorrupt {
        hash: String,
    },
    Packed {
        count: u64,
        path: String,
    },
    Unpacked {
        count: u64,
        path: String,
    },
    LinksNotPacked {
        count: u64,
    },
    TargetExists {
        path: String,
    },
    PackMissing {
        hash: String,
    },
    PackDamaged {
        hash: String,
    },
    PackIncomplete {
        hash: String,
    },
    /// A file could not be written.
    NotWritten {
        path: String,
        reason: String,
    },
    /// A file could not be linked to the study.
    LinkNotAdded {
        name: String,
        reason: String,
    },
}

impl Line {
    /// The English source text, which is also the catalog key. For a line with a count this is
    /// the singular form; see [`Line::plural_source`].
    pub fn source(&self) -> &'static str {
        match self {
            Line::Migrated { .. } => MIGRATED,
            Line::Saved { .. } => SAVED,
            Line::Opened { .. } => OPENED,
            Line::NewerSchema { .. } => NEWER_SCHEMA,
            Line::NotAStudyFile { .. } => NOT_A_STUDY_FILE,
            Line::LinkMissing { .. } => LINK_MISSING,
            Line::LinkChanged { .. } => LINK_CHANGED,
            Line::LinkRefused { .. } => LINK_REFUSED,
            Line::LinkAdded { .. } => LINK_ADDED,
            Line::ResultMissing { .. } => RESULT_MISSING,
            Line::ResultCorrupt { .. } => RESULT_CORRUPT,
            Line::Packed { .. } => PACKED_ONE,
            Line::Unpacked { .. } => UNPACKED_ONE,
            Line::LinksNotPacked { .. } => LINKS_NOT_PACKED_ONE,
            Line::TargetExists { .. } => TARGET_EXISTS,
            Line::PackMissing { .. } => PACK_MISSING,
            Line::PackDamaged { .. } => PACK_DAMAGED,
            Line::PackIncomplete { .. } => PACK_INCOMPLETE,
            Line::NotWritten { .. } => NOT_WRITTEN,
            Line::LinkNotAdded { .. } => LINK_NOT_ADDED,
        }
    }

    /// The plural form of the source text for a line with a count, else `None`.
    pub fn plural_source(&self) -> Option<&'static str> {
        match self {
            Line::Packed { .. } => Some(PACKED_OTHER),
            Line::Unpacked { .. } => Some(UNPACKED_OTHER),
            Line::LinksNotPacked { .. } => Some(LINKS_NOT_PACKED_OTHER),
            _ => None,
        }
    }

    /// The count of a plural line.
    pub(crate) fn count(&self) -> Option<u64> {
        match self {
            Line::Packed { count, .. }
            | Line::Unpacked { count, .. }
            | Line::LinksNotPacked { count } => Some(*count),
            _ => None,
        }
    }

    /// The values to put into the placeholders, by name (the count is filled separately).
    pub(crate) fn args(&self) -> Vec<(&'static str, String)> {
        let s = |v: &String| v.clone();
        match self {
            Line::Migrated { from, to, summary } => vec![
                ("from", from.to_string()),
                ("to", to.to_string()),
                ("summary", s(summary)),
            ],
            Line::Saved { path } | Line::Opened { path } | Line::TargetExists { path } => {
                vec![("path", s(path))]
            }
            Line::NewerSchema { found, supported } => vec![
                ("found", found.to_string()),
                ("supported", supported.to_string()),
            ],
            Line::NotAStudyFile { path, reason } | Line::NotWritten { path, reason } => {
                vec![("path", s(path)), ("reason", s(reason))]
            }
            Line::LinkMissing { name, expected } => {
                vec![("name", s(name)), ("expected", s(expected))]
            }
            Line::LinkChanged { name } => vec![("name", s(name))],
            Line::LinkRefused { name, reason } | Line::LinkNotAdded { name, reason } => {
                vec![("name", s(name)), ("reason", s(reason))]
            }
            Line::LinkAdded { name, path } => vec![("name", s(name)), ("path", s(path))],
            Line::ResultMissing { hash }
            | Line::ResultCorrupt { hash }
            | Line::PackMissing { hash }
            | Line::PackDamaged { hash }
            | Line::PackIncomplete { hash } => vec![("hash", s(hash))],
            Line::Packed { path, .. } | Line::Unpacked { path, .. } => vec![("path", s(path))],
            Line::LinksNotPacked { .. } => Vec::new(),
        }
    }

    /// One sample of every variant, for tests.
    #[doc(hidden)]
    pub fn samples() -> Vec<Line> {
        let t = |v: &str| v.to_string();
        vec![
            Line::Migrated {
                from: 0,
                to: 1,
                summary: t("Moved the lid."),
            },
            Line::Saved {
                path: t("a.cfdcraft"),
            },
            Line::Opened {
                path: t("a.cfdcraft"),
            },
            Line::NewerSchema {
                found: 9,
                supported: 1,
            },
            Line::NotAStudyFile {
                path: t("a.zip"),
                reason: t("no manifest"),
            },
            Line::LinkMissing {
                name: t("part.step"),
                expected: t("geometry/part.step"),
            },
            Line::LinkChanged {
                name: t("part.step"),
            },
            Line::LinkRefused {
                name: t("part.step"),
                reason: t("it leaves the project folder"),
            },
            Line::LinkAdded {
                name: t("part.step"),
                path: t("geometry/part.step"),
            },
            Line::ResultMissing { hash: t("ab12") },
            Line::ResultCorrupt { hash: t("ab12") },
            Line::Packed {
                count: 2,
                path: t("a.pack"),
            },
            Line::Unpacked {
                count: 2,
                path: t("a-dir"),
            },
            Line::LinksNotPacked { count: 2 },
            Line::TargetExists {
                path: t("a.cfdcraft"),
            },
            Line::PackMissing { hash: t("ab12") },
            Line::PackDamaged { hash: t("ab12") },
            Line::PackIncomplete { hash: t("ab12") },
            Line::NotWritten {
                path: t("a.cfdcraft"),
                reason: t("the disk is full"),
            },
            Line::LinkNotAdded {
                name: t("part.step"),
                reason: t("a link named part already exists"),
            },
        ]
    }
}
