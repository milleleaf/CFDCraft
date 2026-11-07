use cfdcraft_text::{Embedded, Line, SOURCES, Text};
use std::collections::BTreeSet;

fn migrated() -> Line {
    Line::Migrated {
        from: 0,
        to: 1,
        summary: "Moved the lid.".to_string(),
    }
}

#[test]
fn migrated_renders_in_english() {
    let out = Text::english().render(&migrated());
    assert_eq!(
        out,
        "This file was written by an older version (schema 0). It was updated to schema 1 in memory; the file on disk is unchanged until you save. Moved the lid."
    );
    assert!(!out.contains('{'));
}

#[test]
fn pseudo_marks_the_line_and_records_the_lookup() {
    let pseudo = Text::pseudo();
    let out = pseudo.render(&migrated());
    assert_ne!(out, Text::english().render(&migrated()));
    assert!(out.contains('0') && out.contains('1') && out.contains("Moved the lid."));
    assert!(pseudo.looked_up().contains(migrated().source()));
}

#[test]
fn english_text_ignores_the_machine_locale() {
    // Run the English check again in a child process whose locale variables name another language.
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "english_text_is_english", "--nocapture"])
        .env("LANG", "de_DE.UTF-8")
        .env("LC_ALL", "de_DE.UTF-8")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed"), "{stdout}");
}

#[test]
fn english_text_is_english() {
    let out = Text::english().render(&Line::Saved {
        path: "a".to_string(),
    });
    assert_eq!(out, "Saved a.");
}

fn texts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_variant_renders_to_its_english_sentence() {
    let expected = texts(&[
        "This file was written by an older version (schema 0). It was updated to schema 1 in memory; the file on disk is unchanged until you save. Moved the lid.",
        "Saved a.cfdcraft.",
        "Opened a.cfdcraft.",
        "This file was written by a newer version of CFDCraft (schema 9). This version reads schemas up to 1, so nothing was read from the file.",
        "a.zip is not a CFDCraft study file: no manifest",
        "The linked file part.step is missing. It was expected at geometry/part.step.",
        "The linked file part.step has changed since it was linked.",
        "The link to part.step cannot be followed: it leaves the project folder",
        "Linked part.step as geometry/part.step.",
        "A stored result (ab12) is missing from the store next to the file.",
        "A stored result (ab12) does not match its recorded fingerprint.",
        "Packed 2 stored results into a.pack.",
        "Unpacked 2 stored results into a-dir.",
        "The pack does not include 2 linked files; copy them next to the unpacked study.",
        "a.cfdcraft already exists. Choose another name or remove it first.",
        "Stored result ab12 is not in the store, so the study cannot be packed.",
        "Stored result ab12 in the pack does not match its fingerprint. No study file was written.",
        "The pack is missing stored result ab12. No study file was written.",
        "a.cfdcraft could not be written: the disk is full",
        "part.step could not be linked: a link named part already exists",
    ]);
    let samples = Line::samples();
    assert_eq!(samples.len(), expected.len());
    let english = Text::english();
    for (line, want) in samples.iter().zip(&expected) {
        let got = english.render(line);
        assert_eq!(&got, want);
        assert!(!got.contains('{') && !got.contains('}'), "{got}");
    }
}

#[test]
fn plurals_use_the_right_form() {
    let english = Text::english();
    let packed = |count| {
        english.render(&Line::Packed {
            count,
            path: "out.pack".to_string(),
        })
    };
    assert_eq!(packed(1), "Packed 1 stored result into out.pack.");
    assert_eq!(packed(2), "Packed 2 stored results into out.pack.");
    assert_eq!(packed(0), "Packed 0 stored results into out.pack.");
    assert_eq!(
        packed(1234),
        "Packed 1234 stored results into out.pack.",
        "counts use plain digits, no grouping"
    );
    let unpacked = |count| {
        english.render(&Line::Unpacked {
            count,
            path: "dir".to_string(),
        })
    };
    assert_eq!(unpacked(1), "Unpacked 1 stored result into dir.");
    assert_eq!(unpacked(3), "Unpacked 3 stored results into dir.");
    let links = |count| english.render(&Line::LinksNotPacked { count });
    assert_eq!(
        links(1),
        "The pack does not include 1 linked file; copy it next to the unpacked study."
    );
    assert_eq!(
        links(2),
        "The pack does not include 2 linked files; copy them next to the unpacked study."
    );
}

