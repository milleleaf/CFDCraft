//! Reads a written Foundation 14 case back into facts, with the dictionary parser of the foamdict
//! crate, which shares no code with the writer or with the safety scan.
//!
//! The mesh dictionary and the field files (velocity, pressure, temperature, the turbulence inflow
//! values) and the gravity file are the same text for both OpenFOAM lines and are read by the
//! reader of the older line, which has its own parser. Everything that differs in the 14 line is
//! read here: the control dictionary, the schemes, the solver settings, the property files, the
//! thermophysical files and the function objects. The reader fails closed: a file it cannot parse or fit to the
//! shape it expects is an error, never a silently skipped entry. An entry that is simply absent is
//! an absent fact.

use crate::markers::{
    ENERGY, EQUATION_OF_STATE, FLUID_MODULE, SOLVER_MODULE, THERMO_TYPE, THERMOPHYSICAL_TRANSPORT,
};
use cfdcraft_backend_esi::reader::EsiReader;
use cfdcraft_foamdict::{Dict, Document, Entry, Limits, ParseError, Token, parse};
use cfdcraft_model::{
    BoundaryKind,
    diagnostics::kind_name,
    roundtrip::{CaseReader, EMPTY_KIND, Fact, FactValue, ReadError, SourceFile, Span},
};
use std::collections::BTreeMap;

const CONTROL: &str = "system/controlDict";
const SCHEMES: &str = "system/fvSchemes";
const SOLUTION: &str = "system/fvSolution";
const PHYSICAL: &str = "constant/physicalProperties";
const TRANSPORT: &str = "constant/momentumTransport";
const MESH: &str = "system/blockMeshDict";
const VELOCITY: &str = "0/U";

type Facts = BTreeMap<String, Fact>;

/// Reads a Foundation 14 case.
#[derive(Debug, Clone, Copy, Default)]
pub struct Of14Reader;

fn fail(path: &str, line: u64, message: impl Into<String>) -> ReadError {
    ReadError {
        file: path.to_owned(),
        line,
        message: message.into(),
    }
}

fn parse_line(error: &ParseError) -> u64 {
    match error {
        ParseError::Unterminated { line } | ParseError::Unexpected { line, .. } => u64::from(*line),
        _ => 0,
    }
}

fn number_of(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A parsed file with the bytes it came from, so a fact can point at its line.
struct File<'a> {
    path: &'a str,
    bytes: &'a [u8],
    doc: Document,
}

impl<'a> File<'a> {
    fn load(files: &'a [SourceFile], path: &'a str) -> Result<Self, ReadError> {
        let mut found = files.iter().filter(|f| f.path == path);
        let file = found
            .next()
            .ok_or_else(|| fail(path, 0, "the file is missing from the written case"))?;
        if found.next().is_some() {
            return Err(fail(path, 0, "the file appears twice in the written case"));
        }
        let text =
            std::str::from_utf8(&file.bytes).map_err(|_| fail(path, 0, "the file is not text"))?;
        let doc = parse(text, &Limits::default())
            .map_err(|e| fail(path, parse_line(&e), e.to_string()))?;
        Ok(Self {
            path,
            bytes: &file.bytes,
            doc,
        })
    }

    fn err(&self, line: u32, message: impl Into<String>) -> ReadError {
        fail(self.path, u64::from(line), message)
    }

    /// The bytes of one line, counted from 1.
    fn span(&self, line: u32) -> Span {
        let mut start = 0;
        let mut current = 1;
        for (i, byte) in self.bytes.iter().enumerate() {
            if current >= line {
                break;
            }
            if *byte == b'\n' {
                current += 1;
                start = i + 1;
            }
        }
        let end = self.bytes[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(self.bytes.len(), |at| start + at);
        Span {
            file: self.path.to_owned(),
            line: u64::from(line),
            start,
            end,
        }
    }

    fn fact(&self, value: FactValue, line: u32) -> Fact {
        Fact {
            value,
            value_spans: vec![self.span(line)],
            entry_span: Some(self.span(line)),
        }
    }

    /// The tokens of a plain entry and its line; a block under this key is an error.
    fn plain<'d>(
        &self,
        dict: &'d Dict,
        key: &str,
    ) -> Result<Option<(&'d [Token], u32)>, ReadError> {
        match dict.get(key) {
            None => Ok(None),
            Some(Entry::Value { tokens, line, .. }) => Ok(Some((tokens.as_slice(), *line))),
            Some(other) => Err(self.err(
                other.line(),
                format!("\"{key}\" should be a plain entry, not a block or a directive"),
            )),
        }
    }

