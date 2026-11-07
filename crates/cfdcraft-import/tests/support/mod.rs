//! Small synthetic cases written into a folder of the test's own. Every case here is written from
//! the dictionary syntax alone; none is a copy of a real case.
// Each test file uses a different part of this module.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A folder that holds a source case and the place its import goes. It is removed on drop, after
/// the read-only marks of the stored originals are cleared.
pub struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("import-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("source")).unwrap();
        Scratch { root }
    }

    /// The folder a case is written into.
    pub fn source(&self) -> PathBuf {
        self.root.join("source")
    }

    /// The folder an import must create; it does not exist until an import makes it.
    pub fn target(&self) -> PathBuf {
        self.root.join("stored.cfdcase")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        make_writable_below(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(unix)]
pub fn make_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o644));
}

#[cfg(not(unix))]
#[allow(clippy::permissions_set_readonly_false)]
pub fn make_writable(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut permissions = meta.permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

fn make_writable_below(dir: &Path) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => make_writable_below(&path),
            Ok(m) if m.is_file() => make_writable(&path),
            _ => {}
        }
    }
}

/// Writes `text` to `root/rel`, creating the folders above it.
pub fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A dictionary file: the `FoamFile` header for `object`, then `body`.
pub fn dict(object: &str, body: &str) -> String {
    format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class dictionary;\n    object {object};\n}}\n\n{body}\n"
    )
}

pub const CONTROL_DICT: &str = "solver incompressibleFluid;
startFrom startTime;
startTime 0;
stopAt endTime;
endTime 0.5;
deltaT 0.005;
writeControl timeStep;
writeInterval 20;

functions
{
    probes
    {
        type probes;
        libs (sampling);
        fields (p U);
        probeLocations ((0.5 0.5 0.05));
    }
}";

/// A small clean case in the layout of a current Foundation release.
pub fn write_clean_case(root: &Path) {
    put(
        root,
        "system/controlDict",
        &dict("controlDict", CONTROL_DICT),
    );
    put(
        root,
        "system/fvSchemes",
        &dict("fvSchemes", "ddtSchemes\n{\n    default steadyState;\n}"),
    );
    put(
        root,
        "system/fvSolution",
        &dict(
            "fvSolution",
            "solvers\n{\n    p\n    {\n        solver PCG;\n    }\n}",
        ),
    );
    put(
        root,
        "system/blockMeshDict",
        &dict(
            "blockMeshDict",
            "convertToMeters 0.1;\nvertices ((0 0 0) (1 0 0) (1 1 0) (0 1 0) (0 0 0.1) (1 0 0.1) (1 1 0.1) (0 1 0.1));\nblocks (hex (0 1 2 3 4 5 6 7) (20 20 1) simpleGrading (1 1 1));",
        ),
    );
    put(
        root,
        "constant/physicalProperties",
        &dict("physicalProperties", "viscosityModel constant;\nnu 0.01;"),
    );
    put(
        root,
        "constant/momentumTransport",
        &dict(
            "momentumTransport",
            "simulationType RAS;\nRAS\n{\n    model kEpsilon;\n    turbulence on;\n}",
        ),
    );
    put(
        root,
        "0/U",
        &dict(
            "U",
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{\n    movingWall\n    {\n        type fixedValue;\n        value uniform (1 0 0);\n    }\n    fixedWalls\n    {\n        type noSlip;\n    }\n    frontAndBack\n    {\n        type empty;\n    }\n}",
        ),
    );
    put(
        root,
        "0/p",
        &dict(
            "p",
            "dimensions [0 2 -2 0 0 0 0];\ninternalField uniform 0;\nboundaryField\n{\n    movingWall\n    {\n        type zeroGradient;\n    }\n    fixedWalls\n    {\n        type zeroGradient;\n    }\n    frontAndBack\n    {\n        type empty;\n    }\n}",
        ),
    );
}

/// A case with only a control dictionary and the two constant files, for dialect detection.
pub fn write_dialect_case(root: &Path, solver_key: &str, constant_files: &[&str]) {
    put(
        root,
        "system/controlDict",
        &dict("controlDict", &format!("{solver_key} someSolver;")),
    );
    for name in constant_files {
        put(root, &format!("constant/{name}"), &dict(name, "a 1;"));
    }
}

/// Every regular file below `root` and its bytes, by `/` separated path.
pub fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    collect(root, "", &mut out);
    out
}

fn collect(dir: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => collect(&path, &child, out),
            Ok(m) if m.is_file() => {
                out.insert(child, fs::read(&path).unwrap());
            }
            _ => {}
        }
    }
}

/// The paths of the regular files below `root`.
pub fn file_names(root: &Path) -> BTreeSet<String> {
    snapshot(root).into_keys().collect()
}
