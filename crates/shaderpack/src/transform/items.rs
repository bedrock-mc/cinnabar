//! Top-level item splitting, the light declaration parser, and GLSL type helpers.

use crate::token::{Tok, TokKind, int};

/// Storage, interpolation and precision qualifiers recognised before a declaration's type.
pub(super) const QUALIFIERS: &[&str] = &[
    "const",
    "uniform",
    "in",
    "out",
    "inout",
    "varying",
    "attribute",
    "flat",
    "smooth",
    "noperspective",
    "centroid",
    "invariant",
    "buffer",
    "shared",
    "readonly",
    "writeonly",
    "coherent",
    "volatile",
    "restrict",
    "highp",
    "mediump",
    "lowp",
    "precise",
    "patch",
    "sample",
];

/// A top-level declaration: `layout(...) quals type[arr] a[arr] = init, b;`, a block
/// `quals Name { members } inst;`, or a struct definition.
#[derive(Debug, Clone)]
pub(super) struct Decl {
    pub layout: Vec<Tok>,
    pub quals: Vec<String>,
    /// The type name; `"struct"` / `"block"` for those forms; empty for `layout(...) in;`.
    pub ty: String,
    pub ty_arr: Vec<Tok>,
    /// Declarators: (name, array tokens including brackets, initializer tokens).
    pub decls: Vec<(String, Vec<Tok>, Vec<Tok>)>,
    /// Struct or block: (name, member tokens, instance tokens).
    pub block: Option<(String, Vec<Tok>, Vec<Tok>)>,
}

impl Decl {
    /// Reports whether the declaration carries the requested qualifier.
    pub fn has(&self, q: &str) -> bool {
        self.quals.iter().any(|x| x == q)
    }
}

/// A top-level item of the preprocessed stream.
pub(super) enum Item {
    Func { name: String, toks: Vec<Tok> },
    Decl(Decl, Vec<Tok>),
    Other(Vec<Tok>),
}