    fn word<'d>(&self, dict: &'d Dict, key: &str) -> Result<Option<(&'d str, u32)>, ReadError> {
        let Some((tokens, line)) = self.plain(dict, key)? else {
            return Ok(None);
        };
        match tokens {
            [only] => only
                .as_word()
                .map(|w| Some((w, line)))
                .ok_or_else(|| self.err(line, format!("\"{key}\" should be one word"))),
            _ => Err(self.err(line, format!("\"{key}\" should be one word"))),
        }
    }

    fn number(&self, dict: &Dict, key: &str) -> Result<Option<(f64, u32)>, ReadError> {
        let Some((word, line)) = self.word(dict, key)? else {
            return Ok(None);
        };
        number_of(word)
            .map(|v| Some((v, line)))
            .ok_or_else(|| self.err(line, format!("\"{key}\" should be one number")))
    }

    fn count(&self, dict: &Dict, key: &str) -> Result<Option<(u64, u32)>, ReadError> {
        let Some((word, line)) = self.word(dict, key)? else {
            return Ok(None);
        };
        word.parse::<u64>()
            .map(|v| Some((v, line)))
            .map_err(|_| self.err(line, format!("\"{key}\" should be a whole number")))
    }

    fn block<'d>(&self, dict: &'d Dict, key: &str) -> Result<Option<(&'d Dict, u32)>, ReadError> {
        match dict.get(key) {
            None => Ok(None),
            Some(Entry::Block { dict, line, .. }) => Ok(Some((dict, *line))),
            Some(other) => Err(self.err(other.line(), format!("\"{key}\" should be a block"))),
        }
    }

    fn required_word<'d>(
        &self,
        dict: &'d Dict,
        key: &str,
        owner: u32,
    ) -> Result<(&'d str, u32), ReadError> {
        self.word(dict, key)?
            .ok_or_else(|| self.err(owner, format!("the object has no \"{key}\" entry")))
    }

    fn required_number(&self, dict: &Dict, key: &str, owner: u32) -> Result<(f64, u32), ReadError> {
        self.number(dict, key)?
            .ok_or_else(|| self.err(owner, format!("the object has no \"{key}\" entry")))
    }

    /// The numbers of a list entry such as `start (0 0 0.5);`.
    fn numbers(&self, dict: &Dict, key: &str, owner: u32, n: usize) -> Result<Vec<f64>, ReadError> {
        let (tokens, line) = self
            .plain(dict, key)?
            .ok_or_else(|| self.err(owner, format!("the object has no \"{key}\" entry")))?;
        let [Token::List(items)] = tokens else {
            return Err(self.err(line, format!("\"{key}\" should be a list of numbers")));
        };
        let values: Option<Vec<f64>> = items
            .iter()
            .map(|t| t.as_word().and_then(number_of))
            .collect();
        match values {
            Some(v) if v.len() == n => Ok(v),
            _ => Err(self.err(line, format!("\"{key}\" should list {n} numbers"))),
        }
    }

    /// The one name an entry such as `fields (U);` lists.
    fn listed_word<'d>(
        &self,
        dict: &'d Dict,
        key: &str,
        owner: u32,
    ) -> Result<(&'d str, u32), ReadError> {
        let (tokens, line) = self
            .plain(dict, key)?
            .ok_or_else(|| self.err(owner, format!("the object has no \"{key}\" entry")))?;
        let name = match tokens {
            [Token::List(items)] => match items.as_slice() {
                [one] => one.as_word().map(|w| (w, line)),
                _ => None,
            },
            _ => None,
        };
        name.ok_or_else(|| self.err(line, format!("\"{key}\" should list exactly one name")))
    }
}

/// The text a file that is not read here is replaced by when the older reader is given the shared
/// files: a header and nothing else, so it finds none of the entries it does not own.
fn stub(path: &str) -> SourceFile {
    SourceFile {
        path: path.to_owned(),
        bytes: b"FoamFile\n{\n    version 2.0;\n    format ascii;\n    class dictionary;\n    object stub;\n}\n"
            .to_vec(),
    }
}

