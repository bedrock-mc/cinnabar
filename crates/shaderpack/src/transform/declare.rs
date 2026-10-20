//! Sorts top-level declarations into the translated module's sections: hoisted constants and
//! structs, the loose-uniform block, resources with fixed bindings, and interface variables
//! with locations shared between a program's stages.

use super::body::{BoolStorage, SamplerKinds, Structs, flatten_leaves};
use super::items::{
    Decl, Item, arr_len, is_image, is_integer, is_opaque, layout_key, loc_size, mat_cols,
    parse_decl, sampler_info, split_items, strip_layout_keys,
};
use super::{ProgramInterface, ResourceNames, TransformError, UboMember};
use crate::programs::StageKind;
use crate::reflection::{
    FIRST_OTHER_ATTRIBUTE_LOCATION, IMAGE_GROUP, STORAGE_BUFFER_GROUP, SamplerClass, StoredNumbers,
    TEXTURE_GROUP, TextureDimension, UNIFORM_GROUP, VertexAttribute,
};
use crate::token::{Tok, id, lex, p};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The sorted declarations of one stage.
#[derive(Default)]
pub(super) struct Collected {
    /// Structs and constants, in source order.
    pub hoisted: Vec<Tok>,
    pub ubo: Vec<UboMember>,
    pub resources: Vec<Tok>,
    pub io: Vec<Tok>,
    /// Functions and remaining statements.
    pub body: Vec<Item>,
    pub samplers: SamplerKinds,
    pub bools: HashMap<String, BoolStorage>,
    /// Statements run after the user's `main` (vertex varying packing, output widening).
    pub after_main: Vec<String>,
    /// Statements run before the user's `main` (fragment varying unpacking).
    pub before_main: Vec<String>,
    /// Private globals backing flattened varyings and widened outputs.
    pub split_globals: Vec<Tok>,
    /// Global initializers moved into the `main` prologue.
    pub global_inits: Vec<Tok>,
    pub frag_locations: BTreeSet<u32>,
    pub declared: HashSet<String>,
    /// Vertex inputs as (location, translated name).
    pub vertex_inputs: Vec<(u32, String)>,
    /// Fragment outputs as (location, translated name).
    pub frag_outputs: Vec<(u32, String)>,
    /// Pack-visible resource names as (group, binding, name).
    pub resource_names: Vec<(u32, u32, String)>,
    /// The next location an `Other` vertex input would take.
    pub next_attribute: u32,
    /// A position or lightmap input was declared, so the raw position parts must be too.
    pub reads_packed_parts: bool,
}

/// Finds the first non-generated source line in a declaration.
fn first_line(toks: &[Tok]) -> u32 {
    toks.iter().map(|t| t.line).find(|l| *l != 0).unwrap_or(0)
}

/// Name of the raw packed-position input ([`VertexAttribute::PackedPosition`]).
const PACKED_POSITION_INPUT: &str = "iris_PackedPosition";
/// Name of the per-draw origin input ([`VertexAttribute::DrawOrigin`]).
const DRAW_ORIGIN_INPUT: &str = "iris_DrawOrigin";

/// The expression a position input of type `ty` reads: the float position `stored` plus the
/// packed position scaled and offset by the draw origin. A vertex stream supplies one of the
/// two and zero for the other (and for the origin), so either sum is exact.
fn position_unpack(stored: &str, ty: &str) -> String {
    let position = format!(
        "({stored} + {DRAW_ORIGIN_INPUT}.xyz + vec3({PACKED_POSITION_INPUT}.xyz) * {DRAW_ORIGIN_INPUT}.w)"
    );
    match ty {
        "vec4" => format!("vec4({position}, 1.0)"),
        "vec2" => format!("{position}.xy"),
        "float" => format!("{position}.x"),
        _ => position,
    }
}

/// The lightmap coordinates a packed vertex stores in the `w` of its packed position: block
/// light in the low byte, sky light in the high byte, as Java's level × 16. (Generated GLSL
/// keeps to the operators `token::lex` knows.)
const PACKED_LIGHTMAP: &str = "vec2(float(iris_PackedPosition.w - iris_PackedPosition.w / 256u * 256u), float(iris_PackedPosition.w / 256u))";

