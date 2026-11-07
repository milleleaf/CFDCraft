//! Looking through a case folder: which files would be copied, which are left out, and what the
//! case's dictionaries say about it. Nothing here follows a link, opens anything that is not a
//! regular file the walk listed, or runs anything.

use crate::record::{ImportRefusal, Inventory, TimeSettings};
use crate::{ImportError, ImportOptions, source_err};
use cfdcraft_foamdict::{Document, Entry, Limits, parse};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest dictionary the inventory parses; a bigger one is listed as unparsed.
const PARSE_BYTES: u64 = 4 * 1024 * 1024;
/// Most bytes of dictionary text kept for the copy step so a file is read only once.
const CACHE_BYTES: usize = 64 * 1024 * 1024;
/// Most dictionaries looked at for a header when the case has no control dictionary.
const HEADER_PROBES: usize = 50;

/// What the dialect detection looks at; the first row that fits wins. Data, not branches.
struct DialectRow {
    id: &'static str,
    /// The control dictionary entry that names the solver.
    key: &'static str,
    /// The constant file that holds the fluid properties.
    property: &'static str,
    /// The constant file that holds the turbulence settings.
    turbulence: &'static str,
}

const DIALECTS: [DialectRow; 3] = [
    DialectRow {
        id: "foundation-14",
        key: "solver",
        property: "constant/physicalProperties",
        turbulence: "constant/momentumTransport",
    },
    DialectRow {
        id: "foundation-legacy",
        key: "application",
        property: "constant/transportProperties",
        turbulence: "constant/momentumTransport",
    },
    DialectRow {
        id: "esi",
        key: "application",
        property: "constant/transportProperties",
        turbulence: "constant/turbulenceProperties",
    },
];

/// One file that will be copied.
pub(crate) struct Planned {
    pub path: PathBuf,
    pub size: u64,
}

/// The result of walking the source.
#[derive(Default)]
pub(crate) struct Walk {
    /// Files to copy, by path below the source, sorted.
    pub files: BTreeMap<String, Planned>,
    /// Files left out, counted by kind.
    pub skipped: BTreeMap<String, u64>,
    /// Entries whose names cannot be stored safely.
    pub refusals: Vec<ImportRefusal>,
}

/// Dictionaries kept in memory after the inventory read them.
pub(crate) type Cache = BTreeMap<String, Vec<u8>>;

/// A script by name: it is copied inert into the original and left out of the working copy.
pub(crate) fn is_script(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.starts_with("Allrun")
        || name == "Allclean"
        || [".sh", ".py", ".bash"].iter().any(|e| name.ends_with(e))
}

/// Why a top-level folder holds results and is left out, if it does.
pub(crate) fn dir_skip_kind(name: &str) -> Option<&'static str> {
    let numeric = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '-' | '+'))
        && name.parse::<f64>().is_ok();
    if numeric && name != "0" {
        Some("time")
    } else if name.starts_with("processor") {
        Some("processor")
    } else if name == "postProcessing" {
        Some("post_processing")
    } else {
        None
    }
}

/// Whether a top-level entry is one the walk leaves out (a result folder, a log file), which the
/// guard scan then does not read. It is the same rule `visit` applies, so the two cannot differ.
pub(crate) fn is_left_out(name: &str, is_folder: bool) -> bool {
    if is_folder {
        dir_skip_kind(name).is_some()
    } else {
        name.starts_with("log.")
    }
}

/// Walks the source without following links, counting what is skipped and stopping at the caps.
/// Links and special files are not listed: the guard scan refuses them.
pub(crate) fn walk(source: &Path, options: &ImportOptions) -> Result<Walk, ImportError> {
    let mut found = Walk::default();
    let mut total = 0u64;
    visit(source, "", 0, None, options, &mut total, &mut found)?;
    Ok(found)
}

