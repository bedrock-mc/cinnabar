//! Top-level item splitting, the light declaration parser, and GLSL type helpers.

use crate::token::{Tok, TokKind, int, p};
use std::collections::HashSet;

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
    let mut out = Vec::new();
    let mut start = 0;
    while start < toks.len() {
        let end = assignment_end(toks, start);
        out.push(toks[start..end].to_vec());
        start = end + 1;
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
        let mut declaration = a.clone();
        declaration.push(p(";"));
        let name = parse_decl(&declaration)
            .and_then(|d| d.decls.first().map(|d| d.0.clone()))
            .unwrap_or_default();
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
            let end = assignment_end(toks, i);
            init.extend_from_slice(&toks[i..end]);
            i = end;
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

/// Finds every local declarator with the shared declaration parser, excluding initializer uses.
pub(super) fn local_declarators(toks: &[Tok]) -> HashSet<usize> {
    let mut names = HashSet::new();
    for (_, end, declaration) in declarations(toks) {
        let mut cursor = end;
        for (_, array, initializer) in declaration.decls.iter().rev() {
            cursor -= initializer.len() + usize::from(!initializer.is_empty()) + array.len() + 1;
            names.insert(cursor);
            cursor = cursor.saturating_sub(1);
        }
    }
    names
}

/// Finds declaration statements and their inclusive token ranges with the shared parser.
fn declarations(toks: &[Tok]) -> Vec<(usize, usize, Decl)> {
    let mut declarations = Vec::new();
    for start in 0..toks.len() {
        if start > 0
            && !toks[start - 1].is("{")
            && !toks[start - 1].is(";")
            && !toks[start - 1].is("}")
            && !toks[start - 1].is(":")
            && !(toks[start - 1].is("(") && start > 1 && toks[start - 2].ident() == Some("for"))
        {
            continue;
        }
        let mut depth = 0;
        let end = (start..toks.len()).find(|&index| {
            let token = &toks[index];
            if token.is("(") || token.is("[") {
                depth += 1;
            } else if token.is(")") || token.is("]") {
                depth -= 1;
            }
            depth < 0 || (token.is(";") || token.is("{") || token.is("}")) && depth == 0
        });
        let Some(end) = end.filter(|&end| toks[end].is(";")) else {
            continue;
        };
        let Some(declaration) = parse_decl(&toks[start..=end]) else {
            continue;
        };
        if matches!(declaration.ty.as_str(), "return" | "else" | "do" | "case") {
            continue;
        }
        declarations.push((start, end, declaration));
    }
    declarations
}

/// Marks declaration array bounds, constant initializers, and switch case expressions.
pub(super) fn constant_contexts(tokens: &[Tok]) -> HashSet<usize> {
    let mut constant = HashSet::new();
    for (start, end, declaration) in declarations(tokens) {
        if declaration.has("const") {
            constant.extend(start..end);
            continue;
        }
        // Initializer indexing is runtime; only brackets preceding each initializer are bounds.
        let mut cursor = end;
        for (_, array, initializer) in declaration.decls.iter().rev() {
            cursor -= initializer.len() + usize::from(!initializer.is_empty()) + array.len();
            constant.extend(cursor..cursor + array.len());
            cursor = cursor.saturating_sub(2);
        }
        if !declaration.ty_arr.is_empty()
            && let Some(open) = tokens[start..=end].iter().position(|token| token.is("["))
        {
            constant.extend(start + open..start + open + declaration.ty_arr.len());
        }
    }
    let mut offset = 0;
    for item in split_items(tokens) {
        if parse_decl(&item).is_some() {
            constant.extend(offset..offset + item.len());
        }
        offset += item.len();
    }
    for (i, token) in tokens.iter().enumerate() {
        if token.ident() == Some("case") {
            constant.extend(i..assignment_end(tokens, i + 1));
        }
    }
    constant
}

/// Finds the end of an assignment's right operand, including nested conditional expressions.
pub(super) fn assignment_end(tokens: &[Tok], start: usize) -> usize {
    let mut conditional = 0;
    let mut i = start;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.is("(") || token.is("[") || token.is("{") {
            let close = match token.text() {
                "(" => ")",
                "[" => "]",
                _ => "}",
            };
            i = matching(tokens, i, token.text(), close);
        } else if token.is("?") {
            conditional += 1;
        } else if token.is(":") {
            if conditional == 0 {
                return i;
            }
            conditional -= 1;
        } else if token.is(";")
            || token.is(")")
            || token.is("]")
            || token.is("}")
            || token.is(",") && conditional == 0
        {
            return i;
        }
        i += 1;
    }
    i
}