/// Whether input `attribute` reads the raw position parts ([`position_parts`]).
fn reads_packed_parts(attribute: &VertexAttribute) -> bool {
    matches!(
        attribute,
        VertexAttribute::Position | VertexAttribute::Lightmap | VertexAttribute::MultiTexCoord2
    )
}

/// An input whose declared type differs from what the vertex stream stores for its
/// attribute: positions, which are unpacked ([`position_unpack`]), lightmap coordinates,
/// stored as floats or in the packed position ([`PACKED_LIGHTMAP`]), and attributes stored
/// as the other kind of number ([`VertexAttribute::stored_numbers`]). Returns the name and
/// type of the input the stage reads instead, and the expression converting that into the
/// declared type `ty`.
fn converted_input(
    attribute: &VertexAttribute,
    name: &str,
    ty: &str,
    arr: &[Tok],
) -> Option<(String, String, String)> {
    if !arr.is_empty() {
        return None;
    }
    if *attribute == VertexAttribute::Position {
        if ty != "float" && !ty.starts_with("vec") {
            return None;
        }
        let stored = format!("iris_float_{name}");
        let convert = position_unpack(&stored, ty);
        return Some((stored, "vec3".to_string(), convert));
    }
    let size = ty
        .trim_start_matches(['i', 'u'])
        .trim_start_matches("vec")
        .parse::<u8>()
        .ok();
    if matches!(
        attribute,
        VertexAttribute::Lightmap | VertexAttribute::MultiTexCoord2
    ) {
        if !is_integer(ty) && ty != "float" && !ty.starts_with("vec") {
            return None;
        }
        // A vertex stream supplies either float coordinates or packed ones and zero for the
        // other, so the sum is exact; integers round it back.
        let stored = format!("iris_float_{name}");
        let (read, sum) = match size {
            None => (
                "float".to_string(),
                format!("({stored} + {PACKED_LIGHTMAP}.x)"),
            ),
            Some(2) => (
                "vec2".to_string(),
                format!("({stored} + {PACKED_LIGHTMAP})"),
            ),
            Some(3) => (
                "vec3".to_string(),
                format!("({stored} + vec3({PACKED_LIGHTMAP}, 0.0))"),
            ),
            Some(_) => (
                "vec4".to_string(),
                format!("({stored} + vec4({PACKED_LIGHTMAP}, 0.0, 0.0))"),
            ),
        };
        let convert = if is_integer(ty) {
            format!("{ty}(round({sum}))")
        } else {
            sum
        };
        return Some((stored, read, convert));
    }
    match attribute.stored_numbers()? {
        StoredNumbers::Int if ty == "float" || ty.starts_with("vec") => {
            let read = size.map_or("int".to_string(), |size| format!("ivec{size}"));
            let stored = format!("iris_int_{name}");
            let convert = format!("{ty}({stored})");
            Some((stored, read, convert))
        }
        _ => None,
    }
}

/// Declares the raw position parts at the first locations the stage's inputs leave free,
/// once a position or lightmap input reads them.
pub(super) fn position_parts(io: &mut String, c: &mut Collected) {
    if !c.reads_packed_parts {
        return;
    }
    let others =
        FIRST_OTHER_ATTRIBUTE_LOCATION..c.next_attribute.max(FIRST_OTHER_ATTRIBUTE_LOCATION);
    let mut free = (0..).filter(|location| {
        !others.contains(location) && c.vertex_inputs.iter().all(|(used, _)| used != location)
    });
    let mut parts = vec![];
    for (name, ty) in [
        (PACKED_POSITION_INPUT, "uvec4"),
        (DRAW_ORIGIN_INPUT, "vec4"),
    ] {
        let location = free.next().expect("locations are unbounded");
        io.push_str(&format!("layout(location = {location}) in {ty} {name};\n"));
        parts.push((location, name.to_string()));
    }
    c.vertex_inputs.extend(parts);
}

