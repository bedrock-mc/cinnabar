//! Token-level rewrites of compatibility-profile and GLSL 1.20 constructs.

use super::TransformError;
use super::items::{matching, size_unsized_arrays, split_args};
use crate::programs::StageKind;
use crate::token::{Tok, TokKind, id, lex, p};
use std::collections::BTreeSet;

/// Identifiers reserved or built in since GLSL 1.30 that 1.20 code may use as names.
const RESERVED_SINCE_130: &[&str] = &[
    "sample",
    "filter",
    "input",
    "output",
    "common",
    "partition",
    "active",
    "resource",
    "buffer",
    "shared",
    "subroutine",
    "patch",
    "precise",
    "coherent",
    "volatile",
    "restrict",
    "readonly",
    "writeonly",
    "layout",
];

/// Vulkan GLSL type keywords that are plain identifiers in OpenGL GLSL.
const RESERVED_VULKAN: &[&str] = &["sampler", "samplerShadow", "subpassInput"];

/// Result of the legacy pass.
pub(super) struct Legacy {
    pub toks: Vec<Tok>,
    /// Legacy built-ins the stage used (GLSL names such as `gl_ModelViewMatrix`).
    pub builtins: BTreeSet<String>,
    /// `gl_FragData[n]` / `gl_FragColor` indices written.
    pub frag_data: BTreeSet<u32>,
    /// `gl_TexCoord[n]` indices used.
    pub tex_coords: BTreeSet<u32>,
}

/// The modern name of a legacy texture lookup function.
fn texture_function(n: &str) -> Option<&'static str> {
    Some(match n {
        "texture1D" | "texture2D" | "texture3D" | "textureCube" | "texture2DRect"
        | "texture2DArray" | "texture" => "texture",
        "texture1DLod" | "texture2DLod" | "texture3DLod" | "textureCubeLod" | "texture2DLodEXT"
        | "texture2DArrayLod" => "textureLod",
        "texture2DGradARB" | "texture2DGrad" | "texture3DGradARB" | "texture2DGradEXT"
        | "texture3DGrad" => "textureGrad",
        "texture2DProj" | "texture3DProj" => "textureProj",
        "texture2DProjLod" => "textureProjLod",
        "texelFetch2D" | "texelFetch3D" | "texelFetch2DArray" => "texelFetch",
        "textureSize2D" | "textureSize3D" => "textureSize",
        _ => return None,
    })
}

/// Legacy built-in variables and their translated names.
fn builtin_variable(name: &str, stage: StageKind) -> Option<&'static str> {
    Some(match (name, stage) {
        ("gl_Vertex", StageKind::Vertex) => "iris_Vertex",
        ("gl_Color", StageKind::Vertex) => "iris_Color",
        ("gl_Color", StageKind::Fragment) => "iris_FrontColor",
        ("gl_FrontColor", _) => "iris_FrontColor",
        ("gl_Normal", _) => "iris_Normal",
        ("gl_MultiTexCoord0", _) => "iris_MultiTexCoord0",
        ("gl_MultiTexCoord1", _) => "iris_MultiTexCoord1",
        ("gl_MultiTexCoord2", _) => "iris_MultiTexCoord2",
        ("gl_MultiTexCoord3", _) => "iris_MultiTexCoord3",
        ("gl_FogFragCoord", _) => "iris_FogFragCoord",
        ("gl_VertexID", _) => "gl_VertexIndex",
        ("gl_InstanceID", _) => "gl_InstanceIndex",
        _ => {
            return crate::reflection::LEGACY_UNIFORMS
                .iter()
                .find(|(g, _, _)| *g == name)
                .map(|(_, m, _)| *m);
        }
    })
}