/// Returns the token after one complete statement, including attached else and do/while parts.
fn statement_end(tokens: &[Tok], start: usize) -> usize {
    enum Part {
        Statement,
        Else,
        While,
    }
    let mut parts = vec![Part::Statement];
    let mut cursor = start;
    while let Some(part) = parts.pop() {
        match part {
            Part::Else => {
                if tokens.get(cursor).and_then(Tok::ident) == Some("else") {
                    cursor += 1;
                    parts.push(Part::Statement);
                }
            }
            Part::While => {
                cursor += 1;
                if tokens.get(cursor).is_some_and(|token| token.is("(")) {
                    cursor = matching(tokens, cursor, "(", ")") + 1;
                }
                cursor += usize::from(tokens.get(cursor).is_some_and(|token| token.is(";")));
            }
            Part::Statement => {
                let Some(token) = tokens.get(cursor) else {
                    return tokens.len();
                };
                if token.is("{") {
                    cursor = matching(tokens, cursor, "{", "}") + 1;
                } else if matches!(token.ident(), Some("if" | "for" | "while" | "switch")) {
                    if token.ident() == Some("if") {
                        parts.push(Part::Else);
                    }
                    cursor = matching(tokens, cursor + 1, "(", ")") + 1;
                    parts.push(Part::Statement);
                } else if token.ident() == Some("do") {
                    cursor += 1;
                    parts.extend([Part::While, Part::Statement]);
                } else {
                    while cursor < tokens.len() {
                        if tokens[cursor].is(";") {
                            cursor += 1;
                            break;
                        }
                        if tokens[cursor].is("(") {
                            cursor = matching(tokens, cursor, "(", ")");
                        } else if tokens[cursor].is("[") {
                            cursor = matching(tokens, cursor, "[", "]");
                        }
                        cursor += 1;
                    }
                }
            }
        }
    }
    cursor
}

/// Identifies declarations and uses that hide the supplied names in their lexical scope.
pub(super) fn shadowed_bindings<'a>(
    tokens: &[Tok],
    names: impl IntoIterator<Item = &'a str>,
) -> std::collections::HashSet<usize> {
    let names: std::collections::HashSet<_> = names.into_iter().collect();
    let declarators = local_declarators(tokens);
    let mut scopes: Vec<(Vec<&str>, usize)> = vec![(vec![], tokens.len() + 1)];
    let mut pending = None;
    let mut depth = 0;
    let mut shadowed = std::collections::HashSet::new();
    for (i, token) in tokens.iter().enumerate() {
        while scopes.last().is_some_and(|(_, end)| *end <= i) {
            scopes.pop();
        }
        if token.ident() == Some("for") && tokens.get(i + 1).is_some_and(|token| token.is("(")) {
            let header_end = matching(tokens, i + 1, "(", ")");
            scopes.push((vec![], statement_end(tokens, header_end + 1)));
        } else if token.is("{") {
            scopes.push((vec![], matching(tokens, i, "{", "}") + 1));
        }
        if token.is("(") {
            depth += 1;
        } else if token.is(")") {
            depth -= 1;
        } else if (token.is(";") || token.is(","))
            && let Some((name, declared_depth)) = pending
            && declared_depth == depth
        {
            scopes.last_mut().unwrap().0.push(name);
            pending = None;
        }
        let Some(name) = token.ident().filter(|name| names.contains(name)) else {
            continue;
        };
        if i > 0 && tokens[i - 1].is(".") {
            continue;
        }
        if declarators.contains(&i) {
            pending = Some((name, depth));
            shadowed.insert(i);
        } else if scopes.iter().any(|(scope, _)| scope.contains(&name)) {
            shadowed.insert(i);
        }
    }
    shadowed
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
    fn loop_binding_ends_at_the_statement_boundary() {
        let tokens = lex(
            "{ for (int enabled = 0; enabled < 2; enabled++) { float value = float(enabled); } bool result = enabled; }",
        );
        let bound = shadowed_bindings(&tokens, ["enabled"]);
        let flags: Vec<_> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.ident() == Some("enabled"))
            .map(|(i, _)| bound.contains(&i))
            .collect();
        assert_eq!(flags, [true, true, true, true, false]);
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