/// Collects declarations of `items` for `stage`.
pub(super) fn collect(
    items: Vec<Item>,
    stage: StageKind,
    iface: &mut ProgramInterface,
    names: &ResourceNames<'_>,
) -> Result<Collected, TransformError> {
    let mut c = Collected::default();
    let mut structs: Structs = HashMap::new();
    let mut next_attr = FIRST_OTHER_ATTRIBUTE_LOCATION;
    let mut frag_out_next = 0u32;
    for item in items {
        let (d, toks) = match item {
            Item::Decl(d, toks) => (d, toks),
            other => {
                c.body.push(other);
                continue;
            }
        };
        c.declared.extend(d.decls.iter().map(|x| x.0.clone()));
        let line = first_line(&toks);
        if d.ty == "precision" || toks.first().and_then(|t| t.ident()) == Some("precision") {
            continue;
        }
        if d.ty.is_empty() {
            // A default qualifier such as `layout(local_size_x = 8) in;`.
            c.body.push(Item::Other(toks));
            continue;
        }
        if d.ty == "struct" {
            if let Some((sname, members, _)) = &d.block {
                let mut ms = vec![];
                for m in split_items(members) {
                    if let Some(md) = parse_decl(&m) {
                        for (mn, marr, _) in md.decls {
                            let mut a = md.ty_arr.clone();
                            a.extend(marr);
                            ms.push((md.ty.clone(), mn, a));
                        }
                    }
                }
                structs.insert(sname.clone(), ms);
            }
            c.hoisted.extend(toks);
            continue;
        }
        if d.has("const") && d.block.is_none() {
            c.hoisted.extend(toks);
            continue;
        }
        if let Some((bname, members, inst)) = &d.block {
            if d.has("uniform") || d.has("buffer") {
                block(&mut c, &d, bname, members, inst, iface, line)?;
                continue;
            }
            let between_stages = (stage == StageKind::Vertex && d.has("out"))
                || (stage == StageKind::Fragment && d.has("in"));
            if between_stages && !inst.iter().any(|t| t.is("[")) {
                let dir = if stage == StageKind::Vertex {
                    "out"
                } else {
                    "in"
                };
                interface_block(&mut c, iface, &structs, &d, bname, members, inst, dir, line)?;
                continue;
            }
            if d.has("in") || d.has("out") {
                return Err(TransformError::new(
                    format!("interface block `{bname}` is not supported"),
                    line,
                ));
            }
            c.body.push(Item::Other(toks));
            continue;
        }
        if d.has("uniform") {
            uniforms(&mut c, &d, iface, names, line)?;
            continue;
        }
        let is_in =
            d.has("in") || (d.has("varying") && stage == StageKind::Fragment) || d.has("attribute");
        let is_out = d.has("out") || (d.has("varying") && stage == StageKind::Vertex);
        if stage == StageKind::Vertex && is_in && !d.has("varying") {
            for (name, arr, _) in &d.decls {
                let attribute = VertexAttribute::from_glsl_name(name);
                let loc = attribute.location().unwrap_or_else(|| {
                    let l = next_attr;
                    next_attr += loc_size(&d.ty, arr_len(arr).unwrap_or(1));
                    l
                });
                c.vertex_inputs.push((loc, name.clone()));
                if let Some((stored, read, convert)) = converted_input(&attribute, name, &d.ty, arr)
                {
                    c.io.extend(
                        lex(&format!("layout(location = {loc}) in {read} {stored};"))
                            .into_iter()
                            .map(|t| t.at(line)),
                    );
                    c.split_globals.extend(
                        lex(&format!("{} {name};", d.ty))
                            .into_iter()
                            .map(|t| t.at(line)),
                    );
                    c.before_main.push(format!("{name} = {convert};"));
                    c.reads_packed_parts |= reads_packed_parts(&attribute);
                    continue;
                }
                c.io.extend(
                    lex(&format!("layout(location = {loc}) in {} {name}", d.ty))
                        .into_iter()
                        .map(|t| t.at(line)),
                );
                c.io.extend(arr.iter().cloned());
                c.io.push(p(";"));
            }
            continue;
        }
        if (stage == StageKind::Vertex && is_out) || (stage == StageKind::Fragment && is_in) {
            let dir = if stage == StageKind::Vertex {
                "out"
            } else {
                "in"
            };
            let mut explicit = layout_key(&d.layout, "location");
            for (name, arr, _) in &d.decls {
                let mut a = d.ty_arr.clone();
                a.extend(arr.iter().cloned());
                let size = varying(
                    &mut c, iface, &structs, name, name, &d.ty, &a, &d.quals, dir, explicit, line,
                )?;
                explicit = explicit
                    .map(|start| {
                        start
                            .checked_add(size)
                            .ok_or_else(|| TransformError::new("varying location overflow", line))
                    })
                    .transpose()?;
            }
            continue;
        }
        if stage == StageKind::Fragment && is_out {
            for (name, arr, _) in &d.decls {
                let loc = layout_key(&d.layout, "location").unwrap_or_else(|| {
                    while c.frag_locations.contains(&frag_out_next) {
                        frag_out_next += 1;
                    }
                    frag_out_next
                });
                c.frag_locations.insert(loc);
                if !arr.is_empty() {
                    return Err(TransformError::new(
                        format!("fragment output array `{name}` is not supported"),
                        line,
                    ));
                }
                fragment_output(&mut c, name, &d.ty, loc, line);
            }
            continue;
        }
        let plain = d
            .quals
            .iter()
            .all(|q| matches!(q.as_str(), "highp" | "mediump" | "lowp" | "precise"));
        if plain && d.decls.iter().any(|x| !x.2.is_empty()) {
            // GLSL runs global initializers before main and they may read uniforms; naga
            // needs constant initializers, so the assignment moves into main's prologue.
            for (name, arr, init) in &d.decls {
                let mut t: Vec<Tok> = d.quals.iter().map(|q| id(q).at(line)).collect();
                t.push(id(&d.ty).at(line));
                t.extend(d.ty_arr.iter().cloned());
                t.push(id(name).at(line));
                t.extend(arr.iter().cloned());
                t.push(p(";").at(line));
                c.body.push(Item::Other(t));
                if !init.is_empty() {
                    c.global_inits.extend([id(name).at(line), p("=").at(line)]);
                    c.global_inits.extend(init.iter().cloned());
                    c.global_inits.push(p(";").at(line));
                }
            }
            continue;
        }
        c.body.push(Item::Decl(d, toks));
    }
    c.next_attribute = next_attr;
    Ok(c)
}

