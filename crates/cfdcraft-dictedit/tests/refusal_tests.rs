use cfdcraft_dictedit::{EditError, MAX_TEXT, get_entry, set_entry};

const SAMPLE: &str =
    "solvers\n{\n    p { tolerance 1e-10; }\n}\nSIMPLE\n{\n    consistent no;\n}\n";

#[test]
fn a_missing_block_is_refused() {
    for entry in ["nothere/x", "solvers/nothere/x", "SIMPLE/consistent/x"] {
        assert_eq!(
            set_entry(SAMPLE, entry, "1"),
            Err(EditError::NoSuchBlock {
                entry: entry.to_owned()
            }),
            "{entry}"
        );
        assert_eq!(
            get_entry(SAMPLE, entry),
            Err(EditError::NoSuchBlock {
                entry: entry.to_owned()
            }),
            "{entry}"
        );
    }
}

#[test]
fn a_whole_block_cannot_be_replaced() {
    for entry in ["SIMPLE", "solvers/p"] {
        assert_eq!(
            set_entry(SAMPLE, entry, "1"),
            Err(EditError::NotAnEntry {
                entry: entry.to_owned()
            }),
            "{entry}"
        );
        assert_eq!(
            get_entry(SAMPLE, entry),
            Err(EditError::NotAnEntry {
                entry: entry.to_owned()
            }),
            "{entry}"
        );
    }
}

#[test]
fn a_bad_entry_path_is_refused() {
    let long = "a".repeat(65);
    for entry in [
        "",
        "a//b",
        "/a",
        "a/",
        "a/b/c/d/e/f/g",
        "1a",
        "a/1b",
        "a.b",
        "a b",
        "a-b",
        long.as_str(),
    ] {
        assert!(
            matches!(
                set_entry(SAMPLE, entry, "1"),
                Err(EditError::BadEntry { .. })
            ),
            "{entry:?}"
        );
        assert!(
            matches!(get_entry(SAMPLE, entry), Err(EditError::BadEntry { .. })),
            "{entry:?}"
        );
    }
    assert!(
        set_entry(SAMPLE, "a/b/c/d/e/f", "1")
            .is_err_and(|e| matches!(e, EditError::NoSuchBlock { .. }))
    );
    assert!(set_entry(SAMPLE, &"a".repeat(64), "1").is_ok());
}

#[test]
fn a_bad_value_is_refused() {
    let long = "1".repeat(201);
    for value in [
        "",
        "   ",
        "1;2",
        "{",
        "}",
        "#include x",
        "$other",
        "\"x\"",
        "a\\b",
        "1\n2",
        "1\t2\u{7}",
        "1 // rest",
        "1 /* x",
        long.as_str(),
    ] {
        assert!(
            matches!(
                set_entry(SAMPLE, "SIMPLE/consistent", value),
                Err(EditError::BadValue { .. })
            ),
            "{value:?}"
        );
    }
    let longest = "1".repeat(200);
    assert!(set_entry(SAMPLE, "SIMPLE/consistent", &longest).is_ok());
    assert!(set_entry(SAMPLE, "SIMPLE/consistent", "( 1 0 0 )").is_ok());
}

#[test]
fn unbalanced_braces_are_refused() {
    // An extra `{`: the line of the brace that is never closed.
    assert_eq!(
        set_entry("solvers\n{\n    p 1;\n", "solvers/p", "2"),
        Err(EditError::Unbalanced { line: 2 })
    );
    // An extra `}`: the line of the brace that has no partner.
    assert_eq!(
        set_entry("a\n{\n}\n}\n", "a/b", "2"),
        Err(EditError::Unbalanced { line: 4 })
    );
    assert_eq!(
        get_entry("}\n", "a"),
        Err(EditError::Unbalanced { line: 1 })
    );
}

#[test]
fn something_never_closed_is_refused() {
    assert_eq!(
        set_entry("a 1\n", "a", "2"),
        Err(EditError::Unterminated { line: 1 })
    );
    assert_eq!(
        set_entry("x 1;\nnote \"open;\n", "x", "2"),
        Err(EditError::Unterminated { line: 2 })
    );
    assert_eq!(
        set_entry("x 1;\n/* never ends\n", "x", "2"),
        Err(EditError::Unterminated { line: 2 })
    );
}

#[test]
fn a_text_over_one_mebibyte_is_refused() {
    let big = "x".repeat(MAX_TEXT + 1);
    assert_eq!(
        set_entry(&big, "a", "1"),
        Err(EditError::TooLarge {
            size: MAX_TEXT + 1,
            max: MAX_TEXT
        })
    );
    assert!(matches!(
        get_entry(&big, "a"),
        Err(EditError::TooLarge { .. })
    ));
    // Exactly at the limit is read.
    let at_limit = format!("a 1;\n//{}", "x".repeat(MAX_TEXT - 7));
    assert_eq!(at_limit.len(), MAX_TEXT);
    assert!(set_entry(&at_limit, "a", "2").is_ok());
}

#[test]
fn nesting_over_32_is_refused() {
    let nested = |n: usize| format!("{}{}", "a { ".repeat(n), "} ".repeat(n));
    assert_eq!(
        set_entry(&nested(33), "b", "1"),
        Err(EditError::TooDeep { max: 32 })
    );
    assert!(set_entry(&nested(32), "b", "1").is_ok());
}

#[test]
fn a_repeated_keyword_in_one_block_is_refused() {
    assert_eq!(
        set_entry("a 1;\na 2;\n", "a", "3"),
        Err(EditError::Ambiguous {
            entry: "a".to_owned()
        })
    );
    // The repeated name is a block on the path.
    assert_eq!(
        set_entry("blk { x 1; }\nblk { y 2; }\n", "blk/x", "3"),
        Err(EditError::Ambiguous {
            entry: "blk/x".to_owned()
        })
    );
    assert!(matches!(
        get_entry("a 1;\na 2;\n", "a"),
        Err(EditError::Ambiguous { .. })
    ));
    // The same name in different blocks is not a repeat.
    assert!(set_entry("p { x 1; }\nq { x 2; }\n", "q/x", "3").is_ok());
}
