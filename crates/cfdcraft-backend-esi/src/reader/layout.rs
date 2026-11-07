//! The mesh dictionary of a case that is more than one box: any number of vertices, several
//! blocks, arc edges, and the conditions each patch holds. Everything is read from the written
//! bytes with the parser of the reader, which this module reaches through its parent. The module
//! knows nothing of how a layout is made or of the patch names the writer chooses.

use super::*;

/// True for a mesh dictionary that is not the one block of eight vertices the box path reads: a
/// dictionary with another number of vertices or blocks, or with a curved edge. A dictionary that
/// lacks `vertices` or `blocks` is left to the box path, which reports nothing for what is absent.
pub(super) fn is_multi_block(doc: &Doc) -> Result<bool, ReadError> {
    if let Some(vertices) = doc.top("vertices")
        && doc.list(vertices, b'(')?.items.len() != 8
    {
        return Ok(true);
    }
    if let Some(blocks) = doc.top("blocks")
        && doc.list(blocks, b'(')?.items.len() != 5
    {
        return Ok(true);
    }
    if let Some(edges) = doc.top("edges")
        && !doc.list(edges, b'(')?.items.is_empty()
    {
        return Ok(true);
    }
    Ok(false)
}

/// The grading of the three axes of a block as one text, and the tokens it is made of. An axis is
/// a plain number, or a list of sections of three numbers each; every number is printed as the
/// shortest decimal that reads back to it, so the text does not depend on how it was written.
pub(super) fn grading_axes(doc: &Doc, list: &List) -> Result<(String, Vec<Span>), ReadError> {
    if list.items.len() != 3 {
        return Err(doc.err(list.sp, "three gradings were expected"));
    }
    let mut texts = Vec::new();
    let mut spans = Vec::new();
    for item in &list.items {
        match item {
            Item::Atom(atom) => {
                let number = doc.num(atom)?;
                spans.push(doc.span(number.sp));
                texts.push(number.v.to_string());
            }
            Item::List(sections) if sections.open == b'(' => {
                let mut parts = Vec::new();
                for section in &sections.items {
                    let Item::List(triple) = section else {
                        return Err(
                            doc.err(section.sp(), "a grading section should be three numbers")
                        );
                    };
                    let [a, b, c] = doc.triple(triple)?;
                    spans.extend([&a, &b, &c].map(|n| doc.span(n.sp)));
                    parts.push(format!("({} {} {})", a.v, b.v, c.v));
                }
                texts.push(format!("({})", parts.join(" ")));
            }
            other => {
                return Err(doc.err(
                    other.sp(),
                    "a grading should be a number or a list of sections",
                ));
            }
        }
    }
    Ok((texts.join(" "), spans))
}

/// A vertex number, which must name a vertex of the dictionary.
fn vertex_index(doc: &Doc, atom: &Atom, vertices: usize) -> Result<usize, ReadError> {
    let index = doc.count(atom)?;
    match usize::try_from(index) {
        Ok(i) if i < vertices => Ok(i),
        _ => Err(doc.err(
            atom.sp,
            format!("vertex {index} does not exist, the mesh has {vertices}"),
        )),
    }
}

/// The vertices as three numbers each.
fn vertex_list(doc: &Doc, vertices: &Entry) -> Result<Vec<[Num; 3]>, ReadError> {
    let outer = doc.list(vertices, b'(')?;
    let mut points = Vec::new();
    for item in &outer.items {
        let Item::List(point) = item else {
            return Err(doc.err(item.sp(), "a vertex should be three numbers in brackets"));
        };
        if point.open != b'(' {
            return Err(doc.err(point.sp, "a vertex should be in round brackets"));
        }
        points.push(doc.triple(point)?);
    }
    if points.is_empty() {
        return Err(doc.err(vertices.sp, "the mesh has no vertices"));
    }
    Ok(points)
}