/// Emits a bound uniform or storage block, rejecting binding conflicts.
#[allow(clippy::too_many_arguments)]
fn block(
    c: &mut Collected,
    d: &Decl,
    bname: &str,
    members: &[Tok],
    inst: &[Tok],
    iface: &mut ProgramInterface,
    line: u32,
) -> Result<(), TransformError> {
    let storage = d.has("buffer");
    let (set, binding) = if storage {
        (
            STORAGE_BUFFER_GROUP,
            iface
                .storage_binding(bname, layout_key(&d.layout, "binding"))
                .map_err(|message| TransformError::new(message, line))?,
        )
    } else {
        (UNIFORM_GROUP, iface.uniform_block_binding(bname))
    };
    let instance = inst.iter().find_map(|t| t.ident()).unwrap_or(bname);
    c.resource_names.push((set, binding, instance.to_string()));
    let mut lay = strip_layout_keys(&d.layout, &["binding", "set"]);
    if !lay
        .iter()
        .any(|t| matches!(t.ident(), Some("std140" | "std430" | "packed" | "shared")))
    {
        if !lay.is_empty() {
            lay.push(p(","));
        }
        lay.push(id(if storage { "std430" } else { "std140" }));
    }
    lay.extend(lex(&format!(", set = {set}, binding = {binding}u")));
    c.resources.extend([id("layout"), p("(")]);
    c.resources.extend(lay);
    c.resources.push(p(")"));
    for q in d.quals.iter().filter(|q| supported_qualifier(q)) {
        c.resources.push(id(q));
    }
    c.resources.extend([id(bname), p("{")]);
    // GLSL stores a block's bools as 32-bit integers, and WGSL has no host-shareable bool:
    // the members are declared as integers and accessed through conversions, as bool
    // uniforms are. A named instance's members are reached through `.` and stay bools.
    let anonymous = inst.iter().all(|t| t.ident().is_none());
    for m in split_items(members) {
        match parse_decl(&m).filter(|md| anonymous && is_bool(&md.ty)) {
            Some(md) => {
                let integer = integer_bool(&md.ty);
                for (name, array, _) in &md.decls {
                    c.bools.insert(
                        name.clone(),
                        BoolStorage::new(&md.ty, md.ty_arr.iter().chain(array)),
                    );
                }
                c.resources.extend(m.iter().map(|t| {
                    if t.ident() == Some(md.ty.as_str()) {
                        id(&integer).at(t.line)
                    } else {
                        t.clone()
                    }
                }));
            }
            None => c.resources.extend(m),
        }
    }
    c.resources.push(p("}"));
    c.resources.extend(inst.iter().cloned());
    c.resources.push(p(";"));
    Ok(())
}