fn visit(
    dir: &Path,
    rel: &str,
    depth: usize,
    skip: Option<&'static str>,
    options: &ImportOptions,
    total: &mut u64,
    found: &mut Walk,
) -> Result<(), ImportError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(source_err)? {
        names.push(entry.map_err(source_err)?.file_name());
    }
    names.sort();
    for name in names {
        let Some(n) = name.to_str() else { continue };
        let child = if rel.is_empty() {
            n.to_string()
        } else {
            format!("{rel}/{n}")
        };
        let path = dir.join(&name);
        let meta = fs::symlink_metadata(&path).map_err(source_err)?;
        let kind = meta.file_type();
        if kind.is_dir() {
            if depth >= options.max_depth {
                return Err(ImportError::Cap {
                    what: "depth",
                    limit: options.max_depth as u64,
                    at: child,
                });
            }
            let next = skip.or_else(|| {
                if rel.is_empty() {
                    dir_skip_kind(n)
                } else {
                    None
                }
            });
            visit(&path, &child, depth + 1, next, options, total, found)?;
        } else if kind.is_file() {
            let left_out =
                skip.or_else(|| (rel.is_empty() && n.starts_with("log.")).then_some("log"));
            if let Some(why) = left_out {
                *found.skipped.entry(why.to_string()).or_insert(0) += 1;
                continue;
            }
            let size = meta.len();
            if size > options.max_file_bytes {
                return Err(ImportError::Cap {
                    what: "file_bytes",
                    limit: options.max_file_bytes,
                    at: child,
                });
            }
            *total = total.saturating_add(size);
            if *total > options.max_total_bytes {
                return Err(ImportError::Cap {
                    what: "total_bytes",
                    limit: options.max_total_bytes,
                    at: child,
                });
            }
            if found.files.len() >= options.max_files {
                return Err(ImportError::Cap {
                    what: "files",
                    limit: options.max_files as u64,
                    at: child,
                });
            }
            if craft_docpath::resolve(&child, None).is_err() {
                found.refusals.push(ImportRefusal {
                    code: "CFD-GUARD-007".to_string(),
                    file: child,
                    line: 1,
                    entry: n.chars().take(80).collect(),
                    message: "this file or path is not allowed in a case".to_string(),
                });
                continue;
            }
            found.files.insert(child, Planned { path, size });
        }
    }
    Ok(())
}

/// Opens a regular file; a link, a folder or a special file is refused.
fn open_regular(path: &Path) -> std::io::Result<fs::File> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    fs::File::open(path)
}

/// Reads a whole regular file and stops at `max` bytes with the cap error.
pub(crate) fn read_capped(path: &Path, rel: &str, max: u64) -> Result<Vec<u8>, ImportError> {
    let mut bytes = Vec::new();
    open_regular(path)
        .and_then(|f| f.take(max.saturating_add(1)).read_to_end(&mut bytes))
        .map_err(source_err)?;
    if bytes.len() as u64 > max {
        return Err(ImportError::Cap {
            what: "file_bytes",
            limit: max,
            at: rel.to_string(),
        });
    }
    Ok(bytes)
}

/// A file one level below a field folder.
fn field_file(rel: &str) -> bool {
    rel.split_once('/')
        .is_some_and(|(top, rest)| matches!(top, "0" | "0.orig") && !rest.contains('/'))
}

/// A file one level below a dictionary folder.
fn dictionary_file(rel: &str) -> bool {
    rel.split_once('/').is_some_and(|(top, rest)| {
        matches!(top, "system" | "constant" | "0" | "0.orig") && !rest.contains('/')
    })
}

fn has_header(path: &Path) -> bool {
    let mut head = Vec::new();
    open_regular(path)
        .and_then(|f| f.take(4096).read_to_end(&mut head))
        .is_ok()
        && head.windows(8).any(|w| w == b"FoamFile")
}

/// Whether the folder is an OpenFOAM case: a control dictionary, or a dictionary with a header.
pub(crate) fn is_case(found: &Walk) -> bool {
    found.files.contains_key("system/controlDict")
        || found
            .files
            .iter()
            .filter(|(rel, _)| dictionary_file(rel))
            .take(HEADER_PROBES)
            .any(|(_, p)| has_header(&p.path))
}

fn word(doc: Option<&Document>, key: &str) -> Option<String> {
    doc?.get_word(key).map(str::to_string)
}

