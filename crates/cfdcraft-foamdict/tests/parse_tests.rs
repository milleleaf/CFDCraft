//! The parser on small synthetic dictionaries written from the syntax alone.

use cfdcraft_foamdict::{Dict, Document, Entry, Limits, ParseError, Token, parse};

fn doc(text: &str) -> Document {
    parse(text, &Limits::default()).expect("the fixture parses")
}

fn w(text: &str) -> Token {
    Token::Word(text.to_string())
}

fn list(tokens: Vec<Token>) -> Token {
    Token::List(tokens)
}

fn tokens_of<'a>(dict: &'a Dict, key: &str) -> &'a [Token] {
    match dict.get(key) {
        Some(Entry::Value { tokens, .. }) => tokens.as_slice(),
        other => panic!("no value entry for {key}: {other:?}"),
    }
}

#[test]
fn a_control_dictionary_reads_by_key() {
    let text = [
        "FoamFile",
        "{",
        "    version 2.0;",
        "    format ascii;",
        "    class dictionary;",
        "    object controlDict;",
        "}",
        "application simpleFoam;",
        "deltaT 0.005;",
        "functions",
        "{",
        "    probes1",
        "    {",
        "        type probes;",
        "    }",
        "}",
    ]
    .join("\n");
    let d = doc(&text);
    let header = d.header.as_ref().expect("the header is read");
    assert_eq!(header.class, "dictionary");
    assert_eq!(header.format, "ascii");
    assert_eq!(header.object, "controlDict");
    assert_eq!(header.version, "2.0");
    assert!(d.get("FoamFile").is_none());
    assert_eq!(d.get_word("application"), Some("simpleFoam"));
    let delta = d.get_number("deltaT").expect("deltaT is a number");
    assert!((delta - 0.005).abs() < 1e-12);
    let functions = d.dict("functions").expect("functions is a block");
    let probes = functions.dict("probes1").expect("probes1 is a block");
    assert_eq!(probes.get_word("type"), Some("probes"));
}

#[test]
fn comments_and_strings_are_handled() {
    let text = [
        "// a comment; with } and ;",
        "a 1; /* block ; } comment",
        r#"spanning lines */ b "has ; and } inside \" quote";"#,
        "c 3;",
    ]
    .join("\n");
    let d = doc(&text);
    let lines: Vec<(Option<&str>, u32)> = d
        .root
        .entries()
        .iter()
        .map(|e| (e.key(), e.line()))
        .collect();
    assert_eq!(lines, vec![(Some("a"), 2), (Some("b"), 3), (Some("c"), 4)]);
    assert_eq!(
        tokens_of(&d.root, "b"),
        [Token::Str("has ; and } inside \" quote".to_string())]
    );
    assert_eq!(d.get_number("c"), Some(3.0));
}

#[test]
fn entries_keep_their_order_and_duplicates() {
    let d = doc("a 1;\nb 2;\na 3;");
    let keys: Vec<&str> = d.root.entries().iter().filter_map(Entry::key).collect();
    assert_eq!(keys, vec!["a", "b", "a"]);
    assert_eq!(d.get_number("a"), Some(3.0));
    let all: Vec<&Entry> = d.all("a").collect();
    assert_eq!(all.len(), 2);
    assert!(matches!(
        all[0],
        Entry::Value { tokens, line: 1, .. } if tokens.as_slice() == [w("1")]
    ));
    assert!(matches!(
        all[1],
        Entry::Value { tokens, line: 3, .. } if tokens.as_slice() == [w("3")]
    ));
}

#[test]
fn an_unterminated_block_is_a_named_error() {
    let text = "a 1;\nblock\n{\n    b 2;\n";
    assert_eq!(
        parse(text, &Limits::default()),
        Err(ParseError::Unterminated { line: 3 })
    );
}

#[test]
fn lists_dimensions_and_nested_lists_parse() {
    let text = [
        "dimensions [0 1 -1 0 0 0 0];",
        "internalField uniform (0 0 0);",
        "boundaryField { inlet { type fixedValue; value uniform (1 0 0); } }",
        "nested (((0 0) (1 1)) (2 2));",
        "empty ();",
        "div(phi,U) Gauss linear;",
    ]
    .join("\n");
    let d = doc(&text);
    assert_eq!(
        tokens_of(&d.root, "dimensions"),
        [Token::Dims(
            ["0", "1", "-1", "0", "0", "0", "0"].map(w).to_vec()
        )]
    );
    assert_eq!(
        tokens_of(&d.root, "internalField"),
        [w("uniform"), list(vec![w("0"), w("0"), w("0")])]
    );
    let inlet = d
        .dict("boundaryField")
        .and_then(|b| b.dict("inlet"))
        .expect("the inlet block");
    assert_eq!(
        tokens_of(inlet, "value"),
        [w("uniform"), list(vec![w("1"), w("0"), w("0")])]
    );
    assert_eq!(
        tokens_of(&d.root, "nested"),
        [list(vec![
            list(vec![list(vec![w("0"), w("0")]), list(vec![w("1"), w("1")])]),
            list(vec![w("2"), w("2")]),
        ])]
    );
    assert_eq!(tokens_of(&d.root, "empty"), [list(vec![])]);
    // Round brackets that follow word characters belong to the word.
    assert_eq!(tokens_of(&d.root, "div(phi,U)"), [w("Gauss"), w("linear")]);
}