/// Whether naga's parser takes a resource qualifier. It has no `coherent` or `volatile`;
/// without them a store is still visible after the barriers packs pair them with.
fn supported_qualifier(q: &str) -> bool {
    !matches!(q, "coherent" | "volatile")
}

/// Whether `ty` is `bool` or a `bvecN`.
fn is_bool(ty: &str) -> bool {
    ty == "bool" || ty.starts_with("bvec")
}

/// The integer type storing a bool type in a block (`int`, `ivecN`).
pub(super) fn integer_bool(ty: &str) -> String {
    if ty == "bool" {
        "int".to_owned()
    } else {
        format!("i{}", &ty[1..])
    }
}

/// Declares built-in vertex input `name` of type `ty` in `io` at its fixed attribute
/// location (read converted, as declared inputs are) and records it in `c`; names without a
/// fixed location are skipped.
pub(super) fn vertex_input(io: &mut String, c: &mut Collected, name: &str, ty: &str) {
    let attribute = VertexAttribute::from_glsl_name(name);
    let Some(loc) = attribute.location() else {
        return;
    };
    c.vertex_inputs.push((loc, name.to_string()));
    match converted_input(&attribute, name, ty, &[]) {
        Some((stored, read, convert)) => {
            io.push_str(&format!(
                "layout(location = {loc}) in {read} {stored};\n{ty} {name};\n"
            ));
            c.before_main.push(format!("{name} = {convert};"));
            c.reads_packed_parts |= reads_packed_parts(&attribute);
        }
        None => io.push_str(&format!("layout(location = {loc}) in {ty} {name};\n")),
    }
}

/// Lowers loose uniforms, sampled textures and storage images.
fn uniforms(
    c: &mut Collected,
    d: &Decl,
    iface: &mut ProgramInterface,
    names: &ResourceNames<'_>,
    line: u32,
) -> Result<(), TransformError> {
    for (name, arr, _) in &d.decls {
        if is_opaque(&d.ty) {
            if let Some((tex_ty, shadow)) = sampler_info(&d.ty) {
                let b = iface.texture_binding(name);
                c.resource_names.push((TEXTURE_GROUP, b, name.clone()));
                let classes = names.sampler_classes;
                let shaped = (name.clone(), Some(texture_dimension(tex_ty)));
                let class = classes
                    .get(&shaped)
                    .or_else(|| classes.get(&(name.clone(), None)))
                    .copied()
                    .unwrap_or_else(|| SamplerClass::for_name(name));
                c.samplers
                    .insert(name.clone(), (d.ty.clone(), shadow, class));
                c.resources.extend(
                    lex(&format!(
                        "layout(set = {TEXTURE_GROUP}, binding = {b}) uniform {tex_ty} {name}"
                    ))
                    .into_iter()
                    .map(|t| t.at(line)),
                );
                c.resources.extend(arr.iter().cloned());
                c.resources.push(p(";"));
            } else if is_image(&d.ty) {
                let b = iface.image_binding(name);
                c.resource_names.push((IMAGE_GROUP, b, name.clone()));
                let mut lay = strip_layout_keys(&d.layout, &["binding", "set"]);
                if lay.is_empty() {
                    // OpenGL allows format-less write-only images; WebGPU needs the format,
                    // which Iris knows from `image.<name>` in shaders.properties.
                    if let Some(f) = names.image_formats.get(name) {
                        lay.push(id(f));
                    }
                }
                if !lay.is_empty() {
                    lay.push(p(","));
                }
                lay.extend(lex(&format!("set = {IMAGE_GROUP}, binding = {b}")));
                c.resources.extend([id("layout").at(line), p("(")]);
                c.resources.extend(lay);
                c.resources.push(p(")"));
                for q in d.quals.iter().filter(|q| supported_qualifier(q)) {
                    c.resources.push(id(q));
                }
                c.resources.extend([id(&d.ty), id(name)]);
                c.resources.extend(arr.iter().cloned());
                c.resources.push(p(";"));
            } else {
                return Err(TransformError::new(
                    format!("unsupported opaque uniform type `{}`", d.ty),
                    line,
                ));
            }
            continue;
        }
        let ty = if is_bool(&d.ty) {
            c.bools.insert(
                name.clone(),
                BoolStorage::new(&d.ty, d.ty_arr.iter().chain(arr)),
            );
            integer_bool(&d.ty)
        } else {
            d.ty.clone()
        };
        let mut toks = vec![id(&ty).at(line)];
        toks.extend(d.ty_arr.iter().cloned());
        toks.push(id(name).at(line));
        toks.extend(arr.iter().cloned());
        toks.push(p(";").at(line));
        c.ubo.push(UboMember::new(name, toks));
    }
    Ok(())
}

