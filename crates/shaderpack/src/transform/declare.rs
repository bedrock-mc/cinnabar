//! Sorts top-level declarations into the translated module's sections: hoisted constants and
//! structs, the loose-uniform block, resources with fixed bindings, and interface variables
//! with locations shared between a program's stages.

use super::body::{BoolStorage, SamplerKinds, Structs, flatten_leaves};
use super::items::{
    Decl, Item, arr_len, is_image, is_integer, is_opaque, layout_key, loc_size, mat_cols, matching,
    parse_decl, sampler_info, split_items, strip_layout_keys,
};
use super::{ProgramInterface, TransformError, UboMember};
use crate::programs::StageKind;
use crate::reflection::{
    FIRST_OTHER_ATTRIBUTE_LOCATION, IMAGE_GROUP, STORAGE_BUFFER_GROUP, TEXTURE_GROUP,
    UNIFORM_GROUP, VertexAttribute,
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
}

/// Finds the first non-generated source line in a declaration.
fn first_line(toks: &[Tok]) -> u32 {
    toks.iter().map(|t| t.line).find(|l| *l != 0).unwrap_or(0)
}

/// Collects declarations of `items` for `stage`.
pub(super) fn collect(
    items: Vec<Item>,
    stage: StageKind,
    iface: &mut ProgramInterface,
    image_formats: &HashMap<String, String>,
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
            uniforms(&mut c, &d, iface, image_formats, line)?;
            continue;
        }
        let is_in =
            d.has("in") || (d.has("varying") && stage == StageKind::Fragment) || d.has("attribute");
        let is_out = d.has("out") || (d.has("varying") && stage == StageKind::Vertex);
        if stage == StageKind::Vertex && is_in && !d.has("varying") {
            for (name, arr, _) in &d.decls {
                let loc = if let Some(loc) = VertexAttribute::from_glsl_name(name).location() {
                    loc
                } else {
                    let overflow = || TransformError::new("attribute location overflow", line);
                    let mut size = loc_size(&d.ty);
                    for mut dimensions in [&d.ty_arr[..], &arr[..]] {
                        while !dimensions.is_empty() {
                            let close = matching(dimensions, 0, "[", "]");
                            size = size
                                .checked_mul(arr_len(&dimensions[..=close]).unwrap_or(1).max(1))
                                .ok_or_else(overflow)?;
                            dimensions = &dimensions[close + 1..];
                        }
                    }
                    let loc = next_attr;
                    next_attr = next_attr.checked_add(size).ok_or_else(overflow)?;
                    loc
                };
                c.vertex_inputs.push((loc, name.clone()));
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
                    &mut c, iface, &structs, name, &d.ty, &a, &d.quals, dir, explicit, line,
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
    for q in &d.quals {
        c.resources.push(id(q));
    }
    c.resources.extend([id(bname), p("{")]);
    c.resources.extend(members.iter().cloned());
    c.resources.push(p("}"));
    c.resources.extend(inst.iter().cloned());
    c.resources.push(p(";"));
    Ok(())
}

/// Lowers loose uniforms, sampled textures and storage images.
fn uniforms(
    c: &mut Collected,
    d: &Decl,
    iface: &mut ProgramInterface,
    image_formats: &HashMap<String, String>,
    line: u32,
) -> Result<(), TransformError> {
    for (name, arr, _) in &d.decls {
        if is_opaque(&d.ty) {
            if let Some((tex_ty, shadow)) = sampler_info(&d.ty) {
                let b = iface.texture_binding(name);
                c.resource_names.push((TEXTURE_GROUP, b, name.clone()));
                c.samplers.insert(name.clone(), (d.ty.clone(), shadow));
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
                    if let Some(f) = image_formats.get(name) {
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
                for q in &d.quals {
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
        let ty = match d.ty.as_str() {
            "bool" => {
                c.bools.insert(
                    name.clone(),
                    BoolStorage::new("bool", d.ty_arr.iter().chain(arr)),
                );
                "int".to_string()
            }
            b if b.starts_with("bvec") => {
                c.bools.insert(
                    name.clone(),
                    BoolStorage::new(b, d.ty_arr.iter().chain(arr)),
                );
                format!("i{}", &b[1..])
            }
            t => t.to_string(),
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

/// Emits a vertex output or fragment input. Matrices, arrays and structs are not legal
/// inter-stage types in WGSL, so they become vector leaves copied in a wrapper `main`.
#[allow(clippy::too_many_arguments)]
fn varying(
    c: &mut Collected,
    iface: &mut ProgramInterface,
    structs: &Structs,
    name: &str,
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
    if arr.is_empty() && mat_cols(ty).is_none() && !structs.contains_key(ty) {
        let size = loc_size(ty);
        let loc = iface.varying(stage, name, size, explicit);
        let q = if dir == "in" {
            qualifiers(ty)
        } else {
            interp.join(" ")
        };
        c.io.extend(
            lex(&format!("layout(location = {loc}) {q} {dir} {ty} {name};"))
                .into_iter()
                .map(|t| t.at(line)),
        );
        return Ok(size);
    }
    let mut leaves = vec![];
    flatten_leaves(name.to_string(), ty, arr, structs, line, &mut leaves)?;
    c.split_globals.extend([id(ty).at(line), id(name).at(line)]);
    c.split_globals.extend_from_slice(arr);
    c.split_globals.push(p(";"));
    let size = leaves.iter().map(|(_, ty)| loc_size(ty)).sum();
    let base = iface.varying(stage, name, size, explicit);
    let mut offset = 0;
    for (k, (path, leaf_ty)) in leaves.iter().enumerate() {
        let vname = format!("iris_v_{name}_{k}");
        let loc = format!("{base} + {offset}");
        offset += loc_size(leaf_ty);
        c.io.extend(
            lex(&format!(
                "layout(location = {loc}) {} {dir} {leaf_ty} {vname};",
                qualifiers(leaf_ty)
            ))
            .into_iter()
            .map(|t| t.at(line)),
        );
        if dir == "out" {
            c.after_main.push(format!("{vname} = {path};"));
        } else {
            c.before_main.push(format!("{path} = {vname};"));
        }
    }
    Ok(size)
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
    use crate::reflection::FIRST_TEXTURE_BINDING;

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
        let vertex = collect(items(VERTEX), StageKind::Vertex, &mut iface, &formats).unwrap();
        let fragment = collect(items(FRAGMENT), StageKind::Fragment, &mut iface, &formats).unwrap();

        let textures = [(TEXTURE_GROUP, FIRST_TEXTURE_BINDING, "colortex0".to_owned())];
        assert_eq!(vertex.resource_names, textures);
        assert_eq!(fragment.resource_names, textures);
        assert_eq!(
            fragment.samplers["colortex0"],
            ("sampler2D".to_owned(), false)
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
            &HashMap::new(),
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
            &formats,
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
