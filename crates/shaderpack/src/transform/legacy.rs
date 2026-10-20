//! Token-level rewrites of compatibility-profile and GLSL 1.20 constructs.

use super::TransformError;
use super::items::{
    func_params, matching, parse_decl, shadowed_bindings, size_unsized_arrays, split_args,
    split_items,
};
use crate::programs::StageKind;
use crate::token::{Tok, TokKind, id, int, lex, p};
use std::collections::{BTreeSet, HashSet};

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

/// Core-profile names and the compatibility built-ins Iris's core-profile transformer
/// renames them to. Iris compiles line programs with that transformer whatever their
/// version, so Complementary's `#version 130` `gbuffers_line` uses these without declaring
/// them. Vertex attributes only apply to vertex stages.
const CORE_NAMES: &[(&str, &str, bool)] = &[
    ("modelViewMatrix", "gl_ModelViewMatrix", false),
    ("modelViewMatrixInverse", "gl_ModelViewMatrixInverse", false),
    ("projectionMatrix", "gl_ProjectionMatrix", false),
    (
        "projectionMatrixInverse",
        "gl_ProjectionMatrixInverse",
        false,
    ),
    ("normalMatrix", "gl_NormalMatrix", false),
    ("vaPosition", "gl_Vertex.xyz", true),
    ("vaColor", "gl_Color", true),
    ("vaNormal", "gl_Normal", true),
    ("vaUV0", "gl_MultiTexCoord0.xy", true),
    ("vaUV2", "ivec2(gl_MultiTexCoord1.xy)", true),
];

/// Replaces undeclared [`CORE_NAMES`] with their compatibility built-ins, so a stage that
/// uses both (`gl_Normal` and `vaNormal`) reads one input. A declared name stays the
/// pack's own variable.
fn undeclared_core_names(src: &[Tok], stage: StageKind) -> Option<Vec<Tok>> {
    let mut declared = HashSet::new();
    let mut shadowed = shadowed_bindings(src, CORE_NAMES.iter().map(|(name, _, _)| *name));
    let mut offset = 0;
    for item in split_items(src) {
        if let Some(declaration) = parse_decl(&item) {
            declared.extend(declaration.decls.into_iter().map(|(name, _, _)| name));
            if let Some((name, _, instance)) = declaration.block {
                declared.insert(name.clone());
                let mut tokens = vec![id(&name)];
                tokens.extend(instance);
                tokens.push(p(";"));
                if let Some(instances) = parse_decl(&tokens) {
                    declared.extend(instances.decls.into_iter().map(|(name, _, _)| name));
                }
            }
        } else if let Some((lp, params)) = func_params(&item) {
            if let Some(name) = lp.checked_sub(1).and_then(|index| item[index].ident()) {
                declared.insert(name.to_string());
            }
            let names: HashSet<_> = params.into_iter().map(|param| param.name).collect();
            shadowed.extend(
                item.iter()
                    .enumerate()
                    .filter(|(_, token)| token.ident().is_some_and(|name| names.contains(name)))
                    .map(|(i, _)| offset + i),
            );
        }
        offset += item.len();
    }
    let replace: Vec<(&str, &str)> = CORE_NAMES
        .iter()
        .filter(|(n, _, attribute)| {
            (!attribute || stage == StageKind::Vertex)
                && src.iter().any(|t| t.ident() == Some(n))
                && !declared.contains(*n)
        })
        .map(|(n, compat, _)| (*n, *compat))
        .collect();
    if replace.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(src.len());
    for (i, t) in src.iter().enumerate() {
        let member = i > 0 && src[i - 1].is(".");
        match replace
            .iter()
            .find(|(n, _)| !member && !shadowed.contains(&i) && t.ident() == Some(n))
        {
            Some((_, compat)) => {
                out.push(p("(").at(t.line));
                out.extend(lex(compat).into_iter().map(|c| c.at(t.line)));
                out.push(p(")").at(t.line));
            }
            None => out.push(t.clone()),
        }
    }
    Some(out)
}

/// GLSL shifts a signed literal into and past the sign bit (Rethinking Voxels marks texels
/// with `uvec4(1<<31)`); naga's constant evaluator follows WGSL and rejects the overflow.
/// A literal shift that overflows, between operators that bind more loosely than `<<`,
/// becomes its wrapped value.
fn fold_sign_shifts(src: &[Tok]) -> Option<Vec<Tok>> {
    let loose_before = |t: &Tok| {
        t.ident() == Some("return")
            || [
                "(", ",", "=", "{", "[", "?", ":", ";", "&", "|", "^", "&&", "||", "==", "!=", "<",
                ">", "<=", ">=",
            ]
            .iter()
            .any(|p| t.is(p))
    };
    let loose_after = |t: &Tok| {
        [
            ")", ",", ";", "]", "}", "?", ":", "&", "|", "^", "&&", "||", "==", "!=", "<", ">",
            "<=", ">=",
        ]
        .iter()
        .any(|p| t.is(p))
    };
    let signed = |t: &Tok| match &t.kind {
        TokKind::Int(s) => s.parse::<i64>().ok(),
        _ => None,
    };
    let fold = |i: usize| {
        (i > 0 && i + 3 < src.len())
            .then_some(())
            .filter(|()| src[i + 1].is("<<") && loose_before(&src[i - 1]))
            .filter(|()| loose_after(&src[i + 3]))
            .and_then(|()| Some((signed(&src[i])?, signed(&src[i + 2])?)))
            .filter(|&(value, shift)| (0..32).contains(&shift) && value >= 0)
            .filter(|&(value, shift)| value << shift > i64::from(i32::MAX))
            .map(|(value, shift)| (value as i32).wrapping_shl(shift as u32))
    };
    let first = (0..src.len()).find(|&i| fold(i).is_some())?;
    let mut out = src[..first].to_vec();
    let mut i = first;
    while i < src.len() {
        match fold(i) {
            Some(wrapped) => {
                // Large signed literals are spelled in hex, as the preprocessor does.
                out.push(int(format!("0x{:X}", wrapped as u32)).at(src[i].line));
                i += 3;
            }
            None => {
                out.push(src[i].clone());
                i += 1;
            }
        }
    }
    Some(out)
}

/// Renames reserved identifiers, maps legacy built-ins and lookup functions, and gives
/// implicit-LOD sampling outside fragment shaders an explicit LOD.
pub(super) fn rewrite(
    src: &[Tok],
    stage: StageKind,
    version: u32,
) -> Result<Legacy, TransformError> {
    let legacy = version < 130;
    let mut sized = size_unsized_arrays(src);
    if let Some(core) = undeclared_core_names(&sized, stage) {
        sized = core;
    }
    if let Some(folded) = fold_sign_shifts(&sized) {
        sized = folded;
    }
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
        // Iris renames a non-call `texture` to `gtexture` in every version (SEUS Renewed's
        // `#version 130` gbuffers declare `uniform sampler2D texture`).
        if name == "texture" && !called {
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
            let close = matching(src, i + 1, "(", ")");
            // As for `texture`: a compare sample needs an explicit LOD outside fragment
            // shaders (Rethinking Voxels' compute `deferred1` filters shadows).
            let explicit_lod = name == "shadow2D"
                && stage != StageKind::Fragment
                && split_args(&src[i + 2..close]).len() == 2;
            let f = match name {
                "shadow2DLod" => "textureLod",
                "shadow2DProj" => "textureProj",
                _ if explicit_lod => "textureLod",
                _ => "texture",
            };
            if explicit_lod {
                pending_lod.push(close);
            }
            // shadow2D returned vec4 in 1.20; the extra `)` follows the call's own.
            pending_close.push(close);
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