/// Splits the token stream into top-level statements and function definitions.
pub(super) fn split_items(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut items = vec![];
    let mut cur: Vec<Tok> = vec![];
    let mut brace = 0i32;
    let mut func = false;
    for t in toks {
        cur.push(t.clone());
        if t.is("{") {
            if brace == 0 && cur.len() >= 2 && cur[cur.len() - 2].is(")") {
                func = true;
            }
            brace += 1;
        } else if t.is("}") {
            brace -= 1;
            if brace == 0 && func {
                items.push(std::mem::take(&mut cur));
                func = false;
            }
        } else if t.is(";") && brace == 0 {
            items.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        items.push(cur);
    }
    items
}

/// Index of the token closing the bracket opened at `open_idx` (last index if unbalanced).
pub(super) fn matching(toks: &[Tok], open_idx: usize, open: &str, close: &str) -> usize {
    let mut d = 0;
    for (i, t) in toks.iter().enumerate().skip(open_idx) {
        if t.is(open) {
            d += 1;
        } else if t.is(close) {
            d -= 1;
            if d == 0 {
                return i;
            }
        }
    }
    toks.len().saturating_sub(1)
}

/// Splits call arguments at top-level commas.
pub(super) fn split_args(toks: &[Tok]) -> Vec<Vec<Tok>> {
    let mut out: Vec<Vec<Tok>> = vec![];
    let mut cur = vec![];
    let mut depth = 0;
    for t in toks {
        if t.is("(") || t.is("[") || t.is("{") {
            depth += 1;
        } else if t.is(")") || t.is("]") || t.is("}") {
            depth -= 1;
        } else if t.is(",") && depth == 0 {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(t.clone());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A function parameter.
#[derive(Clone)]
pub(super) struct Param {
    pub toks: Vec<Tok>,
    pub opaque: bool,
    pub name: String,
}

/// The `(` index and parameters of a function definition's tokens.
pub(super) fn func_params(toks: &[Tok]) -> Option<(usize, Vec<Param>)> {
    let lp = toks.iter().position(|t| t.is("("))?;
    let rp = matching(toks, lp, "(", ")");
    let mut params = vec![];
    for a in split_args(&toks[lp + 1..rp]) {
        if a.len() == 1 && a[0].ident() == Some("void") {
            continue;
        }
        let opaque = a.iter().any(|t| t.ident().is_some_and(is_opaque));
        let name = a
            .iter()
            .rev()
            .find_map(|t| t.ident())
            .unwrap_or("")
            .to_string();
        params.push(Param {
            toks: a,
            opaque,
            name,
        });
    }
    Some((lp, params))
}

/// Gives `T[] x = T[](a, b)` / `T x[] = T[](...)` explicit sizes. GLSL infers them from the
/// initializer; naga's GLSL frontend would make a runtime-sized array instead.
pub(super) fn size_unsized_arrays(src: &[Tok]) -> Vec<Tok> {
    let ctor_len = |open: usize| -> usize {
        let close = matching(src, open, "(", ")");
        split_args(&src[open + 1..close]).len()
    };
    let mut out = Vec::with_capacity(src.len());
    for i in 0..src.len() {
        out.push(src[i].clone());
        if !(src[i].is("[") && src.get(i + 1).is_some_and(|t| t.is("]"))) {
            continue;
        }
        if src.get(i + 2).is_some_and(|t| t.is("(")) {
            out.push(int(ctor_len(i + 2)).at(src[i].line));
            continue;
        }
        let mut j = i + 2;
        while j + 3 < src.len() && !src[j].is(";") {
            if src[j].is("=")
                && src[j + 2].is("[")
                && src[j + 3].is("]")
                && src.get(j + 4).is_some_and(|t| t.is("("))
            {
                out.push(int(ctor_len(j + 4)).at(src[i].line));
                break;
            }
            j += 1;
        }
    }
    out
}

/// Parses one top-level statement as a declaration; `None` for prototypes and anything else.
pub(super) fn parse_decl(toks: &[Tok]) -> Option<Decl> {
    let mut i = 0;
    let mut layout = vec![];
    let mut quals = vec![];
    loop {
        match toks.get(i)?.ident() {
            Some("layout") => {
                let close = matching(toks, i + 1, "(", ")");
                layout.extend_from_slice(toks.get(i + 2..close)?);
                i = close + 1;
            }
            Some(q) if QUALIFIERS.contains(&q) => {
                quals.push(q.to_string());
                i += 1;
            }
            _ => break,
        }
    }
    let empty = |ty: &str, block| Decl {
        layout: layout.clone(),
        quals: quals.clone(),
        ty: ty.to_string(),
        ty_arr: vec![],
        decls: vec![],
        block,
    };
    if toks.get(i)?.is(";") {
        return Some(empty("", None));
    }
    let last = toks.len() - 1;
    if toks[i].ident() == Some("struct") {
        let name = toks.get(i + 1)?.ident()?.to_string();
        if !toks.get(i + 2)?.is("{") {
            return None;
        }
        let close = matching(toks, i + 2, "{", "}");
        let members = toks.get(i + 3..close)?.to_vec();
        let inst = toks.get(close + 1..last)?.to_vec();
        return Some(empty("struct", Some((name, members, inst))));
    }
    let ty = toks[i].ident()?.to_string();
    i += 1;
    if toks.get(i)?.is("{") {
        let close = matching(toks, i, "{", "}");
        let members = toks.get(i + 1..close)?.to_vec();
        let inst = toks.get(close + 1..last)?.to_vec();
        return Some(empty("block", Some((ty, members, inst))));
    }
    let mut ty_arr = vec![];
    while toks.get(i)?.is("[") {
        let close = matching(toks, i, "[", "]");
        ty_arr.extend_from_slice(&toks[i..=close]);
        i = close + 1;
    }
    let mut decls = vec![];
    while i < last {
        let name = toks[i].ident()?.to_string();
        i += 1;
        if toks.get(i)?.is("(") {
            return None;
        }
        let mut arr = vec![];
        while toks.get(i)?.is("[") {
            let close = matching(toks, i, "[", "]");
            arr.extend_from_slice(&toks[i..=close]);
            i = close + 1;
        }
        let mut init = vec![];
        if toks.get(i)?.is("=") {
            i += 1;
            let mut depth = 0;
            while i < last {
                let t = &toks[i];
                if t.is("(") || t.is("[") || t.is("{") {
                    depth += 1;
                } else if t.is(")") || t.is("]") || t.is("}") {
                    depth -= 1;
                } else if t.is(",") && depth == 0 {
                    break;
                }
                init.push(t.clone());
                i += 1;
            }
        }
        decls.push((name, arr, init));
        if toks.get(i)?.is(",") {
            i += 1;
        }
    }
    Some(Decl {
        layout,
        quals,
        ty,
        ty_arr,
        decls,
        block: None,
    })
}

/// Length of a single `[N]` array suffix (1 when empty).
pub(super) fn arr_len(arr: &[Tok]) -> Option<u32> {
    if arr.is_empty() {
        return Some(1);
    }
    if arr.len() == 3
        && let TokKind::Int(s) = &arr[1].kind
    {
        return s.trim_end_matches(['u', 'U']).parse().ok();
    }
    None
}

/// The separate texture type for a combined sampler type, and whether it compares depth.
pub(super) fn sampler_info(ty: &str) -> Option<(&'static str, bool)> {
    Some(match ty {
        "sampler1D" => ("texture1D", false),
        "sampler2D" => ("texture2D", false),
        "sampler3D" => ("texture3D", false),
        "samplerCube" => ("textureCube", false),
        "sampler1DArray" => ("texture1DArray", false),
        "sampler2DArray" => ("texture2DArray", false),
        "samplerCubeArray" => ("textureCubeArray", false),
        "sampler2DMS" => ("texture2DMS", false),
        "sampler2DRect" => ("texture2D", false),
        "sampler2DShadow" => ("texture2D", true),
        "sampler2DArrayShadow" => ("texture2DArray", true),
        "samplerCubeShadow" => ("textureCube", true),
        "isampler1D" => ("itexture1D", false),
        "usampler1D" => ("utexture1D", false),
        "isampler2D" => ("itexture2D", false),
        "usampler2D" => ("utexture2D", false),
        "isampler3D" => ("itexture3D", false),
        "usampler3D" => ("utexture3D", false),
        "isampler1DArray" => ("itexture1DArray", false),
        "usampler1DArray" => ("utexture1DArray", false),
        "isampler2DArray" => ("itexture2DArray", false),
        "usampler2DArray" => ("utexture2DArray", false),
        _ => return None,
    })
}

/// Recognizes GLSL storage image types.
pub(super) fn is_image(ty: &str) -> bool {
    ty.starts_with("image") || ty.starts_with("iimage") || ty.starts_with("uimage")
}

/// Recognizes sampler and image types that require resource bindings.
pub(super) fn is_opaque(ty: &str) -> bool {
    sampler_info(ty).is_some() || is_image(ty) || ty.starts_with("sampler") || ty == "atomic_uint"
}

/// Recognizes integer and boolean interface types requiring flat interpolation.
pub(super) fn is_integer(ty: &str) -> bool {
    matches!(ty, "int" | "uint" | "bool")
        || ty.starts_with("ivec")
        || ty.starts_with("uvec")
        || ty.starts_with("bvec")
}

/// Interface locations a value of `ty` occupies (`arr` elements).
pub(super) fn loc_size(ty: &str, arr: u32) -> u32 {
    let base = match ty {
        "dvec3" | "dvec4" => 2,
        t => mat_cols(t).map(|(c, _)| c).unwrap_or(1),
    };
    base * arr.max(1)
}

/// (columns, column type) of a `matC[xR]` type.
pub(super) fn mat_cols(ty: &str) -> Option<(u32, &'static str)> {
    let rest = ty.strip_prefix("mat")?;
    let (c, r) = match rest.split_once('x') {
        Some((c, r)) => (c.parse::<u32>().ok()?, r.parse::<u32>().ok()?),
        None => {
            let n = rest.parse::<u32>().ok()?;
            (n, n)
        }
    };
    let col = match r {
        2 => "vec2",
        3 => "vec3",
        4 => "vec4",
        _ => return None,
    };
    Some((c, col))
}

/// The integer value of `key = N` inside a layout qualifier.
pub(super) fn layout_key(layout: &[Tok], key: &str) -> Option<u32> {
    let i = layout.iter().position(|t| t.ident() == Some(key))?;
    if let (
        Some(e),
        Some(Tok {
            kind: TokKind::Int(n),
            ..
        }),
    ) = (layout.get(i + 1), layout.get(i + 2))
        && e.is("=")
    {
        return n.trim_end_matches(['u', 'U']).parse().ok();
    }
    None
}

/// The layout qualifier without the entries starting with one of `keys`.
pub(super) fn strip_layout_keys(layout: &[Tok], keys: &[&str]) -> Vec<Tok> {
    let mut parts: Vec<Vec<Tok>> = vec![vec![]];
    for t in layout {
        if t.is(",") {
            parts.push(vec![]);
        } else if let Some(last) = parts.last_mut() {
            last.push(t.clone());
        }
    }
    let mut out = vec![];
    for part in parts
        .into_iter()
        .filter(|p| !p.is_empty() && !keys.contains(&p[0].ident().unwrap_or("")))
    {
        if !out.is_empty() {
            out.push(crate::token::p(","));
        }
        out.extend(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    /// Returns the spellings of a fixture token sequence.
    fn texts(toks: &[Tok]) -> Vec<&str> {
        toks.iter().map(Tok::text).collect()
    }

    #[test]
    fn top_level_items_split_at_semicolons_and_function_bodies() {
        let toks =
            lex("uniform float a; struct S { float x; }; void f(int i) { a = 1.0; } out vec4 c;");
        let items: Vec<String> = split_items(&toks)
            .iter()
            .map(|item| texts(item).join(" "))
            .collect();
        assert_eq!(
            items,
            [
                "uniform float a ;",
                "struct S { float x ; } ;",
                "void f ( int i ) { a = 1.0 ; }",
                "out vec4 c ;",
            ]
        );
    }

    #[test]
    fn declarations_keep_layout_qualifiers_arrays_and_initializers() {
        let decl = parse_decl(&lex(
            "layout(location = 2) flat out ivec2 a[4], b = ivec2(1, 2);",
        ))
        .unwrap();
        assert_eq!(layout_key(&decl.layout, "location"), Some(2));
        assert!(decl.has("flat") && decl.has("out"));
        assert_eq!(decl.ty, "ivec2");
        let decls: Vec<_> = decl
            .decls
            .iter()
            .map(|(name, arr, init)| (name.as_str(), arr_len(arr), texts(init).join(" ")))
            .collect();
        assert_eq!(
            decls,
            [
                ("a", Some(4), String::new()),
                ("b", Some(1), "ivec2 ( 1 , 2 )".to_owned()),
            ]
        );

        let block = parse_decl(&lex("uniform Light { vec3 dir; } light;")).unwrap();
        assert_eq!(block.ty, "block");
        let (name, members, inst) = block.block.unwrap();
        assert_eq!((name.as_str(), texts(&inst)), ("Light", vec!["light"]));
        assert_eq!(texts(&members), ["vec3", "dir", ";"]);

        assert!(parse_decl(&lex("vec4 shade(vec3 n);")).is_none());
    }

    #[test]
    fn type_helpers_describe_samplers_and_interface_sizes() {
        assert_eq!(sampler_info("sampler2DShadow"), Some(("texture2D", true)));
        assert_eq!(sampler_info("usampler2D"), Some(("utexture2D", false)));
        assert!(is_opaque("image2D") && is_opaque("sampler2D") && !is_opaque("vec4"));
        assert_eq!(mat_cols("mat3x2"), Some((3, "vec2")));
        assert_eq!(loc_size("mat4", 2), 8);
        assert_eq!(loc_size("dvec4", 1), 2);
        let layout = lex("std430, binding = 3, set = 1");
        assert_eq!(
            texts(&strip_layout_keys(&layout, &["binding", "set"])),
            ["std430"]
        );
    }
}