/// Shape of a separate texture type (`texture3D`, `utexture2DArray` …).
fn texture_dimension(tex_ty: &str) -> TextureDimension {
    let shape = tex_ty
        .trim_start_matches(['i', 'u'])
        .trim_start_matches("texture");
    match shape {
        "1D" | "1DArray" => TextureDimension::D1,
        "2DArray" => TextureDimension::D2Array,
        "3D" => TextureDimension::D3,
        "Cube" => TextureDimension::Cube,
        "CubeArray" => TextureDimension::CubeArray,
        _ => TextureDimension::D2,
    }
}

/// Emits a vertex output or fragment input. Matrices, arrays and structs are not legal
/// inter-stage types in WGSL, so they become vector leaves copied in a wrapper `main`.
#[allow(clippy::too_many_arguments)]
fn varying(
    c: &mut Collected,
    iface: &mut ProgramInterface,
    structs: &Structs,
    key: &str,
    path: &str,
    ty: &str,
    arr: &[Tok],
    quals: &[String],
    dir: &str,
    explicit: Option<u32>,
    line: u32,
) -> Result<u32, TransformError> {
    let stage = if dir == "out" {
        StageKind::Vertex
    } else {
        StageKind::Fragment
    };
    let interp: Vec<&str> = quals
        .iter()
        .map(String::as_str)
        .filter(|q| matches!(*q, "flat" | "smooth" | "noperspective" | "centroid"))
        .collect();
    let qualifiers = |leaf_ty: &str| {
        let mut q = interp.clone();
        if is_integer(leaf_ty) && !q.contains(&"flat") {
            q.push("flat");
        }
        q.join(" ")
    };
    if key == path && arr.is_empty() && mat_cols(ty).is_none() && !structs.contains_key(ty) {
        let size = loc_size(ty, 1);
        let loc = iface.varying(stage, key, size, explicit);
        let q = if dir == "in" {
            qualifiers(ty)
        } else {
            interp.join(" ")
        };
        c.io.extend(
            lex(&format!("layout(location = {loc}) {q} {dir} {ty} {path};"))
                .into_iter()
                .map(|t| t.at(line)),
        );
        return Ok(size);
    }
    let mut leaves = vec![];
    flatten_leaves(path.to_string(), ty, arr, structs, line, &mut leaves)?;
    if key == path {
        c.split_globals.extend([id(ty).at(line), id(path).at(line)]);
        c.split_globals.extend_from_slice(arr);
        c.split_globals.push(p(";"));
    }
    let size = leaves.iter().map(|(_, ty)| loc_size(ty, 1)).sum();
    let base = iface.varying(stage, key, size, explicit);
    let mut offset = 0;
    for (k, (leaf_path, leaf_ty)) in leaves.iter().enumerate() {
        let vname = if key == path {
            format!("iris_v_{key}_{k}")
        } else {
            format!("iris_b_{}_{k}", key.replace('.', "_"))
        };
        let loc = format!("{base} + {offset}");
        offset += loc_size(leaf_ty, 1);
        c.io.extend(
            lex(&format!(
                "layout(location = {loc}) {} {dir} {leaf_ty} {vname};",
                qualifiers(leaf_ty)
            ))
            .into_iter()
            .map(|t| t.at(line)),
        );
        if dir == "out" {
            c.after_main.push(format!("{vname} = {leaf_path};"));
        } else {
            c.before_main.push(format!("{leaf_path} = {vname};"));
        }
    }
    Ok(size)
}