/// The files both lines write the same way and the older reader owns: the mesh, the velocity, the
/// pressure, and the files only some studies have (`p_rgh`, the temperature, gravity and the
/// turbulence inflow values).
const SHARED: [&str; 9] = [
    MESH,
    VELOCITY,
    "0/p",
    "0/p_rgh",
    "0/T",
    "constant/g",
    "0/k",
    "0/omega",
    "0/epsilon",
];

/// The facts of the files both lines write the same way.
fn shared_facts(files: &[SourceFile]) -> Result<Facts, ReadError> {
    let mut shared: Vec<SourceFile> = files
        .iter()
        .filter(|f| SHARED.contains(&f.path.as_str()))
        .cloned()
        .collect();
    shared.extend(
        [
            "constant/transportProperties",
            "constant/turbulenceProperties",
            CONTROL,
            SOLUTION,
        ]
        .map(stub),
    );
    EsiReader.read(&shared)
}

/// The kinematic viscosity: a constant model and `nu` with or without its unit set, or, for a case
/// with heat transfer, the thermophysical description.
fn viscosity(files: &[SourceFile], facts: &mut Facts) -> Result<(), ReadError> {
    let f = File::load(files, PHYSICAL)?;
    let root = &f.doc.root;
    if root.get("thermoType").is_some() {
        return thermo_facts(&f, facts);
    }
    if let Some((model, line)) = f.word(root, "viscosityModel")?
        && model != "constant"
    {
        return Err(f.err(line, "the viscosity model should be constant"));
    }
    let Some((tokens, line)) = f.plain(root, "nu")? else {
        return Ok(());
    };
    let rest: Vec<&Token> = tokens
        .iter()
        .filter(|t| !matches!(t, Token::Dims(_)))
        .collect();
    let value = match rest.as_slice() {
        [one] => one.as_word().and_then(number_of),
        _ => None,
    };
    let value =
        value.ok_or_else(|| f.err(line, "\"nu\" should be a number and an optional unit"))?;
    facts.insert(
        "/fluid/kinematic_viscosity".into(),
        f.fact(FactValue::Number(value), line),
    );
    Ok(())
}

fn required_block<'d>(
    f: &File<'_>,
    dict: &'d Dict,
    key: &str,
    owner: u32,
) -> Result<(&'d Dict, u32), ReadError> {
    f.block(dict, key)?
        .ok_or_else(|| f.err(owner, format!("the file has no \"{key}\" block")))
}

/// The thermophysical description of the heated cavity: the thermodynamic type, the equation of
/// state and the energy variable must be the ones the solver module is written for, and the
/// density, expansion coefficient, reference temperature, viscosity and Prandtl number are the
/// study's facts. The kinematic viscosity is the dynamic one over the reference density. The heat
/// capacity and the molecular weight are free choices and are not facts of the study.
fn thermo_facts(f: &File<'_>, facts: &mut Facts) -> Result<(), ReadError> {
    let root = &f.doc.root;
    let (thermo, owner) = required_block(f, root, "thermoType", 0)?;
    for (key, expected) in [
        ("type", THERMO_TYPE),
        ("equationOfState", EQUATION_OF_STATE),
        ("energy", ENERGY),
    ] {
        let (word, line) = f.required_word(thermo, key, owner)?;
        if word != expected {
            return Err(f.err(line, format!("\"{key}\" should be {expected}")));
        }
    }
    let (eos, eos_owner) = required_block(f, root, "equationOfState", owner)?;
    let (rho0, rho_line) = f.required_number(eos, "rho0", eos_owner)?;
    let (t0, t0_line) = f.required_number(eos, "T0", eos_owner)?;
    let (beta, beta_line) = f.required_number(eos, "beta", eos_owner)?;
    let (transport, transport_owner) = required_block(f, root, "transport", owner)?;
    let (mu, mu_line) = f.required_number(transport, "mu", transport_owner)?;
    let (pr, pr_line) = f.required_number(transport, "Pr", transport_owner)?;
    if rho0 <= 0.0 {
        return Err(f.err(rho_line, "\"rho0\" should be above zero"));
    }
    for (key, value, line) in [
        ("/fluid/kinematic_viscosity", mu / rho0, mu_line),
        ("/physics/buoyancy/beta", beta, beta_line),
        ("/physics/buoyancy/reference_temperature", t0, t0_line),
        ("/physics/buoyancy/prandtl", pr, pr_line),
    ] {
        facts.insert(key.into(), f.fact(FactValue::Number(value), line));
    }
    Ok(())
}