/// Renames reserved identifiers, maps legacy built-ins and lookup functions, and gives
/// implicit-LOD sampling outside fragment shaders an explicit LOD.
pub(super) fn rewrite(
    src: &[Tok],
    stage: StageKind,
    version: u32,
) -> Result<Legacy, TransformError> {
    let mut delimiters = Vec::new();
    for token in src {
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
    let legacy = version < 130;
    let sized = size_unsized_arrays(src);
    let src = &sized;
    let mut out = Legacy {
        toks: Vec::with_capacity(src.len()),
        builtins: BTreeSet::new(),
        frag_data: BTreeSet::new(),
        tex_coords: BTreeSet::new(),
    };
    let toks = &mut out.toks;
    let mut pending_lod: Vec<usize> = vec![];
    let mut pending_close: Vec<usize> = vec![];
    let mut i = 0;
    while i < src.len() {
        let t = &src[i];
        let line = t.line;
        let TokKind::Ident(name) = &t.kind else {
            if let Some(pos) = pending_lod.iter().position(|c| *c == i) {
                pending_lod.swap_remove(pos);
                toks.extend([
                    p(",").at(line),
                    Tok {
                        kind: TokKind::Float("0.0".into()),
                        line,
                    },
                ]);
            }
            toks.push(t.clone());
            while let Some(pos) = pending_close.iter().position(|c| *c == i) {
                pending_close.swap_remove(pos);
                toks.push(p(")").at(line));
            }
            i += 1;
            continue;
        };
        let name = name.as_str();
        let called = src.get(i + 1).is_some_and(|n| n.is("("));
        if legacy && name == "texture" && !called {
            toks.push(id("gtexture").at(line));
            i += 1;
            continue;
        }
        if (legacy && RESERVED_SINCE_130.contains(&name)) || RESERVED_VULKAN.contains(&name) {
            toks.push(id(&format!("_r_{name}")).at(line));
            i += 1;
            continue;
        }
        if i > 0 && src[i - 1].is(".") {
            toks.push(t.clone());
            i += 1;
            continue;
        }
        if called && let Some(f) = texture_function(name) {
            let close = matching(src, i + 1, "(", ")");
            // Implicit derivatives only exist in fragment shaders; GL samples LOD 0 elsewhere.
            if f == "texture"
                && stage != StageKind::Fragment
                && split_args(&src[i + 2..close]).len() == 2
            {
                pending_lod.push(close);
                toks.push(id("textureLod").at(line));
            } else {
                toks.push(id(f).at(line));
            }
            i += 1;
            continue;
        }
        if called
            && matches!(
                name,
                "shadow2D" | "shadow2DLod" | "shadow2DProj" | "shadow2DArray"
            )
        {
            let f = match name {
                "shadow2DLod" => "textureLod",
                "shadow2DProj" => "textureProj",
                _ => "texture",
            };
            // shadow2D returned vec4 in 1.20; the extra `)` follows the call's own.
            pending_close.push(matching(src, i + 1, "(", ")"));
            toks.extend([id("vec4").at(line), p("(").at(line), id(f).at(line)]);
            i += 1;
            continue;
        }
        if name == "ftransform" && called {
            for b in ["gl_Vertex", "gl_ModelViewMatrix", "gl_ProjectionMatrix"] {
                out.builtins.insert(b.to_string());
            }
            toks.extend(
                lex("(iris_ProjMat * (iris_ModelViewMat * iris_Vertex))")
                    .into_iter()
                    .map(|t| t.at(line)),
            );
            i += 3;
            continue;
        }
        if name == "gl_ModelViewProjectionMatrix" {
            for b in ["gl_ModelViewMatrix", "gl_ProjectionMatrix"] {
                out.builtins.insert(b.to_string());
            }
            toks.extend(
                lex("(iris_ProjMat * iris_ModelViewMat)")
                    .into_iter()
                    .map(|t| t.at(line)),
            );
            i += 1;
            continue;
        }
        if (name == "gl_FragData" || name == "gl_TexCoord")
            && src.get(i + 1).is_some_and(|n| n.is("["))
        {
            let idx = match (&src.get(i + 2).map(|t| &t.kind), src.get(i + 3)) {
                (Some(TokKind::Int(n)), Some(c)) if c.is("]") => {
                    n.trim_end_matches(['u', 'U']).parse::<u32>().ok()
                }
                _ => None,
            };
            let Some(idx) = idx else {
                return Err(TransformError::new(
                    format!("`{name}` indexed with a non-constant"),
                    line,
                ));
            };
            out.builtins.insert(name.to_string());
            if name == "gl_FragData" {
                out.frag_data.insert(idx);
                toks.push(id(&format!("iris_FragData{idx}")).at(line));
            } else {
                out.tex_coords.insert(idx);
                toks.push(id(&format!("iris_TexCoord{idx}")).at(line));
            }
            i += 4;
            continue;
        }
        if name == "gl_FragColor" {
            out.builtins.insert(name.to_string());
            out.frag_data.insert(0);
            toks.push(id("iris_FragData0").at(line));
            i += 1;
            continue;
        }
        if let Some(r) = builtin_variable(name, stage) {
            out.builtins.insert(name.to_string());
            toks.push(id(r).at(line));
        } else {
            toks.push(t.clone());
        }
        i += 1;
    }
    Ok(out)
}