fn function_objects(control: &Document) -> Vec<String> {
    let mut names = BTreeSet::new();
    if let Some(functions) = control.dict("functions") {
        for entry in functions.entries() {
            if let Entry::Block { key, .. } = entry {
                names.insert(key.clone());
            }
        }
    }
    for directive in control.directives() {
        if directive.word == "#includeFunc"
            && let Some(first) = directive.args.first()
        {
            let name = first
                .split('(')
                .next()
                .unwrap_or_default()
                .trim_matches('"');
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        }
    }
    names.into_iter().collect()
}

fn mesh_kind(found: &Walk) -> &'static str {
    if found.files.contains_key("system/snappyHexMeshDict") {
        "snappyHexMesh"
    } else if found.files.contains_key("system/blockMeshDict") {
        "blockMesh"
    } else if found
        .files
        .keys()
        .any(|r| r.starts_with("constant/polyMesh/"))
    {
        "polyMesh"
    } else {
        "unknown"
    }
}

/// Reads the dictionaries the inventory needs, once each, and builds the inventory. A dictionary
/// that cannot be read as text, is over the parse size or does not parse is listed as unparsed.
pub(crate) fn load(found: &Walk) -> Result<(Cache, Inventory), ImportError> {
    let limits = Limits {
        max_bytes: PARSE_BYTES as usize,
        ..Limits::default()
    };
    let mut cache = Cache::new();
    let mut cached = 0usize;
    let mut docs: BTreeMap<&str, Document> = BTreeMap::new();
    let mut unparsed = BTreeSet::new();
    for (rel, planned) in &found.files {
        let wanted = matches!(
            rel.as_str(),
            "system/controlDict" | "constant/momentumTransport" | "constant/turbulenceProperties"
        ) || field_file(rel);
        if !wanted {
            continue;
        }
        if planned.size > PARSE_BYTES {
            unparsed.insert(rel.clone());
            continue;
        }
        let bytes = read_capped(&planned.path, rel, PARSE_BYTES)?;
        match std::str::from_utf8(&bytes).map(|t| parse(t, &limits)) {
            Ok(Ok(doc)) => {
                docs.insert(rel.as_str(), doc);
            }
            _ => {
                unparsed.insert(rel.clone());
            }
        }
        if cached + bytes.len() <= CACHE_BYTES {
            cached += bytes.len();
            cache.insert(rel.clone(), bytes);
        }
    }

    let control = docs.get("system/controlDict");
    let turbulence = docs
        .get("constant/momentumTransport")
        .or_else(|| docs.get("constant/turbulenceProperties"));
    let dialect = control
        .and_then(|c| {
            DIALECTS.iter().find(|d| {
                c.get(d.key).is_some()
                    && found.files.contains_key(d.property)
                    && found.files.contains_key(d.turbulence)
            })
        })
        .map_or("unknown", |d| d.id);
    let simulation = word(turbulence, "simulationType");
    let turbulence_detail = simulation.as_deref().and_then(|s| {
        let block = turbulence?.dict(s)?;
        ["model", "RASModel", "LESModel"]
            .iter()
            .find_map(|k| block.get_word(k))
            .map(str::to_string)
    });
    let mut kinds = BTreeSet::new();
    for (rel, doc) in &docs {
        if !field_file(rel) {
            continue;
        }
        for entry in doc.dict("boundaryField").map_or(&[][..], |b| b.entries()) {
            if let Entry::Block { dict, .. } = entry
                && let Some(kind) = dict.get_word("type")
            {
                kinds.insert(kind.to_string());
            }
        }
    }
    let inventory = Inventory {
        dialect: dialect.to_string(),
        application_or_solver: word(control, "solver").or_else(|| word(control, "application")),
        turbulence_model: simulation,
        turbulence_detail,
        mesh_kind: mesh_kind(found).to_string(),
        boundary_condition_kinds: kinds.into_iter().collect(),
        time_settings: TimeSettings {
            start_from: word(control, "startFrom"),
            start_time: word(control, "startTime"),
            stop_at: word(control, "stopAt"),
            end_time: word(control, "endTime"),
            delta_t: word(control, "deltaT"),
            write_control: word(control, "writeControl"),
            write_interval: word(control, "writeInterval"),
        },
        function_objects: control.map(function_objects).unwrap_or_default(),
        unparsed: unparsed.into_iter().collect(),
    };
    Ok((cache, inventory))
}