/// Declares an `in`/`out` interface block between the vertex and fragment stages. GLSL
/// matches blocks by block and member names whatever their instance names, so each member
/// is a varying keyed `Block.member`; a named instance (`out DATA { … } data_out;`) is a
/// private global of a struct with the block's members.
#[allow(clippy::too_many_arguments)]
fn interface_block(
    c: &mut Collected,
    iface: &mut ProgramInterface,
    structs: &Structs,
    d: &Decl,
    bname: &str,
    members: &[Tok],
    inst: &[Tok],
    dir: &str,
    line: u32,
) -> Result<(), TransformError> {
    let mut fields = vec![];
    for m in split_items(members) {
        if let Some(md) = parse_decl(&m) {
            for (name, arr, _) in md.decls {
                let mut a = md.ty_arr.clone();
                a.extend(arr);
                fields.push((
                    md.quals.clone(),
                    md.ty.clone(),
                    name,
                    a,
                    layout_key(&md.layout, "location"),
                ));
            }
        }
    }
    let instance = inst.iter().find_map(|t| t.ident());
    if let Some(instance) = instance {
        let sname = format!("iris_block_{bname}");
        c.hoisted
            .extend([id("struct").at(line), id(&sname).at(line), p("{").at(line)]);
        for (_, ty, name, arr, _) in &fields {
            c.hoisted.extend([id(ty).at(line), id(name).at(line)]);
            c.hoisted.extend(arr.iter().cloned());
            c.hoisted.push(p(";").at(line));
        }
        c.hoisted.extend([p("}").at(line), p(";").at(line)]);
        c.split_globals
            .extend([id(&sname).at(line), id(instance).at(line), p(";").at(line)]);
    }
    let mut explicit = layout_key(&d.layout, "location");
    for (quals, ty, name, arr, member_location) in &fields {
        if instance.is_none() {
            c.split_globals.extend([id(ty).at(line), id(name).at(line)]);
            c.split_globals.extend(arr.iter().cloned());
            c.split_globals.push(p(";").at(line));
        }
        let key = format!("{bname}.{name}");
        let path = instance.map_or_else(|| name.clone(), |i| format!("{i}.{name}"));
        let quals: Vec<_> = d.quals.iter().chain(quals).cloned().collect();
        let location = member_location.or(explicit);
        let size = varying(
            c, iface, structs, &key, &path, ty, arr, &quals, dir, location, line,
        )?;
        explicit = location
            .map(|start| {
                start
                    .checked_add(size)
                    .ok_or_else(|| TransformError::new("varying location overflow", line))
            })
            .transpose()?;
    }
    Ok(())
}

