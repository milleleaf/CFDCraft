use cfdcraft_dictedit::{get_entry, set_entry};

/// The solution dictionary of a closed domain, in the layout the case writer emits.
const FV_SOLUTION: &str = r"FoamFile
{
    version 2.0;
    format ascii;
    class dictionary;
    object fvSolution;
}

solvers
{
    p
    {
        solver PCG;
        preconditioner DIC;
        tolerance 1e-10;
        relTol 0;
    }
    U
    {
        solver smoothSolver;
        smoother symGaussSeidel;
        tolerance 1e-10;
        relTol 0;
    }
}
SIMPLE
{
    nNonOrthogonalCorrectors 0;
    pRefCell 0;
    pRefValue 0;
    consistent no;
}
relaxationFactors
{
    fields { p 0.3; }
    equations { U 0.7; }
}
";

const FV_SCHEMES: &str = r"FoamFile
{
    version 2.0;
    format ascii;
    class dictionary;
    object fvSchemes;
}

ddtSchemes { default steadyState; }
gradSchemes { default Gauss linear; }
divSchemes
{
    default none;
    div(phi,U) Gauss linear;
    div((nuEff*dev2(T(grad(U))))) Gauss linear;
}
laplacianSchemes { default Gauss linear orthogonal; }
interpolationSchemes { default linear; }
snGradSchemes { default orthogonal; }
fluxRequired { default no; p; }
";

const CONTROL_DICT: &str = r"FoamFile
{
    version 2.0;
    format ascii;
    class dictionary;
    object controlDict;
}

application simpleFoam;
startFrom startTime;
startTime 0;
endTime 500;
deltaT 1;
writeControl timeStep;

functions
{
    centreline
    {
        type sets;
        libs (sampling);
        writeControl writeTime;
        fields (U);
        sets
        (
            vertical
            {
                type uniform;
                axis y;
                nPoints 17;
            }
            horizontal
            {
                type uniform;
                axis x;
                nPoints 17;
            }
        );
    }
}
";

#[test]
fn a_value_in_a_nested_one_line_block_is_replaced() {
    let out = set_entry(FV_SOLUTION, "relaxationFactors/equations/U", "0.5").unwrap();
    let expected = FV_SOLUTION.replace("equations { U 0.7; }", "equations { U 0.5; }");
    assert_ne!(out, FV_SOLUTION);
    assert_eq!(out, expected);
}

#[test]
fn a_value_in_a_multi_line_block_is_replaced() {
    let out = set_entry(FV_SOLUTION, "solvers/p/tolerance", "1e-8").unwrap();
    // The first `tolerance 1e-10;` belongs to `p`; the one of `U` stays.
    let expected = FV_SOLUTION.replacen("tolerance 1e-10;", "tolerance 1e-8;", 1);
    assert_eq!(out, expected);
    assert_eq!(out.matches("tolerance 1e-10;").count(), 1);
    assert_eq!(out.lines().count(), FV_SOLUTION.lines().count());
}

#[test]
fn a_top_level_entry_is_replaced() {
    let out = set_entry(CONTROL_DICT, "application", "pimpleFoam").unwrap();
    assert_eq!(
        out,
        CONTROL_DICT.replace("application simpleFoam;", "application pimpleFoam;")
    );
    let out = set_entry(CONTROL_DICT, "endTime", "1000").unwrap();
    assert_eq!(out, CONTROL_DICT.replace("endTime 500;", "endTime 1000;"));
}

#[test]
fn words_with_brackets_and_bare_keywords_are_found() {
    let out = set_entry(FV_SCHEMES, "divSchemes/default", "bounded Gauss linear").unwrap();
    assert_eq!(
        out,
        FV_SCHEMES.replace("default none;", "default bounded Gauss linear;")
    );
    assert_eq!(
        get_entry(FV_SCHEMES, "laplacianSchemes/default").unwrap(),
        Some("Gauss linear orthogonal".to_owned())
    );
    assert_eq!(
        get_entry(FV_SCHEMES, "fluxRequired/p").unwrap(),
        Some(String::new())
    );
    let out = set_entry(FV_SCHEMES, "fluxRequired/p", "yes").unwrap();
    assert_eq!(
        out,
        FV_SCHEMES.replace(
            "fluxRequired { default no; p; }",
            "fluxRequired { default no; p yes; }"
        )
    );
}

#[test]
fn a_list_value_that_holds_blocks_is_one_entry() {
    let sets = get_entry(CONTROL_DICT, "functions/centreline/sets")
        .unwrap()
        .unwrap();
    assert!(sets.starts_with('('));
    assert!(sets.ends_with(')'));
    assert!(sets.contains("horizontal"));
    assert_eq!(
        get_entry(CONTROL_DICT, "functions/centreline/type").unwrap(),
        Some("sets".to_owned())
    );
    let out = set_entry(
        CONTROL_DICT,
        "functions/centreline/writeControl",
        "timeStep",
    )
    .unwrap();
    assert_eq!(
        out,
        CONTROL_DICT.replacen("writeControl writeTime;", "writeControl timeStep;", 1)
    );
}