/// The turbulent Prandtl number of the heat transfer, from the eddy diffusivity block. A case
/// without the file has no heat transfer.
fn thermophysical(files: &[SourceFile], facts: &mut Facts) -> Result<(), ReadError> {
    if !files.iter().any(|f| f.path == THERMOPHYSICAL_TRANSPORT) {
        return Ok(());
    }
    let f = File::load(files, THERMOPHYSICAL_TRANSPORT)?;
    let (ras, owner) = required_block(&f, &f.doc.root, "RAS", 0)?;
    let (prt, line) = f.required_number(ras, "Prt", owner)?;
    facts.insert(
        "/physics/buoyancy/turbulent_prandtl".into(),
        f.fact(FactValue::Number(prt), line),
    );
    Ok(())
}

/// The turbulence selection: `laminar`, or the model named inside a `RAS` block.
fn transport(files: &[SourceFile], facts: &mut Facts) -> Result<(), ReadError> {
    let f = File::load(files, TRANSPORT)?;
    let root = &f.doc.root;
    let Some((kind, line)) = f.word(root, "simulationType")? else {
        return Ok(());
    };
    let (name, at) = if kind == "RAS" {
        let (ras, owner) = f
            .block(root, "RAS")?
            .ok_or_else(|| f.err(line, "a RAS case has no \"RAS\" block"))?;
        let (model, model_line) = f.required_word(ras, "model", owner)?;
        (model.to_owned(), model_line)
    } else {
        (kind.to_owned(), line)
    };
    facts.insert(
        "/solver/turbulence".into(),
        f.fact(FactValue::Text(name), at),
    );
    Ok(())
}

/// Whether the time scheme is steady, from `ddtSchemes`.
fn is_steady(files: &[SourceFile]) -> Result<bool, ReadError> {
    let f = File::load(files, SCHEMES)?;
    let Some((ddt, owner)) = f.block(&f.doc.root, "ddtSchemes")? else {
        return Ok(false);
    };
    let (word, _) = f.required_word(ddt, "default", owner)?;
    Ok(word == "steadyState")
}

/// The solver module and its steadiness, written in the vocabulary of the study (`simpleFoam` for a
/// steady run of the incompressible module, `pimpleFoam` for a time-marching one, the two
/// `buoyantBoussinesq` words for the heat transfer module), the run times and the function
/// objects.
fn control(files: &[SourceFile], facts: &mut Facts) -> Result<(), ReadError> {
    let f = File::load(files, CONTROL)?;
    let root = &f.doc.root;
    let steady = is_steady(files)?;
    if let Some((module, line)) = f.word(root, "solver")? {
        let word = match (module, steady) {
            (SOLVER_MODULE, true) => "simpleFoam",
            (SOLVER_MODULE, false) => "pimpleFoam",
            (FLUID_MODULE, true) => "buoyantBoussinesqSimpleFoam",
            (FLUID_MODULE, false) => "buoyantBoussinesqPimpleFoam",
            (other, _) => other,
        };
        facts.insert(
            "/solver/application".into(),
            f.fact(FactValue::Text(word.to_owned()), line),
        );
    }
    for (key, name) in [
        ("endTime", "/solver/end_time"),
        ("writeInterval", "/solver/write_interval"),
    ] {
        if let Some((value, line)) = f.number(root, key)? {
            facts.insert(name.into(), f.fact(FactValue::Number(value), line));
        }
    }
    if !steady {
        if let Some((value, line)) = f.number(root, "deltaT")? {
            facts.insert(
                "/solver/time/delta_t".into(),
                f.fact(FactValue::Number(value), line),
            );
        }
        if let Some((word, line)) = f.word(root, "writeControl")? {
            facts.insert(
                "/solver/time/write_control".into(),
                f.fact(FactValue::Text(word.to_owned()), line),
            );
        }
    }
    let Some((functions, _)) = f.block(root, "functions")? else {
        return Ok(());
    };
    if let Some((centreline, owner)) = f.block(functions, "centreline")? {
        centreline_facts(&f, centreline, owner, facts)?;
    }
    monitor_facts(&f, files, functions, facts)
}

