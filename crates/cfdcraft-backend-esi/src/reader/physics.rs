//! The facts of the physics a study can choose beyond the lid-driven cavity: the initialisation,
//! the time step, buoyancy, the turbulence inflow values, the thermal condition of a wall and the
//! profile of an inlet. Everything is read from the written bytes with the parser of the reader,
//! which this module reaches through its parent; none of it knows how the writer works.

use super::*;

/// The files of the three turbulence inflow values; each is there only for the models that use it.
pub(super) struct Inflow {
    pub k: Option<Doc>,
    pub omega: Option<Doc>,
    pub epsilon: Option<Doc>,
}

/// A potential-flow start has a `potentialFlow` dictionary in the solution settings; every other
/// start is uniform.
pub(super) fn init_fact(doc: &Doc, facts: &mut BTreeMap<String, Fact>) {
    let (word, spans, entry) = match doc.top("potentialFlow") {
        Some(entry) => {
            let key = Sp {
                start: entry.sp.start,
                end: entry.sp.start + entry.key.len(),
                line: entry.sp.line,
            };
            ("potential", vec![doc.span(key)], Some(doc.span(entry.sp)))
        }
        None => ("uniform", Vec::new(), None),
    };
    facts.insert(
        "/solver/init".into(),
        Fact {
            value: FactValue::Text(word.into()),
            value_spans: spans,
            entry_span: entry,
        },
    );
}