#[test]
fn get_entry_reads_what_set_entry_writes() {
    let path = "relaxationFactors/equations/U";
    assert_eq!(
        get_entry(FV_SOLUTION, path).unwrap(),
        Some("0.7".to_owned())
    );
    assert_eq!(
        get_entry(FV_SOLUTION, "relaxationFactors/equations/V").unwrap(),
        None
    );
    let out = set_entry(FV_SOLUTION, path, "0.5").unwrap();
    assert_eq!(get_entry(&out, path).unwrap(), Some("0.5".to_owned()));
    let out = set_entry(&out, "SIMPLE/momentumPredictor", "no").unwrap();
    assert_eq!(
        get_entry(&out, "SIMPLE/momentumPredictor").unwrap(),
        Some("no".to_owned())
    );
}

#[test]
fn the_value_is_written_with_one_space_and_a_semicolon() {
    let text = "equations { U    0.7 ; }\n";
    assert_eq!(
        set_entry(text, "equations/U", "0.5").unwrap(),
        "equations { U 0.5; }\n"
    );
    let text = "equations\n{\n    U\t\t0.7\n    ;\n}\n";
    assert_eq!(
        set_entry(text, "equations/U", " 0.5 ").unwrap(),
        "equations\n{\n    U 0.5;\n}\n"
    );
}

#[test]
fn an_edit_leaves_the_header_and_other_blocks_byte_for_byte() {
    let out = set_entry(FV_SOLUTION, "relaxationFactors/equations/U", "0.5").unwrap();
    assert_eq!(out.len(), FV_SOLUTION.len());
    let differing: Vec<usize> = out
        .bytes()
        .zip(FV_SOLUTION.bytes())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    let at = FV_SOLUTION.find("U 0.7;").unwrap() + 4;
    assert_eq!(differing, vec![at]);
}

#[test]
fn a_missing_entry_is_added_to_a_multi_line_block() {
    let out = set_entry(FV_SOLUTION, "SIMPLE/momentumPredictor", "no").unwrap();
    let expected = FV_SOLUTION.replace(
        "    consistent no;\n}\n",
        "    consistent no;\n    momentumPredictor no;\n}\n",
    );
    assert_eq!(out, expected);
}

#[test]
fn a_missing_entry_is_added_to_an_empty_multi_line_block() {
    let text = "outer\n{\n    inner\n    {\n    }\n}\n";
    assert_eq!(
        set_entry(text, "outer/inner/x", "1").unwrap(),
        "outer\n{\n    inner\n    {\n        x 1;\n    }\n}\n"
    );
}

#[test]
fn a_missing_entry_is_added_to_a_one_line_block() {
    let out = set_entry(FV_SOLUTION, "relaxationFactors/fields/k", "0.5").unwrap();
    let expected = FV_SOLUTION.replace("fields { p 0.3; }", "fields { p 0.3; k 0.5; }");
    assert_eq!(out, expected);
    assert_eq!(set_entry("a { }\n", "a/b", "1").unwrap(), "a { b 1; }\n");
}

#[test]
fn a_missing_top_level_entry_is_appended() {
    let out = set_entry(CONTROL_DICT, "purgeWrite", "0").unwrap();
    assert_eq!(out, format!("{CONTROL_DICT}purgeWrite 0;\n"));
    assert_eq!(set_entry("a 1;", "b", "2").unwrap(), "a 1;\nb 2;\n");
    assert_eq!(set_entry("", "b", "2").unwrap(), "b 2;\n");
}

#[test]
fn comments_and_strings_do_not_confuse_the_search() {
    let text = r#"// U 9;
/* equations { U 1; } */
note "} U 2;";
relaxationFactors
{
    // U 8;
    equations { U 0.7; /* U 7; */ }
}
"#;
    let out = set_entry(text, "relaxationFactors/equations/U", "0.5").unwrap();
    assert_eq!(out, text.replace("{ U 0.7;", "{ U 0.5;"));
    assert!(out.contains("// U 9;"));
    assert!(out.contains("/* equations { U 1; } */"));
    assert!(out.contains(r#"note "} U 2;";"#));
    assert_eq!(
        get_entry(text, "note").unwrap(),
        Some(r#""} U 2;""#.to_owned())
    );
}

#[test]
fn a_quoted_keyword_in_the_text_is_skipped_not_matched() {
    let text = r#"relaxationFactors
{
    fields
    {
        "(U|k)" { p 0.3; }
        "p.*" 0.9;
    }
    equations { U 0.7; }
}
"#;
    let out = set_entry(text, "relaxationFactors/equations/U", "0.5").unwrap();
    assert_eq!(out, text.replace("{ U 0.7;", "{ U 0.5;"));
    assert_eq!(get_entry(text, "relaxationFactors/fields/p").unwrap(), None);
    let out = set_entry(text, "relaxationFactors/fields/k", "0.4").unwrap();
    assert_eq!(
        out,
        text.replace(
            "        \"p.*\" 0.9;\n",
            "        \"p.*\" 0.9;\n        k 0.4;\n"
        )
    );
}