/// The two sampled profiles of the `centreline` object.
fn centreline_facts(
    f: &File<'_>,
    centreline: &Dict,
    owner: u32,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let Some((sets, _)) = f.block(centreline, "sets")? else {
        return Err(f.err(owner, "the centreline object has no \"sets\" block"));
    };
    for profile in ["vertical", "horizontal"] {
        let Some((set, set_line)) = f.block(sets, profile)? else {
            continue;
        };
        if let Some((points, line)) = f.count(set, "nPoints")? {
            facts.insert(
                format!("/monitors/centreline_u_sample_count/{profile}"),
                f.fact(FactValue::Count(points), line),
            );
        } else {
            return Err(f.err(set_line, "the sampled line has no \"nPoints\" entry"));
        }
    }
    Ok(())
}

/// The kind and number of an object named `probe_<n>`, `line_<n>`, `force_<n>` or `surface_<n>`.
/// Any other name (the centreline, the balances) is not a monitor of the study.
fn monitor_name(key: &str) -> Option<(&str, &str)> {
    let (kind, n) = key.split_once('_')?;
    let counted = n.parse::<u64>().is_ok_and(|v| v >= 1 && v.to_string() == n);
    (counted && matches!(kind, "probe" | "line" | "force" | "surface")).then_some((kind, n))
}

fn monitor_facts(
    f: &File<'_>,
    files: &[SourceFile],
    functions: &Dict,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let mut kinds: Option<BTreeMap<String, String>> = None;
    for entry in functions.entries() {
        let Entry::Block { key, dict, line } = entry else {
            continue;
        };
        let Some((kind, n)) = monitor_name(key) else {
            continue;
        };
        match kind {
            "probe" => probe_facts(f, dict, *line, n, facts)?,
            "line" => line_facts(f, dict, *line, n, facts)?,
            "force" => force_facts(f, dict, *line, n, facts)?,
            _ => {
                if kinds.is_none() {
                    kinds = Some(patch_kinds(files)?);
                }
                if let Some(known) = &kinds {
                    surface_facts(f, dict, *line, n, known, facts)?;
                }
            }
        }
    }
    Ok(())
}

/// The monitored field of a probe or line, as the study words it.
fn monitor_field(f: &File<'_>, dict: &Dict, owner: u32) -> Result<Fact, ReadError> {
    let (word, line) = f.listed_word(dict, "fields", owner)?;
    let name = match word {
        "U" => "velocity",
        "p" => "pressure",
        other => {
            return Err(f.err(line, format!("\"{other}\" is not a monitored field")));
        }
    };
    Ok(f.fact(FactValue::Text(name.into()), line))
}

fn probe_facts(
    f: &File<'_>,
    dict: &Dict,
    owner: u32,
    n: &str,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let (kind, kind_line) = f.required_word(dict, "type", owner)?;
    let on_boundary = match kind {
        "probes" => false,
        "patchProbes" => true,
        other => {
            return Err(f.err(kind_line, format!("\"{other}\" is not a kind of probe")));
        }
    };
    let (tokens, line) = f
        .plain(dict, "probeLocations")?
        .ok_or_else(|| f.err(owner, "the probe has no \"probeLocations\" entry"))?;
    let point: Option<Vec<f64>> = match tokens {
        [Token::List(points)] => match points.as_slice() {
            [Token::List(coords)] => coords
                .iter()
                .map(|t| t.as_word().and_then(number_of))
                .collect(),
            _ => None,
        },
        _ => None,
    };
    let [x, y, _] = point.unwrap_or_default()[..] else {
        return Err(f.err(
            line,
            "a probe should list exactly one location of three numbers",
        ));
    };
    for (axis, value) in [("x", x), ("y", y)] {
        facts.insert(
            format!("/monitors/probe/{n}/{axis}"),
            f.fact(FactValue::Number(value), line),
        );
    }
    facts.insert(
        format!("/monitors/probe/{n}/field"),
        monitor_field(f, dict, owner)?,
    );
    if on_boundary {
        facts.insert(
            format!("/monitors/probe/{n}/on_boundary"),
            f.fact(FactValue::Flag(true), kind_line),
        );
        let (patch, patch_line) = f.listed_word(dict, "patches", owner)?;
        facts.insert(
            format!("/monitors/probe/{n}/patch"),
            f.fact(FactValue::Text(patch.to_owned()), patch_line),
        );
    }
    Ok(())
}

