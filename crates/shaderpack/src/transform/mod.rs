//! Compatibility-profile / GLSL 1.20 → Vulkan-flavoured GLSL 4.50 for naga's GLSL frontend.
//!
//! Works on the fully preprocessed token stream (no macros left), so a light parser over
//! top-level declarations is enough; function bodies are only rewritten at identifier level.
//! Each stage becomes a [`StageDraft`]; a program's drafts are assembled with one shared
//! loose-uniform block so every stage binds the same layout.

mod assemble;
mod body;
mod builtins;
mod declare;
mod dot;
mod interface;
mod items;
mod legacy;
mod math;
mod missing;
mod narrow;
mod shared;
mod specialize;

use crate::preprocess::Preprocessed;
use crate::programs::StageKind;
use crate::reflection::{
    LEGACY_UNIFORMS, SHADOW_SAMPLER_BINDING, SHADOW_SAMPLER_NAME, SamplerClass, TEXTURE_GROUP,
    TextureDimension,
};
use crate::token::{Tok, lex};
pub(crate) use assemble::{UboMember, assemble, merge_uniforms};
use body::{BodyContext, SamplerUse, convert_returns, return_type, rewrite_body, rewrite_params};
pub(crate) use interface::ProgramInterface;
use items::{Item, func_params, parse_decl, split_items};
pub(crate) use missing::vertex_outputs;
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

/// Converts the vertex stage's OpenGL clip position to wgpu's: depth `[-w, w]` becomes
/// `[0, w]`, so window depth keeps GL's `(z + 1) / 2`, and Y flips, so rows are stored
/// bottom-up exactly as GL stores them. Texture coordinates, `gl_FragCoord` and
/// `texelFetch` then agree with the pack's GL assumptions; hosts flip Y when copying
/// images into or out of pack buffers, and triangle winding is mirrored.
const GL_CLIP_TO_WGPU: &str = "gl_Position = vec4(gl_Position.x, -gl_Position.y, (gl_Position.z + gl_Position.w) * 0.5, gl_Position.w);";

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
    validate_delimiters(&pp.tokens)?;
    let wide = narrow::lower(&pp.tokens);
    let wide = wide.as_deref().unwrap_or(&pp.tokens);
    let fused = dot::order(wide);
    let lg = legacy::rewrite(fused.as_deref().unwrap_or(wide), stage, pp.version)?;
    let mut items: Vec<Item> = vec![];
    let mut user_funcs: HashSet<String> = HashSet::new();
    for it in split_items(&lg.toks) {
        if it.len() == 1 && it[0].is(";") {
            continue;
        }
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
    math::defer_negative_sqrt(&mut items, &user_funcs, &lg.toks);
    let items = shared::split_declarations(items);
    let items = specialize::specialize_opaque_params(items)?;
    let mut c = declare::collect(items, stage, iface, names)?;
    if stage == StageKind::Vertex {
        c.after_main.push(GL_CLIP_TO_WGPU.to_string());
    }

    let mut ubo: Vec<UboMember> = LEGACY_UNIFORMS
        .iter()
        .filter(|(g, _, _)| lg.builtins.contains(*g))
        .map(|(_, m, decl)| UboMember::new(m, lex(decl)))
        .collect();
    ubo.append(&mut c.ubo);

    let builtin_io = builtins::declarations(stage, &lg, &mut c, iface)?;

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
                let return_ty = return_type(&header);
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
                out_body.extend(convert_returns(
                    rewrite_body(&toks[brace..], &cx, &mut used),
                    &return_ty,
                ));
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

/// Rejects unbalanced token groups before a compatibility parser inspects their contents.
fn validate_delimiters(tokens: &[Tok]) -> Result<(), TransformError> {
    let mut delimiters = Vec::new();
    for token in tokens {
        if token.is("(") || token.is("[") || token.is("{") {
            delimiters.push(token);
        } else if token.is(")") || token.is("]") || token.is("}") {
            let expected = match token.text() {
                ")" => "(",
                "]" => "[",
                _ => "{",
            };
            if !delimiters.pop().is_some_and(|open| open.is(expected)) {
                return Err(TransformError::new(
                    "unmatched closing delimiter",
                    token.line,
                ));
            }
        }
    }
    if let Some(open) = delimiters.last() {
        return Err(TransformError::new("unclosed delimiter", open.line));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
