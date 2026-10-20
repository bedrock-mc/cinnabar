//! Compatibility-profile / GLSL 1.20 → Vulkan-flavoured GLSL 4.50 for naga's GLSL frontend.
//!
//! Works on the fully preprocessed token stream (no macros left), so a light parser over
//! top-level declarations is enough; function bodies are only rewritten at identifier level.
//! Each stage becomes a [`StageDraft`]; a program's drafts are assembled with one shared
//! loose-uniform block so every stage binds the same layout.

mod assemble;
mod body;
mod declare;
mod interface;
mod items;
mod legacy;
mod specialize;

use crate::preprocess::Preprocessed;
use crate::programs::StageKind;
use crate::reflection::{
    LEGACY_UNIFORMS, SHADOW_SAMPLER_BINDING, SHADOW_SAMPLER_NAME, SamplerClass, TEXTURE_GROUP,
    TextureDimension, VertexAttribute,
};
use crate::token::{Tok, lex};
pub(crate) use assemble::{UboMember, assemble, merge_uniforms};
use body::{BodyContext, SamplerUse, rewrite_body, rewrite_params};
pub(crate) use interface::ProgramInterface;
use items::{Item, func_params, parse_decl, split_items};
use std::collections::{HashMap, HashSet};

/// A transform failure at a 1-based line of the include-expanded source (0 if unknown).
#[derive(Debug)]
pub(crate) struct TransformError {
    pub message: String,
    pub line: u32,
}

impl TransformError {
    /// Creates a transform error at the expanded source line.
    pub fn new(message: impl Into<String>, line: u32) -> TransformError {
        TransformError {
            message: message.into(),
            line,
        }
    }
}

/// One translated stage, waiting for the program's merged uniform block.
pub(crate) struct StageDraft {
    header: String,
    hoisted: Vec<Tok>,
    pub ubo: Vec<UboMember>,
    resources: Vec<Tok>,
    samplers: SamplerUse,
    builtin_io: String,
    io: Vec<Tok>,
    split_globals: Vec<Tok>,
    body: Vec<Tok>,
    /// Vertex inputs as (location, translated name).
    pub vertex_inputs: Vec<(u32, String)>,
    /// Fragment outputs as (location, translated name).
    pub frag_outputs: Vec<(u32, String)>,
    /// Pack-visible resource names as (group, binding, name).
    pub resource_names: Vec<(u32, u32, String)>,
}

/// Sampler name and shape (`None` for any shape) → class, where the program's custom
/// textures and images override [`SamplerClass::for_name`].
pub(crate) type SamplerClasses = HashMap<(String, Option<TextureDimension>), SamplerClass>;

/// Pack-level facts about resource names, from `shaders.properties`.
pub(crate) struct ResourceNames<'a> {
    /// Image name → GLSL format qualifier from `image.<name>`.
    pub image_formats: &'a HashMap<String, String>,
    pub sampler_classes: &'a SamplerClasses,
}