fn line_facts(
    f: &File<'_>,
    dict: &Dict,
    owner: u32,
    n: &str,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let (sets, sets_line) = f
        .block(dict, "sets")?
        .ok_or_else(|| f.err(owner, "the line object has no \"sets\" block"))?;
    let named: Vec<(&Dict, u32)> = sets
        .entries()
        .iter()
        .filter_map(|e| match e {
            Entry::Block { dict, line, .. } => Some((dict, *line)),
            _ => None,
        })
        .collect();
    let [(line_dict, line_owner)] = named.as_slice() else {
        return Err(f.err(sets_line, "a line object should hold exactly one set"));
    };
    let start = f.numbers(line_dict, "start", *line_owner, 3)?;
    let end = f.numbers(line_dict, "end", *line_owner, 3)?;
    let at = |key: &str| {
        f.plain(line_dict, key)
            .map(|r| r.map_or(*line_owner, |(_, l)| l))
    };
    let (start_line, end_line) = (at("start")?, at("end")?);
    for (name, value, line) in [
        ("x0", start[0], start_line),
        ("y0", start[1], start_line),
        ("x1", end[0], end_line),
        ("y1", end[1], end_line),
    ] {
        facts.insert(
            format!("/monitors/line/{n}/{name}"),
            f.fact(FactValue::Number(value), line),
        );
    }
    let (points, points_line) = f
        .count(line_dict, "nPoints")?
        .ok_or_else(|| f.err(*line_owner, "the set has no \"nPoints\" entry"))?;
    facts.insert(
        format!("/monitors/line/{n}/samples"),
        f.fact(FactValue::Count(points), points_line),
    );
    facts.insert(
        format!("/monitors/line/{n}/field"),
        monitor_field(f, dict, owner)?,
    );
    Ok(())
}

fn force_facts(
    f: &File<'_>,
    dict: &Dict,
    owner: u32,
    n: &str,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let (kind, kind_line) = f.required_word(dict, "type", owner)?;
    if kind != "forceCoeffs" {
        return Err(f.err(
            kind_line,
            "a force object should be of the kind forceCoeffs",
        ));
    }
    for (key, name) in [
        ("magUInf", "reference_velocity"),
        ("lRef", "reference_length"),
    ] {
        let (value, line) = f.required_number(dict, key, owner)?;
        facts.insert(
            format!("/monitors/force/{n}/{name}"),
            f.fact(FactValue::Number(value), line),
        );
    }
    let lift = f.numbers(dict, "liftDir", owner, 3)?;
    let lift_line = f.plain(dict, "liftDir")?.map_or(owner, |(_, l)| l);
    let axis = match lift[..] {
        [x, y, z] if x == 1.0 && y == 0.0 && z == 0.0 => "x",
        [x, y, z] if x == 0.0 && y == 1.0 && z == 0.0 => "y",
        [x, y, z] if x == 0.0 && y == 0.0 && z == 1.0 => "z",
        _ => {
            return Err(f.err(
                lift_line,
                "the lift direction should be an axis of the case",
            ));
        }
    };
    facts.insert(
        format!("/monitors/force/{n}/lift_axis"),
        f.fact(FactValue::Text(axis.into()), lift_line),
    );
    let (patch, patch_line) = f.listed_word(dict, "patches", owner)?;
    facts.insert(
        format!("/monitors/force/{n}/patch"),
        f.fact(FactValue::Text(patch.to_owned()), patch_line),
    );
    Ok(())
}

fn surface_facts(
    f: &File<'_>,
    dict: &Dict,
    owner: u32,
    n: &str,
    kinds: &BTreeMap<String, String>,
    facts: &mut Facts,
) -> Result<(), ReadError> {
    let (patch, patch_line) = f.required_word(dict, "patch", owner)?;
    let kind = kinds.get(patch).ok_or_else(|| {
        f.err(
            patch_line,
            format!("\"{patch}\" is not a patch of the case"),
        )
    })?;
    facts.insert(
        format!("/monitors/surface/{n}/kind"),
        f.fact(FactValue::Text(kind.clone()), patch_line),
    );
    let (operation, operation_line) = f.required_word(dict, "operation", owner)?;
    let (field, _) = f.listed_word(dict, "fields", owner)?;
    let quantity = match (operation, field) {
        ("sum", "phi") => "flux",
        ("areaAverage", "p") => "pressure_average",
        ("areaIntegrate", "p") => "pressure_integral",
        ("areaIntegrate", "gradT") => "heat_flux",
        (op, name) => {
            return Err(f.err(
                operation_line,
                format!("the operation \"{op}\" on \"{name}\" is not a monitored quantity"),
            ));
        }
    };
    facts.insert(
        format!("/monitors/surface/{n}/quantity"),
        f.fact(FactValue::Text(quantity.into()), operation_line),
    );
    Ok(())
}