/// The lowest and highest coordinate of the vertices on each axis.
fn bounds(points: &[[Num; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for point in points {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(point[axis].v);
            hi[axis] = hi[axis].max(point[axis].v);
        }
    }
    (lo, hi)
}

fn vertex_facts(
    doc: &Doc,
    vertices: &Entry,
    points: &[[Num; 3]],
    (lo, hi): ([f64; 3], [f64; 3]),
    facts: &mut BTreeMap<String, Fact>,
) {
    let entry = doc.span(vertices.sp);
    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        let at = |end: f64| -> Vec<Span> {
            points
                .iter()
                .filter(|point| point[axis].v == end)
                .map(|point| doc.span(point[axis].sp))
                .collect()
        };
        facts.insert(
            format!("/domain/length_{name}"),
            fact(
                FactValue::Number(hi[axis] - lo[axis]),
                at(hi[axis]).into_iter().chain(at(lo[axis])).collect(),
                entry.clone(),
            ),
        );
        facts.insert(
            format!("/domain/origin_{name}"),
            fact(FactValue::Number(lo[axis]), at(lo[axis]), entry.clone()),
        );
    }
    facts.insert(
        "/mesh/vertices".into(),
        Fact {
            value: FactValue::Count(points.len() as u64),
            value_spans: Vec::new(),
            entry_span: Some(entry.clone()),
        },
    );
    for (i, point) in points.iter().enumerate() {
        for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
            facts.insert(
                format!("/mesh/vertex/{i}/{name}"),
                fact(
                    FactValue::Number(point[axis].v),
                    vec![doc.span(point[axis].sp)],
                    entry.clone(),
                ),
            );
        }
    }
}