/// Transforms one preprocessed stage.
pub(crate) fn transform(
    pp: &Preprocessed,
    stage: StageKind,
    iface: &mut ProgramInterface,
    names: &ResourceNames<'_>,
) -> Result<StageDraft, TransformError> {
    let lg = legacy::rewrite(&pp.tokens, stage, pp.version)?;
    let mut items: Vec<Item> = vec![];
    let mut user_funcs: HashSet<String> = HashSet::new();
    for it in split_items(&lg.toks) {
        let first_brace = it.iter().position(|t| t.is("{"));
        let is_func = it.last().is_some_and(|t| t.is("}"))
            && first_brace.is_some_and(|b| b > 0 && it[b - 1].is(")"));
        let lp = it.iter().position(|t| t.is("("));
        let fname = lp
            .filter(|lp| *lp > 0)
            .and_then(|lp| it[lp - 1].ident())
            .map(str::to_string);
        if is_func && let Some(name) = fname {
            user_funcs.insert(name.clone());
            items.push(Item::Func { name, toks: it });
        } else if let Some(d) = parse_decl(&it) {
            items.push(Item::Decl(d, it));
        } else {
            if let Some(name) = fname {
                user_funcs.insert(name);
            }
            items.push(Item::Other(it));
        }
    }
    let items = specialize::specialize_opaque_params(items);
    let mut c = declare::collect(items, stage, iface, names)?;

    let mut ubo: Vec<UboMember> = LEGACY_UNIFORMS
        .iter()
        .filter(|(g, _, _)| lg.builtins.contains(*g))
        .map(|(_, m, decl)| UboMember::new(m, lex(decl)))
        .collect();
    ubo.append(&mut c.ubo);

    let mut builtin_io = String::new();
    if stage == StageKind::Vertex {
        // Inputs Iris supplies without the pack declaring them.
        for (n, ty) in [("mc_chunkFade", "float"), ("dhMaterialId", "int")] {
            if !c.declared.contains(n) && lg.toks.iter().any(|t| t.ident() == Some(n)) {
                vertex_input(&mut builtin_io, &mut c.vertex_inputs, n, ty);
            }
        }
        for (g, n, ty) in [
            ("gl_Vertex", "iris_Vertex", "vec4"),
            ("gl_Color", "iris_Color", "vec4"),
            ("gl_Normal", "iris_Normal", "vec3"),
            ("gl_MultiTexCoord0", "iris_MultiTexCoord0", "vec4"),
            ("gl_MultiTexCoord1", "iris_MultiTexCoord1", "vec4"),
            ("gl_MultiTexCoord2", "iris_MultiTexCoord2", "vec4"),
            ("gl_MultiTexCoord3", "iris_MultiTexCoord3", "vec4"),
        ] {
            if lg.builtins.contains(g) {
                vertex_input(&mut builtin_io, &mut c.vertex_inputs, n, ty);
            }
        }
    }
    let varying_dir = match stage {
        StageKind::Vertex => Some("out"),
        StageKind::Fragment => Some("in"),
        _ => None,
    };
    if let Some(dir) = varying_dir {
        let front = (stage == StageKind::Vertex && lg.builtins.contains("gl_FrontColor"))
            || (stage == StageKind::Fragment && lg.builtins.contains("gl_Color"));
        let mut varying = |name: &str, ty: &str| {
            let loc = iface.varying(stage, name, 1, None);
            builtin_io.push_str(&format!("layout(location = {loc}) {dir} {ty} {name};\n"));
        };
        if front {
            varying("iris_FrontColor", "vec4");
        }
        if lg.builtins.contains("gl_FogFragCoord") {
            varying("iris_FogFragCoord", "float");
        }
        for t in &lg.tex_coords {
            varying(&format!("iris_TexCoord{t}"), "vec4");
        }
    }
    if stage == StageKind::Fragment {
        for f in &lg.frag_data {
            if c.frag_locations.contains(f) {
                return Err(TransformError::new(
                    format!("gl_FragData[{f}] collides with a declared output"),
                    0,
                ));
            }
            let name = format!("iris_FragData{f}");
            builtin_io.push_str(&format!("layout(location = {f}) out vec4 {name};\n"));
            c.frag_outputs.push((*f, name));
        }
    }

    let mut used = SamplerUse::default();
    let mut out_body: Vec<Tok> = vec![];
    let wrapper =
        !c.after_main.is_empty() || !c.before_main.is_empty() || !c.global_inits.is_empty();
    for item in std::mem::take(&mut c.body) {
        match item {
            Item::Func { name, toks } => {
                let brace = toks.iter().position(|t| t.is("{")).unwrap_or(toks.len());
                let mut local = c.samplers.clone();
                let mut bools = c.bools.clone();
                if let Some((_, params)) = func_params(&toks) {
                    for param in params {
                        local.remove(&param.name);
                        bools.remove(&param.name);
                    }
                }
                let mut header = rewrite_params(&toks[..brace], &mut local);
                if name == "main"
                    && wrapper
                    && let Some(lp) = header.iter().position(|t| t.is("("))
                    && lp > 0
                {
                    header[lp - 1] = crate::token::id("iris_user_main").at(header[lp - 1].line);
                }
                out_body.extend(header);
                let cx = BodyContext {
                    samplers: &local,
                    bools: &bools,
                    user_funcs: &user_funcs,
                };
                out_body.extend(rewrite_body(&toks[brace..], &cx, &mut used));
            }
            Item::Other(toks)
                if func_params(&toks).is_some()
                    && toks.last().is_some_and(|token| token.is(";")) =>
            {
                out_body.extend(rewrite_params(&toks, &mut HashMap::new()));
            }
            Item::Decl(_, toks) | Item::Other(toks) => {
                let cx = BodyContext {
                    samplers: &c.samplers,
                    bools: &c.bools,
                    user_funcs: &user_funcs,
                };
                out_body.extend(rewrite_body(&toks, &cx, &mut used));
            }
        }
    }
    if wrapper {
        let mut w = String::from("void main() {\n");
        for s in &c.before_main {
            w.push_str(s);
            w.push('\n');
        }
        out_body.extend(lex(&w));
        let cx = BodyContext {
            samplers: &c.samplers,
            bools: &c.bools,
            user_funcs: &user_funcs,
        };
        out_body.extend(rewrite_body(&c.global_inits, &cx, &mut used));
        let mut w = String::from("iris_user_main();\n");
        for s in &c.after_main {
            w.push_str(s);
            w.push('\n');
        }
        w.push('}');
        out_body.extend(lex(&w));
    }

    let mut header = String::from("#version 450\n");
    for e in &pp.extensions {
        // Everything else the packs enable is core in GLSL 4.50.
        if e.contains("GL_KHR_shader_subgroup") {
            header.push_str(&format!("#extension {e}\n"));
        }
    }
    let mut resource_names = c.resource_names;
    for class in &used.filtering {
        resource_names.push((
            TEXTURE_GROUP,
            class.binding(),
            class.glsl_name().to_string(),
        ));
    }
    if used.comparison {
        resource_names.push((
            TEXTURE_GROUP,
            SHADOW_SAMPLER_BINDING,
            SHADOW_SAMPLER_NAME.to_string(),
        ));
    }
    Ok(StageDraft {
        header,
        hoisted: c.hoisted,
        ubo,
        resources: c.resources,
        samplers: used,
        builtin_io,
        io: c.io,
        split_globals: c.split_globals,
        body: out_body,
        vertex_inputs: c.vertex_inputs,
        frag_outputs: c.frag_outputs,
        resource_names,
    })
}

/// Emits a supplied vertex input at its fixed host location.
fn vertex_input(io: &mut String, inputs: &mut Vec<(u32, String)>, name: &str, ty: &str) {
    if let Some(loc) = VertexAttribute::from_glsl_name(name).location() {
        io.push_str(&format!("layout(location = {loc}) in {ty} {name};\n"));
        inputs.push((loc, name.to_string()));
    }
}

#[cfg(test)]
mod tests;