/// What each patch is, in the study's words, from its mesh type and what the velocity file does on
/// it. Used to name the kind of the patch a surface integral reads.
fn patch_kinds(files: &[SourceFile]) -> Result<BTreeMap<String, String>, ReadError> {
    let mesh = File::load(files, MESH)?;
    let velocity = File::load(files, VELOCITY)?;
    let Some(Entry::Value { tokens, line, .. }) = mesh.doc.get("boundary") else {
        return Err(mesh.err(0, "the mesh dictionary has no \"boundary\" list"));
    };
    let [Token::List(items)] = tokens.as_slice() else {
        return Err(mesh.err(*line, "\"boundary\" should be a list of patches"));
    };
    let (field, field_line) = velocity
        .block(&velocity.doc.root, "boundaryField")?
        .ok_or_else(|| velocity.err(0, "the velocity file has no \"boundaryField\" block"))?;
    let mut kinds = BTreeMap::new();
    for pair in items.chunks(2) {
        let [Token::Word(name), Token::Dict(body)] = pair else {
            return Err(mesh.err(*line, "each patch should be a name and a block"));
        };
        let (mesh_type, _) = mesh.required_word(body, "type", *line)?;
        let (patch, patch_line) = velocity
            .block(field, name)?
            .ok_or_else(|| velocity.err(field_line, format!("\"{name}\" has no velocity entry")))?;
        let (velocity_type, _) = velocity.required_word(patch, "type", patch_line)?;
        kinds.insert(name.clone(), kind_text(mesh_type, velocity_type));
    }
    Ok(kinds)
}

/// The study's word for a patch of this mesh type that the velocity file treats this way.
fn kind_text(mesh_type: &str, velocity_type: &str) -> String {
    let kind = match (mesh_type, velocity_type) {
        ("empty", _) => return EMPTY_KIND.to_owned(),
        (_, "noSlip") => BoundaryKind::NoSlipWall,
        (_, "slip") => BoundaryKind::SlipWall,
        ("wall", "fixedValue" | "fixedProfile") => BoundaryKind::MovingWall,
        (_, "fixedValue" | "fixedProfile") => BoundaryKind::VelocityInlet,
        (_, "zeroGradient") => BoundaryKind::PressureOutlet,
        (_, "symmetry" | "symmetryPlane") => BoundaryKind::Symmetry,
        // An unknown type is reported as it is written, so the mismatch names it.
        (_, other) => return other.to_owned(),
    };
    kind_name(kind).to_owned()
}

/// The pressure reference of the single `PIMPLE` block says whether the domain is closed; a
/// `potentialFlow` block would make the start a potential-flow one.
fn solution(files: &[SourceFile], facts: &mut Facts) -> Result<(), ReadError> {
    let f = File::load(files, SOLUTION)?;
    let root = &f.doc.root;
    let init = match root.get("potentialFlow") {
        Some(entry) => f.fact(FactValue::Text("potential".into()), entry.line()),
        None => Fact {
            value: FactValue::Text("uniform".into()),
            value_spans: Vec::new(),
            entry_span: None,
        },
    };
    facts.insert("/solver/init".into(), init);
    let Some((pimple, owner)) = f.block(root, "PIMPLE")? else {
        return Ok(());
    };
    let (open, line) = match pimple.get("pRefCell") {
        Some(entry) => (false, entry.line()),
        None => (true, owner),
    };
    facts.insert("/domain/open".into(), f.fact(FactValue::Flag(open), line));
    Ok(())
}

impl CaseReader for Of14Reader {
    /// Reads `system/blockMeshDict`, `0/U`, `0/p` and, when the case has them, `0/p_rgh`, `0/T`,
    /// `constant/g` and the turbulence inflow files (through the reader of the older line),
    /// `system/controlDict`, `system/fvSchemes`, `system/fvSolution`, `constant/physicalProperties`,
    /// `constant/thermophysicalTransport` and `constant/momentumTransport`. The numerics are
    /// covered by the golden case bytes, except the time scheme, which says whether the run is
    /// steady.
    fn read(&self, files: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError> {
        let mut facts = shared_facts(files)?;
        viscosity(files, &mut facts)?;
        thermophysical(files, &mut facts)?;
        transport(files, &mut facts)?;
        control(files, &mut facts)?;
        solution(files, &mut facts)?;
        Ok(facts)
    }
}