/// `( hex (v0 .. v7) (nx ny nz) simpleGrading (gx gy gz) hex ... )`: any number of blocks.
fn block_facts(
    doc: &Doc,
    blocks: &Entry,
    vertices: usize,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let shape = "\"blocks\" should hold hex blocks with simple grading";
    let list = doc.list(blocks, b'(')?;
    let (chunks, rest) = list.items.as_chunks::<5>();
    if !rest.is_empty() {
        return Err(doc.err(blocks.sp, shape));
    }
    let mut count = 0u64;
    let mut total = 0u64;
    let mut cell_spans = Vec::new();
    for (j, chunk) in chunks.iter().enumerate() {
        let [
            Item::Atom(hex),
            Item::List(corners),
            Item::List(cells),
            Item::Atom(grading_kind),
            Item::List(grading),
        ] = chunk
        else {
            return Err(doc.err(blocks.sp, shape));
        };
        if hex.text != "hex" || grading_kind.text != "simpleGrading" {
            return Err(doc.err(blocks.sp, shape));
        }
        let entry = doc.span(hex.sp.join(grading.sp));
        if corners.items.len() != 8 || cells.items.len() != 3 {
            return Err(doc.err(
                corners.sp,
                "a block needs eight vertices and three cell counts",
            ));
        }
        let mut order = Vec::new();
        let mut corner_spans = Vec::new();
        for item in &corners.items {
            let Item::Atom(atom) = item else {
                return Err(doc.err(item.sp(), "a vertex number was expected"));
            };
            order.push(vertex_index(doc, atom, vertices)?.to_string());
            corner_spans.push(doc.span(atom.sp));
        }
        let mut counts = Vec::new();
        let mut cell_tokens = Vec::new();
        for item in &cells.items {
            let Item::Atom(atom) = item else {
                return Err(doc.err(item.sp(), "a cell count was expected"));
            };
            counts.push(doc.count(atom)?);
            cell_tokens.push(doc.span(atom.sp));
        }
        let product = counts
            .iter()
            .try_fold(1u64, |acc, c| acc.checked_mul(*c))
            .and_then(|n| total.checked_add(n))
            .ok_or_else(|| doc.err(cells.sp, "the cell count is too large"))?;
        total = product;
        facts.insert(
            format!("/mesh/block/{j}/vertices"),
            fact(
                FactValue::Text(order.join(" ")),
                corner_spans,
                entry.clone(),
            ),
        );
        facts.insert(
            format!("/mesh/block/{j}/cells"),
            fact(
                FactValue::Text(
                    counts
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                cell_tokens.clone(),
                entry.clone(),
            ),
        );
        let (text, spans) = grading_axes(doc, grading)?;
        facts.insert(
            format!("/mesh/block/{j}/grading"),
            fact(FactValue::Text(text), spans, entry),
        );
        cell_spans.extend(cell_tokens);
        count += 1;
    }
    facts.insert(
        "/mesh/blocks".into(),
        Fact {
            value: FactValue::Count(count),
            value_spans: Vec::new(),
            entry_span: Some(doc.span(blocks.sp)),
        },
    );
    facts.insert(
        "/mesh/cells_total".into(),
        fact(FactValue::Count(total), cell_spans, doc.span(blocks.sp)),
    );
    Ok(())
}

/// `( arc a b (x y z) arc ... )`: the curved edges. A plain `edges ();` holds none.
fn arc_facts(
    doc: &Doc,
    edges: &Entry,
    vertices: usize,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<(), ReadError> {
    let shape = "\"edges\" should hold arcs, each with two vertices and a point";
    let list = doc.list(edges, b'(')?;
    let (chunks, rest) = list.items.as_chunks::<4>();
    if !rest.is_empty() {
        return Err(doc.err(edges.sp, shape));
    }
    let mut count = 0u64;
    for (k, chunk) in chunks.iter().enumerate() {
        let [
            Item::Atom(kind),
            Item::Atom(from),
            Item::Atom(to),
            Item::List(through),
        ] = chunk
        else {
            return Err(doc.err(edges.sp, shape));
        };
        if kind.text != "arc" {
            return Err(doc.err(kind.sp, shape));
        }
        let entry = doc.span(kind.sp.join(through.sp));
        let ends = (
            vertex_index(doc, from, vertices)?,
            vertex_index(doc, to, vertices)?,
        );
        facts.insert(
            format!("/mesh/arc/{k}/ends"),
            fact(
                FactValue::Text(format!("{} {}", ends.0, ends.1)),
                vec![doc.span(from.sp), doc.span(to.sp)],
                entry.clone(),
            ),
        );
        for (name, number) in ["x", "y", "z"].into_iter().zip(doc.triple(through)?) {
            facts.insert(
                format!("/mesh/arc/{k}/through/{name}"),
                fact(
                    FactValue::Number(number.v),
                    vec![doc.span(number.sp)],
                    entry.clone(),
                ),
            );
        }
        count += 1;
    }
    facts.insert(
        "/mesh/arcs".into(),
        Fact {
            value: FactValue::Count(count),
            value_spans: Vec::new(),
            entry_span: Some(doc.span(edges.sp)),
        },
    );
    Ok(())
}

/// A patch of the mesh dictionary: its type, and its faces as vertex numbers.
struct ShapedPatch<'a> {
    name: &'a Atom,
    mesh_type: &'a Atom,
    quads: Vec<[usize; 4]>,
    entry: Sp,
}

fn shaped_patches<'a>(
    doc: &Doc,
    boundary: &'a Entry,
    vertices: usize,
    facts: &mut BTreeMap<String, Fact>,
) -> Result<Vec<ShapedPatch<'a>>, ReadError> {
    let mut patches = Vec::new();
    for (name, block) in doc.named_blocks(boundary)? {
        let type_entry = sub(&block.entries, "type")
            .ok_or_else(|| doc.err(name.sp, format!("the patch \"{}\" has no type", name.text)))?;
        let mesh_type = doc.word(type_entry)?;
        let faces_entry = sub(&block.entries, "faces")
            .ok_or_else(|| doc.err(name.sp, format!("the patch \"{}\" has no faces", name.text)))?;
        let faces = doc.list(faces_entry, b'(')?;
        let mut quads = Vec::new();
        let mut spans = Vec::new();
        for item in &faces.items {
            let Item::List(tuple) = item else {
                return Err(doc.err(item.sp(), "each face should be a list of vertex numbers"));
            };
            if tuple.open != b'(' || tuple.items.len() != 4 {
                return Err(doc.err(tuple.sp, "a face should be four vertex numbers in brackets"));
            }
            let mut quad = [0usize; 4];
            for (slot, vertex) in quad.iter_mut().zip(&tuple.items) {
                let Item::Atom(atom) = vertex else {
                    return Err(doc.err(vertex.sp(), "a vertex number was expected"));
                };
                *slot = vertex_index(doc, atom, vertices)?;
                spans.push(doc.span(atom.sp));
            }
            quads.push(quad);
        }
        let entry = name.sp.join(block.sp);
        let text = quads
            .iter()
            .map(|q| format!("({} {} {} {})", q[0], q[1], q[2], q[3]))
            .collect::<Vec<_>>()
            .join(" ");
        facts.insert(
            format!("/mesh/patch/{}/type", name.text),
            fact(
                FactValue::Text(mesh_type.text.clone()),
                vec![doc.span(mesh_type.sp)],
                doc.span(entry),
            ),
        );
        facts.insert(
            format!("/mesh/patch/{}/faces", name.text),
            fact(FactValue::Text(text), spans, doc.span(entry)),
        );
        patches.push(ShapedPatch {
            name,
            mesh_type,
            quads,
            entry,
        });
    }
    Ok(patches)
}