#[test]
fn a_value_that_looks_like_a_placeholder_is_inserted_as_written() {
    let english = Text::english();
    let out = english.render(&Line::LinkChanged {
        name: "a{path}b".to_string(),
    });
    assert_eq!(
        out,
        "The linked file a{path}b has changed since it was linked."
    );
    let out = english.render(&Line::LinkAdded {
        name: "{path}".to_string(),
        path: "{name}".to_string(),
    });
    assert_eq!(out, "Linked {path} as {name}.");
    let out = english.render(&Line::Packed {
        count: 2,
        path: "{n}".to_string(),
    });
    assert_eq!(out, "Packed 2 stored results into {n}.");
}

#[test]
fn direction_marks_in_a_value_are_dropped() {
    let hostile = "a\u{202E}b\u{2067}c\u{200F}d";
    let out = Text::english().render(&Line::Saved {
        path: hostile.to_string(),
    });
    assert_eq!(out, "Saved abcd.");
}

#[test]
fn the_pseudo_locale_shows_every_source_was_looked_up() {
    let pseudo = Text::pseudo();
    let english = Text::english();
    for line in Line::samples() {
        let out = pseudo.render(&line);
        assert!(out.starts_with('\u{27E6}'), "{out}");
        assert_ne!(out, english.render(&line));
        assert!(!out.contains('{'), "{out}");
    }
    let looked_up = pseudo.looked_up();
    for line in Line::samples() {
        if line.plural_source().is_none() {
            assert!(looked_up.contains(line.source()), "{}", line.source());
        }
    }
}

#[test]
fn the_samples_cover_every_variant_and_every_source() {
    let samples = Line::samples();
    let kinds: BTreeSet<String> = samples
        .iter()
        .map(|l| {
            format!("{l:?}")
                .split([' ', '{'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds.len(), samples.len(), "one sample per variant");
    let mut from_samples: BTreeSet<&str> = BTreeSet::new();
    for line in &samples {
        from_samples.insert(line.source());
        if let Some(other) = line.plural_source() {
            from_samples.insert(other);
        }
    }
    let listed: BTreeSet<&str> = SOURCES.iter().copied().collect();
    assert_eq!(listed.len(), SOURCES.len(), "no source listed twice");
    assert_eq!(from_samples, listed);
}

static GOOD: &Embedded = &[("de", "", "\tSaved {path}.\tTESTDATA gespeichert: {path}\n")];
static BAD: &Embedded = &[("de", "", "\tOpened {path}.\tTESTDATA geoeffnet\n")];

#[test]
fn a_catalog_row_replaces_one_sentence() {
    let text = Text::with_catalogs(GOOD, &["de"], "de").unwrap();
    assert!(text.check_all().is_empty(), "{:?}", text.check_all());
    let saved = text.render(&Line::Saved {
        path: "a".to_string(),
    });
    assert_eq!(saved, "TESTDATA gespeichert: a");
    let opened = text.render(&Line::Opened {
        path: "a".to_string(),
    });
    assert_eq!(opened, "Opened a.");
}

#[test]
fn a_row_with_a_missing_placeholder_is_reported_and_skipped() {
    let text = Text::with_catalogs(BAD, &["de"], "de").unwrap();
    assert!(!text.check_all().is_empty());
    let opened = text.render(&Line::Opened {
        path: "a".to_string(),
    });
    assert_eq!(opened, "Opened a.");
}

#[test]
fn a_language_that_is_not_offered_gives_no_text() {
    assert!(Text::with_catalogs(GOOD, &[], "de").is_none());
    assert!(Text::with_catalogs(GOOD, &["de"], "fr").is_none());
}