/// Declares a fragment output. GL lets a vec3 output feed an RGBA buffer; WebGPU needs at
/// least the target's component count, so narrow outputs widen to 4 components after main.
fn fragment_output(c: &mut Collected, name: &str, ty: &str, loc: u32, line: u32) {
    let pad = match ty {
        "float" => Some(("vec4", "0.0, 0.0, 1.0")),
        "vec2" => Some(("vec4", "0.0, 1.0")),
        "vec3" => Some(("vec4", "1.0")),
        "int" => Some(("ivec4", "0, 0, 1")),
        "ivec2" => Some(("ivec4", "0, 1")),
        "ivec3" => Some(("ivec4", "1")),
        "uint" => Some(("uvec4", "0u, 0u, 1u")),
        "uvec2" => Some(("uvec4", "0u, 1u")),
        "uvec3" => Some(("uvec4", "1u")),
        _ => None,
    };
    match pad {
        Some((wide, rest)) => {
            let wname = format!("iris_out_{name}");
            c.split_globals.extend(
                lex(&format!("{ty} {name};"))
                    .into_iter()
                    .map(|t| t.at(line)),
            );
            c.io.extend(
                lex(&format!("layout(location = {loc}) out {wide} {wname};"))
                    .into_iter()
                    .map(|t| t.at(line)),
            );
            c.after_main
                .push(format!("{wname} = {wide}({name}, {rest});"));
            c.frag_outputs.push((loc, wname));
        }
        None => {
            c.io.extend(
                lex(&format!("layout(location = {loc}) out {ty} {name};"))
                    .into_iter()
                    .map(|t| t.at(line)),
            );
            c.frag_outputs.push((loc, name.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflection::{FIRST_TEXTURE_BINDING, SamplerClass};
    use crate::transform::{ResourceNames, SamplerClasses};

    /// Resource names with the given image formats and no custom sampler classes.
    fn names(image_formats: &HashMap<String, String>) -> ResourceNames<'_> {
        static NO_CLASSES: std::sync::LazyLock<SamplerClasses> =
            std::sync::LazyLock::new(SamplerClasses::new);
        ResourceNames {
            image_formats,
            sampler_classes: &NO_CLASSES,
        }
    }

    /// Parses fixture declarations into collector input items.
    fn items(src: &str) -> Vec<Item> {
        split_items(&lex(src))
            .into_iter()
            .map(|item| match parse_decl(&item) {
                Some(decl) => Item::Decl(decl, item),
                None => Item::Other(item),
            })
            .collect()
    }

    /// Joins fixture token spellings.
    fn text(toks: &[Tok]) -> String {
        toks.iter().map(Tok::text).collect::<Vec<_>>().join(" ")
    }

    const VERTEX: &str = "uniform sampler2D colortex0;
uniform bool isDay;
uniform float viewWidth;
in vec4 myAttr;
out vec2 texcoord;
out mat2 rot;
flat out int blockId;
void main() {}";

    const FRAGMENT: &str = "uniform sampler2D colortex0;
in vec2 texcoord;
in mat2 rot;
flat in int blockId;
out vec3 color;
void main() {}";

    #[test]
    fn stages_collect_shared_resources_and_vertex_inputs() {
        let mut iface = ProgramInterface::default();
        let formats = HashMap::new();
        let names = names(&formats);
        let vertex = collect(items(VERTEX), StageKind::Vertex, &mut iface, &names).unwrap();
        let fragment = collect(items(FRAGMENT), StageKind::Fragment, &mut iface, &names).unwrap();

        let textures = [(TEXTURE_GROUP, FIRST_TEXTURE_BINDING, "colortex0".to_owned())];
        assert_eq!(vertex.resource_names, textures);
        assert_eq!(fragment.resource_names, textures);
        assert_eq!(
            fragment.samplers["colortex0"],
            ("sampler2D".to_owned(), false, SamplerClass::ClampLinear)
        );

        let ubo: Vec<_> = vertex.ubo.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(ubo, ["isDay", "viewWidth"]);
        assert_eq!(vertex.bools["isDay"].ty, "bool");
        assert_eq!(
            vertex.vertex_inputs,
            [(FIRST_OTHER_ATTRIBUTE_LOCATION, "myAttr".to_owned())]
        );
    }

    #[test]
    fn narrow_fragment_outputs_widen_to_four_components() {
        let mut iface = ProgramInterface::default();
        let fragment = collect(
            items(FRAGMENT),
            StageKind::Fragment,
            &mut iface,
            &names(&HashMap::new()),
        )
        .unwrap();
        assert_eq!(fragment.frag_outputs, [(0, "iris_out_color".to_owned())]);
        assert!(
            fragment
                .after_main
                .contains(&"iris_out_color = vec4(color, 1.0);".to_owned())
        );
    }

    #[test]
    fn format_less_images_take_their_format_from_the_pack_properties() {
        let mut iface = ProgramInterface::default();
        let formats = HashMap::from([("colorimg0".to_owned(), "rgba16f".to_owned())]);
        let compute = collect(
            items("uniform writeonly image2D colorimg0; void main() {}"),
            StageKind::Compute,
            &mut iface,
            &names(&formats),
        )
        .unwrap();
        assert_eq!(
            text(&compute.resources),
            format!(
                "layout ( rgba16f , set = {IMAGE_GROUP} , binding = 0 ) uniform writeonly image2D colorimg0 ;"
            )
        );
    }
}