/// The sides of the bounding box the faces of a patch lie flat on, in x and y. A face on the front
/// or back, or on no side of the box, says nothing.
fn quad_faces(points: &[[Num; 3]], quads: &[[usize; 4]], lo: [f64; 3], hi: [f64; 3]) -> Vec<Face> {
    let mut found = BTreeSet::new();
    for quad in quads {
        for axis in 0..2 {
            let at = points[quad[0]][axis].v;
            if !quad.iter().all(|&i| points[i][axis].v == at) {
                continue;
            }
            if at == lo[axis] {
                found.extend(Side::of(axis, false).face());
            } else if at == hi[axis] {
                found.extend(Side::of(axis, true).face());
            }
        }
    }
    found.into_iter().collect()
}

/// How far up y the faces of a patch reach: the extent of an inlet across the flow.
fn extent_y(points: &[[Num; 3]], quads: &[[usize; 4]]) -> Option<(f64, f64)> {
    let ys = quads.iter().flatten().map(|&i| points[i][1].v);
    let lo = ys.clone().reduce(f64::min)?;
    let hi = ys.reduce(f64::max)?;
    Some((lo, hi))
}

/// The facts of a mesh dictionary of several blocks and of the conditions of its patches. The
/// patches are keyed by their written name, because without a box there are no faces to name them
/// by; the geometry is keyed by position.
pub(super) fn layout_facts(
    doc: &Doc,
    fields: &Fields<'_>,
    facts: &mut BTreeMap<String, Fact>,
    kinds: &mut PatchKinds,
) -> Result<(), ReadError> {
    let Some(vertices) = doc.top("vertices") else {
        return Err(fail(&doc.path, 0, "the mesh dictionary has no vertices"));
    };
    let points = vertex_list(doc, vertices)?;
    let (lo, hi) = bounds(&points);
    vertex_facts(doc, vertices, &points, (lo, hi), facts);
    if let Some(blocks) = doc.top("blocks") {
        block_facts(doc, blocks, points.len(), facts)?;
    }
    if let Some(edges) = doc.top("edges") {
        arc_facts(doc, edges, points.len(), facts)?;
    }
    let patches = match doc.top("boundary") {
        Some(boundary) => shaped_patches(doc, boundary, points.len(), facts)?,
        None => Vec::new(),
    };

    let (velocity, pressure) = (fields.velocity, fields.pressure);
    let u = field_patches(velocity)?;
    let p = field_patches(pressure)?;
    let t = fields.temperature.map(field_patches).transpose()?;
    let names: Vec<&Atom> = patches.iter().map(|patch| patch.name).collect();
    let pressure_label = if fields.buoyant { "0/p_rgh" } else { "0/p" };
    let mut sets = vec![(velocity, &u, "0/U"), (pressure, &p, pressure_label)];
    if let (Some(doc_t), Some(t)) = (fields.temperature, &t) {
        sets.push((doc_t, t, "0/T"));
    }
    match_patch_names(doc, &names, &sets)?;

    for patch in &patches {
        let name = &patch.name.text;
        let (fu, fp) = (&u[name], &p[name]);
        let (recovered, text) = recover(fields, &patch.mesh_type.text, fu, fp)?;
        kinds.insert(name.clone(), text.clone());
        facts.insert(
            format!("/patch/{name}/kind"),
            fact(
                FactValue::Text(text),
                vec![
                    doc.span(patch.mesh_type.sp),
                    velocity.span(fu.ty.sp),
                    pressure.span(fp.ty.sp),
                ],
                doc.span(patch.entry),
            ),
        );
        let Recovered::Known(kind) = &recovered else {
            continue;
        };
        let faces = quad_faces(&points, &patch.quads, lo, hi);
        let extent = extent_y(&points, &patch.quads);
        if let Some(speed) = speed_fact(velocity, *kind, &faces, fu, extent)? {
            facts.insert(format!("/patch/{name}/velocity"), speed);
        }
        if let Some(profile) = profile_fact(velocity, *kind, fu) {
            facts.insert(format!("/patch/{name}/profile"), profile);
        }
        if let (Some(doc_t), Some(t)) = (fields.temperature, &t)
            && let Some(ft) = t.get(name)
        {
            for (suffix, thermal) in physics::thermal_facts(doc_t, *kind, ft)? {
                facts.insert(format!("/patch/{name}/{suffix}"), thermal);
            }
        }
    }
    Ok(())
}