/// The step and the write control of a transient run, which only a pimple solver has.
pub(super) fn time_facts(
    doc: &Doc,
    application: &str,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    if !application.to_ascii_lowercase().ends_with("pimplefoam") {
        return Ok(());
    }
    if let Some(entry) = doc.top("deltaT") {
        let word = doc.word(entry)?;
        facts.insert(
            "/solver/time/delta_t".into(),
            fact(
                FactValue::Number(doc.num(word)?.v),
                vec![doc.span(word.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    if let Some(entry) = doc.top("writeControl") {
        let word = doc.word(entry)?;
        facts.insert(
            "/solver/time/write_control".into(),
            fact(
                FactValue::Text(word.text.clone()),
                vec![doc.span(word.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    Ok(())
}

/// The four dimensioned numbers of `transportProperties` and the gravity of `g`, which acts along
/// minus y: the file holds the signed component, the fact its magnitude.
pub(super) fn buoyancy_facts(
    transport: &Doc,
    gravity: Option<&Doc>,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    for (key, name) in [
        ("beta", "beta"),
        ("TRef", "reference_temperature"),
        ("Pr", "prandtl"),
        ("Prt", "turbulent_prandtl"),
    ] {
        if let Some(entry) = transport.top(key) {
            let value = dimensioned(transport, entry)?;
            facts.insert(
                format!("/physics/buoyancy/{name}"),
                fact(
                    FactValue::Number(value.v),
                    vec![transport.span(value.sp)],
                    transport.span(entry.sp),
                ),
            );
        }
    }
    let Some(doc) = gravity else {
        return Ok(());
    };
    let Some(entry) = doc.top("value") else {
        return Ok(());
    };
    let vector = match doc.items(entry)? {
        [Item::List(vector)] if vector.open == b'(' => doc.triple(vector)?,
        _ => {
            return Err(doc.err(entry.sp, "gravity should be three numbers in brackets"));
        }
    };
    if vector[0].v != 0.0 || vector[2].v != 0.0 {
        return Err(doc.err(entry.sp, "gravity should act along y only"));
    }
    facts.insert(
        "/physics/buoyancy/gravity".into(),
        fact(
            FactValue::Number(-vector[1].v),
            vec![doc.span(vector[1].sp)],
            doc.span(entry.sp),
        ),
    );
    Ok(())
}

/// The value each turbulence field holds everywhere to start with, which is the inflow value.
pub(super) fn inflow_facts(
    inflow: &Inflow,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    for (file, name) in [
        (&inflow.k, "k"),
        (&inflow.omega, "omega"),
        (&inflow.epsilon, "epsilon"),
    ] {
        let Some(doc) = file else {
            continue;
        };
        let Some(entry) = doc.top("internalField") else {
            continue;
        };
        let value = match doc.items(entry)? {
            [Item::Atom(kind), Item::Atom(value)] if kind.text == "uniform" => doc.num(value)?,
            _ => {
                return Err(doc.err(entry.sp, "an inflow value should be uniform and one number"));
            }
        };
        facts.insert(
            format!("/physics/turbulence/{name}"),
            fact(
                FactValue::Number(value.v),
                vec![doc.span(value.sp)],
                doc.span(entry.sp),
            ),
        );
    }
    Ok(())
}

/// What a wall does to heat, from its entry in the temperature file: held at a value, or left
/// alone. Anything that is not a wall has no thermal condition of its own. The answers are the
/// last part of the key and the fact.
pub(super) fn thermal_facts(
    doc: &Doc,
    kind: BoundaryKind,
    patch: &FieldPatch<'_>,
) -> Result<Vec<(&'static str, Fact)>, ReadError> {
    if !matches!(
        kind,
        BoundaryKind::NoSlipWall | BoundaryKind::SlipWall | BoundaryKind::MovingWall
    ) {
        return Ok(Vec::new());
    }
    let word = |text: &str| {
        fact(
            FactValue::Text(text.into()),
            vec![doc.span(patch.ty.sp)],
            doc.span(patch.entry),
        )
    };
    match patch.ty.text.as_str() {
        "fixedValue" => {
            let Some(value) = patch.value else {
                return Err(doc.err(patch.entry, "a fixed temperature has no value"));
            };
            let number = match doc.items(value)? {
                [Item::Atom(kind), Item::Atom(number)] if kind.text == "uniform" => {
                    doc.num(number)?
                }
                _ => {
                    return Err(doc.err(
                        value.sp,
                        "a temperature value should be uniform and one number",
                    ));
                }
            };
            Ok(vec![
                ("thermal", word("fixed")),
                (
                    "temperature",
                    fact(
                        FactValue::Number(number.v),
                        vec![doc.span(number.sp)],
                        doc.span(value.sp),
                    ),
                ),
            ])
        }
        "zeroGradient" => Ok(vec![("thermal", word("adiabatic"))]),
        _ => Ok(Vec::new()),
    }
}

/// The mean speed of a `fixedProfile` inlet and the tokens it was worked out from.
pub(super) struct Mean {
    /// The signed mean along x, as the velocity of the inlet.
    pub value: f64,
    pub spans: Vec<Span>,
    /// The `profile` entry.
    pub entry: Sp,
}

/// The polynomial `a s + b s^2` of a parabolic inflow, with `s` the distance along y from the
/// start of the inlet, is zero at both walls and peaks at 1.5 times its mean in the middle, so the
/// mean speed over an inlet from `lo` to `hi` is `a (hi - lo) / 6`. The profile must be exactly
/// that: a polynomial of the powers 1 and 2 along (0 1 0), starting at `lo`, with `b = -a / h`.
/// The mean is returned as the shortest decimal that gives back the written coefficient, which is
/// the number a person typed.
pub(super) fn profile_mean(
    doc: &Doc,
    patch: &FieldPatch<'_>,
    lo: f64,
    hi: f64,
) -> Result<Mean, ReadError> {
    let entry_of = |key: &str| {
        sub(&patch.block.entries, key)
            .ok_or_else(|| doc.err(patch.entry, format!("the inlet has no \"{key}\" entry")))
    };
    let profile = entry_of("profile")?;
    let shape = "the profile should be a polynomial of two terms";
    let [Item::Atom(kind), Item::List(terms)] = doc.items(profile)? else {
        return Err(doc.err(profile.sp, shape));
    };
    if kind.text != "polynomial" || terms.open != b'(' || terms.items.len() != 2 {
        return Err(doc.err(profile.sp, shape));
    }
    let mut coefficients: Vec<Num> = Vec::new();
    for (item, power) in terms.items.iter().zip([1.0, 2.0]) {
        let Item::List(term) = item else {
            return Err(doc.err(item.sp(), shape));
        };
        let [Item::List(coefficient), Item::List(exponent)] = term.items.as_slice() else {
            return Err(doc.err(term.sp, shape));
        };
        let [c, c1, c2] = doc.triple(coefficient)?;
        let [p, p1, p2] = doc.triple(exponent)?;
        if c1.v != 0.0 || c2.v != 0.0 || p.v != power || p1.v != 0.0 || p2.v != 0.0 {
            return Err(doc.err(
                term.sp,
                "the profile should be a polynomial in the distance along y",
            ));
        }
        coefficients.push(c);
    }
    let direction = entry_of("direction")?;
    let direction_ok = match doc.items(direction)? {
        [Item::List(v)] if v.open == b'(' => {
            let [x, y, z] = doc.triple(v)?;
            x.v == 0.0 && y.v == 1.0 && z.v == 0.0
        }
        _ => false,
    };
    if !direction_ok {
        return Err(doc.err(direction.sp, "the profile should run along (0 1 0)"));
    }
    let origin_entry = entry_of("origin")?;
    let origin = doc.num(doc.word(origin_entry)?)?;
    let height = hi - lo;
    if origin.v != lo || height <= 0.0 {
        return Err(doc.err(
            origin_entry.sp,
            "the profile should start at the lower edge of the inlet",
        ));
    }
    let (linear, quadratic) = (coefficients[0].v, coefficients[1].v);
    if (quadratic + linear / height).abs() > 1e-9 * (linear / height).abs() {
        return Err(doc.err(profile.sp, "the profile is not a parabola across the inlet"));
    }
    let guess = linear * height / 6.0;
    let mean = (1..=17usize)
        .filter_map(|digits| format!("{:.*e}", digits - 1, guess).parse::<f64>().ok())
        .find(|candidate| 6.0 * candidate / height == linear)
        .unwrap_or(guess);
    Ok(Mean {
        value: mean,
        spans: coefficients.iter().map(|c| doc.span(c.sp)).collect(),
        entry: profile.sp,
    })
}
