//! The safety scan of a written case.
//!
//! The solver compiles any formula typed into its setup and runs any user source, user script or
//! shared library placed in a case. The scan therefore allows exactly four files and refuses a
//! setup that holds a formula or a function in any element name, attribute or text.

use crate::{
    setup::{MESH_FILE, SETUP_FILE},
    xml::{self, Element},
};
use cfdcraft_backend::{CaseTree, Refusal};

/// The code of the refusal of a setup that holds a forbidden node.
pub const FORBIDDEN_NODE: &str = "CFD-GUARD-012";

/// The code of the refusal of a case that holds a forbidden file.
pub const FORBIDDEN_FILE: &str = "CFD-GUARD-013";

/// The files a case may hold, as `/`-separated paths from the case root.
pub const ALLOWED_FILES: [&str; 4] = [SETUP_FILE, MESH_FILE, "Allrun", "README.md"];

/// Words that no element name, attribute name, attribute value or text of a setup may contain, in
/// any letter case. Each stands for a way to run code: a formula, its interpreter, a computed
/// value, a user routine or a function.
pub const FORBIDDEN_WORDS: [&str; 5] = ["formula", "mei", "compute", "user", "function"];

/// The longest piece of text a refusal quotes.
const SHOWN: usize = 60;

fn shown(text: &str) -> String {
    text.chars().take(SHOWN).collect()
}

fn forbidden_word(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    FORBIDDEN_WORDS
        .into_iter()
        .find(|word| lower.contains(word))
}

fn node_refusal(line: u32, entry: &str, message: String) -> Refusal {
    Refusal {
        code: FORBIDDEN_NODE,
        file: SETUP_FILE.to_owned(),
        line,
        entry: shown(entry),
        message,
    }
}

fn scan_element(element: &Element, found: &mut Vec<Refusal>) {
    let line = element.line;
    if let Some(word) = forbidden_word(&element.name) {
        found.push(node_refusal(
            line,
            &element.name,
            format!("The setup has an element whose name contains \"{word}\", which can run code."),
        ));
    }
    for (key, value) in &element.attributes {
        if let Some(word) = forbidden_word(key) {
            found.push(node_refusal(
                line,
                key,
                format!(
                    "The setup has an attribute whose name contains \"{word}\", which can run code."
                ),
            ));
        } else if let Some(word) = forbidden_word(value) {
            found.push(node_refusal(
                line,
                value,
                format!(
                    "The setup has an attribute value that contains \"{word}\", which can run code."
                ),
            ));
        }
    }
    if let Some(word) = forbidden_word(&element.text) {
        found.push(node_refusal(
            line,
            &element.text,
            format!("The setup has text that contains \"{word}\", which can run code."),
        ));
    } else if element.text.contains([';', '=']) {
        found.push(node_refusal(
            line,
            &element.text,
            "The setup has text that reads as an expression; only plain values are allowed."
                .to_owned(),
        ));
    }
}

fn scan_setup(bytes: &[u8], found: &mut Vec<Refusal>) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        found.push(node_refusal(
            1,
            SETUP_FILE,
            "The setup is not valid text, so it cannot be checked.".to_owned(),
        ));
        return;
    };
    match xml::parse(text) {
        Ok(root) => root.walk(&mut |element| scan_element(element, found)),
        Err(error) => found.push(node_refusal(
            1,
            SETUP_FILE,
            format!("The setup cannot be checked: {error}."),
        )),
    }
}

/// The path of a file as `/`-separated names from the case root.
pub(crate) fn rel_name(file: &cfdcraft_backend::CaseFile) -> String {
    file.rel_path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_message(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.starts_with("src/") {
        "A user source file would be compiled into the solver, so it is refused."
    } else if lower.ends_with(".py") {
        "A script placed in a case is run by the solver, so it is refused."
    } else if [".so", ".dylib", ".dll"]
        .iter()
        .any(|end| lower.ends_with(end))
    {
        "A shared library placed in a case is loaded by the solver, so it is refused."
    } else {
        "This file is not one of the four files a case may hold."
    }
}

/// Every construct of the written case the scan refuses; empty when the case is clean.
pub fn scan_tree(tree: &CaseTree) -> Vec<Refusal> {
    let mut found = Vec::new();
    for file in &tree.files {
        let name = rel_name(file);
        if !ALLOWED_FILES.contains(&name.as_str()) {
            found.push(Refusal {
                code: FORBIDDEN_FILE,
                file: name.clone(),
                line: 1,
                entry: shown(&name),
                message: file_message(&name).to_owned(),
            });
        } else if name == SETUP_FILE {
            scan_setup(&file.bytes, &mut found);
        }
    }
    found
}
