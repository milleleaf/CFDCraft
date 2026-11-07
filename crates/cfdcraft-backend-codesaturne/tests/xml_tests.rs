use cfdcraft_backend_codesaturne::xml::{MAX_BYTES, MAX_DEPTH, XmlError, parse};

#[test]
fn the_subset_reader_reads_elements_attributes_and_text() {
    let root = parse("<?xml version=\"1.0\"?>\n<!-- first -->\n<a x=\"1\" y='two'><b>t</b><!-- inside --><c/><b>u</b></a>\n").unwrap();
    assert_eq!(root.name, "a");
    assert_eq!(root.attr("x"), Some("1"));
    assert_eq!(root.attr("y"), Some("two"));
    assert_eq!(root.attr("z"), None);
    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["b", "c", "b"]);
    assert_eq!(root.children[0].text, "t");
    assert_eq!(root.children[2].text, "u");
    assert!(root.children[1].children.is_empty());
    assert_eq!(root.children_named("b").count(), 2);
    assert_eq!(root.find(&["c"]).map(|c| c.name.as_str()), Some("c"));
}

#[test]
fn the_five_predefined_entities_are_resolved_and_lines_are_counted() {
    let root = parse("<a v=\"&lt;&amp;&quot;\">\n  &gt; &apos;x&apos;\n  <b/></a>").unwrap();
    assert_eq!(root.attr("v"), Some("<&\""));
    assert_eq!(root.text, "> 'x'");
    assert_eq!(root.line, 1);
    assert_eq!(root.children[0].line, 3);
}

#[test]
fn the_reader_refuses_what_a_setup_never_needs() {
    let doctype = parse("<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>").unwrap_err();
    assert!(
        matches!(doctype, XmlError::Doctype { line: 1 }),
        "{doctype:?}"
    );

    let entity = parse("<a>&e;</a>").unwrap_err();
    assert!(
        matches!(&entity, XmlError::Entity { name, .. } if name == "e"),
        "{entity:?}"
    );
    let numeric = parse("<a>&#65;</a>").unwrap_err();
    assert!(matches!(numeric, XmlError::Entity { .. }), "{numeric:?}");

    let instruction = parse("<?xml version=\"1.0\"?><?run now?><a/>").unwrap_err();
    assert!(
        matches!(instruction, XmlError::ProcessingInstruction { .. }),
        "{instruction:?}"
    );
    let inside = parse("<a><?run now?></a>").unwrap_err();
    assert!(
        matches!(inside, XmlError::ProcessingInstruction { .. }),
        "{inside:?}"
    );

    let cdata = parse("<a><![CDATA[x]]></a>").unwrap_err();
    assert!(matches!(cdata, XmlError::CData { .. }), "{cdata:?}");

    let at_limit = format!("{}x{}", "<a>".repeat(MAX_DEPTH), "</a>".repeat(MAX_DEPTH));
    assert!(parse(&at_limit).is_ok());
    let too_deep = format!(
        "{}x{}",
        "<a>".repeat(MAX_DEPTH + 1),
        "</a>".repeat(MAX_DEPTH + 1)
    );
    let deep = parse(&too_deep).unwrap_err();
    assert!(matches!(deep, XmlError::TooDeep { .. }), "{deep:?}");

    let long = format!("<a>{}</a>", "x".repeat(MAX_BYTES + 1));
    let text = parse(&long).unwrap_err();
    assert!(matches!(text, XmlError::TextTooLong { .. }), "{text:?}");
}

#[test]
fn malformed_documents_are_errors() {
    for text in [
        "",
        "<a>",
        "<a></b>",
        "<a x=1/>",
        "<a x=\"1\" x=\"2\"/>",
        "<a/><b/>",
        "<a x=\"<\"/>",
        "<a>&amp</a>",
        "<a><!-- open</a>",
        "text",
    ] {
        let found = parse(text);
        assert!(
            matches!(found, Err(XmlError::Malformed { .. })),
            "{text:?}: {found:?}"
        );
    }
}