#[test]
fn a_list_may_hold_named_blocks() {
    let text = "sets ( vertical { type uniform; nPoints 64; } horizontal { type uniform; } );";
    let d = doc(text);
    let Some(Token::List(items)) = tokens_of(&d.root, "sets").first() else {
        panic!("sets is a list");
    };
    assert_eq!(items.len(), 4);
    assert_eq!(items[0], w("vertical"));
    let Token::Dict(first) = &items[1] else {
        panic!("the second item is a block");
    };
    assert_eq!(first.get_number("nPoints"), Some(64.0));
    assert_eq!(items[2], w("horizontal"));
}

#[test]
fn regex_keys_and_macros_are_kept_not_expanded() {
    let text = "\"(inlet|outlet)\" { type fixedValue; value $U; }\n$common;";
    let d = doc(text);
    let block = d.dict("(inlet|outlet)").expect("the quoted key is a key");
    assert_eq!(tokens_of(block, "value"), [w("$U")]);
    assert_eq!(
        d.root.entries()[1],
        Entry::Macro {
            name: "common".to_string(),
            line: 2
        }
    );
}

#[test]
fn directives_are_listed_and_never_followed() {
    let text = [
        r#"#include "x""#,
        r#"#includeEtc "caseDicts/setConstraintTypes""#,
        "functions",
        "{",
        "    #includeFunc streamlinesLine",
        "    #includeFunc writeObjects(kEpsilon:G)",
        "}",
        r#"width #calc "0.1 * 2";"#,
        "vertices #codeStream",
        "{",
        "    code #{ int x = 1; { #};",
        "};",
        "after 1;",
    ]
    .join("\n");
    let d = doc(&text);
    let found: Vec<(String, u32, bool, Vec<String>)> = d
        .directives()
        .into_iter()
        .map(|x| (x.word, x.line, x.has_body, x.args))
        .collect();
    let expect = |word: &str, line: u32, has_body: bool, args: &[&str]| {
        (
            word.to_string(),
            line,
            has_body,
            args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        found,
        vec![
            expect("#include", 1, false, &[r#""x""#]),
            expect(
                "#includeEtc",
                2,
                false,
                &[r#""caseDicts/setConstraintTypes""#]
            ),
            expect("#includeFunc", 5, false, &["streamlinesLine"]),
            expect("#includeFunc", 6, false, &["writeObjects(kEpsilon:G)"]),
            expect("#calc", 8, false, &[r#""0.1 * 2""#]),
            expect("#codeStream", 9, true, &[]),
        ]
    );
    assert_eq!(d.get_number("after"), Some(1.0));
    let first = &d.directives()[0];
    assert_eq!(first.arg_text, r#""x""#);
}

#[test]
fn an_entry_directive_with_a_body_skips_the_body() {
    let text = "#codeStream { code #{ { #}; nested { deep 1; } }\nafter 2;";
    let d = doc(text);
    let found = d.directives();
    assert_eq!(found.len(), 1);
    assert!(found[0].has_body);
    assert_eq!(d.root.entries().len(), 2);
    assert_eq!(d.get_number("after"), Some(2.0));
}

#[test]
fn caps_refuse_before_work() {
    let small = Limits {
        max_bytes: 10,
        ..Limits::default()
    };
    assert_eq!(
        parse("a 1; b 2; c 3;", &small),
        Err(ParseError::TooLarge { size: 14, max: 10 })
    );

    let nested = |levels: usize| format!("{}{}", "a{".repeat(levels), "}".repeat(levels));
    assert!(parse(&nested(64), &Limits::default()).is_ok());
    assert_eq!(
        parse(&nested(65), &Limits::default()),
        Err(ParseError::TooDeep { max: 64 })
    );

    let few = Limits {
        max_tokens: 3,
        ..Limits::default()
    };
    assert_eq!(
        parse("a 1; b 2;", &few),
        Err(ParseError::TooManyTokens { max: 3 })
    );
}

#[test]
fn malformed_text_is_a_named_error() {
    let limits = Limits::default();
    assert_eq!(
        parse("a 1", &limits),
        Err(ParseError::Unterminated { line: 1 })
    );
    assert_eq!(
        parse("a (1 2;", &limits),
        Err(ParseError::Unexpected {
            line: 1,
            what: ";".to_string()
        })
    );
    assert_eq!(
        parse("a 1;\n}", &limits),
        Err(ParseError::Unexpected {
            line: 2,
            what: "}".to_string()
        })
    );
    assert_eq!(
        parse("a \"never closed;", &limits),
        Err(ParseError::Unterminated { line: 1 })
    );
    assert_eq!(
        parse("a 1; /* never closed", &limits),
        Err(ParseError::Unterminated { line: 1 })
    );
}
