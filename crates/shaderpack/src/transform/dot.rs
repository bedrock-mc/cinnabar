//! `dot` rounded the way the reference OpenGL driver rounds it.
//!
//! GLSL leaves the rounding of `dot` to the driver. NVIDIA's OpenGL driver, which runs the
//! Java Iris reference, accumulates the component products with fused multiply-adds from x
//! upward: `dot(a, b)` is `fma(a.y, b.y, a.x * b.x)`; its Vulkan driver fuses the vec2 case
//! the other way round. Hashes such as BSL's `fract(sin(dot(p, k)) * 43758.5453)` turn that
//! last-bit difference into an unrelated value, so pack noise (BSL's waving foliage) came out
//! as a different pattern. Calls in function bodies therefore go through `iris_dot` helpers
//! spelled with explicit `fma`. Constant expressions keep the built-in: naga folds those
//! itself and cannot call functions there.

use crate::token::{Tok, id, lex};

const HELPERS: &str = "\
float iris_dot(float a, float b) { return a * b; }
float iris_dot(vec2 a, vec2 b) { return fma(a.y, b.y, a.x * b.x); }
float iris_dot(vec3 a, vec3 b) { return fma(a.z, b.z, fma(a.y, b.y, a.x * b.x)); }
float iris_dot(vec4 a, vec4 b) { return fma(a.w, b.w, fma(a.z, b.z, fma(a.y, b.y, a.x * b.x))); }
";

/// Routes `dot` calls in function bodies through the `iris_dot` helpers, or `None` when the
/// stage has none to route. Stages that use double precision keep the built-in: the helpers
/// only have single-precision overloads.
pub(super) fn order(src: &[Tok]) -> Option<Vec<Tok>> {
    if src.iter().any(|t| {
        t.ident()
            .is_some_and(|n| n == "double" || n.starts_with("dvec"))
    }) {
        return None;
    }
    let constant = super::items::constant_contexts(src);
    let identifiers: std::collections::HashSet<_> = src.iter().filter_map(Tok::ident).collect();
    let mut helper = "iris_dot".to_string();
    while identifiers.contains(helper.as_str()) {
        helper.push('_');
    }
    let items = super::items::split_items(src);
    let mut user_types = std::collections::HashSet::new();
    let mut rewritten = Vec::new();
    let mut routed = false;
    let mut offset = 0;
    for item in &items {
        let body = item.iter().position(|token| token.is("{"));
        let is_function = body.is_some_and(|b| b > 0 && item[b - 1].is(")"));
        let params = (is_function || super::items::parse_decl(item).is_none())
            .then(|| super::items::func_params(item))
            .flatten();
        if let Some((lp, params)) = &params
            && *lp > 0
            && item[*lp - 1].ident() == Some("dot")
        {
            user_types.insert(signature(params));
        }
        let mut output = item.clone();
        if is_function {
            for i in body.unwrap() + 1..item.len() {
                if item[i].ident() == Some("dot")
                    && item.get(i + 1).is_some_and(|token| token.is("("))
                    && !item[i - 1].is(".")
                    && !constant.contains(&(offset + i))
                {
                    output[i] = id(&helper).at(item[i].line);
                    routed = true;
                }
            }
        }
        // Resolve the renamed user overloads and fused built-ins in the same overload set.
        if let Some((lp, _)) = params
            && lp > 0
            && item[lp - 1].ident() == Some("dot")
        {
            output[lp - 1] = id(&helper).at(item[lp - 1].line);
        }
        rewritten.extend(output);
        offset += item.len();
    }
    if !routed {
        return None;
    }
    let mut out = Vec::new();
    for item in super::items::split_items(&lex(HELPERS)) {
        let (_, params) = super::items::func_params(&item).unwrap();
        let types = signature(&params);
        if !user_types.contains(&types) {
            out.extend(item.into_iter().map(|token| {
                if token.ident() == Some("iris_dot") {
                    id(&helper).at(token.line)
                } else {
                    token
                }
            }));
        }
    }
    out.extend(rewritten);
    Some(out)
}

/// Identifies overload parameter types, including array dimensions but excluding names and qualifiers.
fn signature(params: &[super::items::Param]) -> Vec<String> {
    params
        .iter()
        .map(|param| {
            param
                .toks
                .iter()
                .filter(|token| {
                    !token.ident().is_some_and(|name| {
                        name == param.name || super::items::QUALIFIERS.contains(&name)
                    })
                })
                .map(Tok::text)
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Emitter;

    /// Reorders a fixture expression and emits its resulting token stream.
    fn ordered(src: &str) -> Option<String> {
        let mut e = Emitter::default();
        e.tokens(&order(&lex(src))?);
        Some(e.finish().0)
    }

    #[test]
    fn body_calls_use_the_fused_helpers() {
        let out = ordered(
            "float h(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 4.1414))) * 43758.5453); }",
        )
        .expect("routes the call");
        assert!(out.contains("sin(iris_dot(p, vec2"), "{out}");
        assert!(out.contains("float iris_dot(vec2 a, vec2 b)"), "{out}");
        assert!(out.contains("return fma(a.y, b.y, a.x * b.x);"), "{out}");
    }

    #[test]
    fn constant_expressions_keep_the_built_in() {
        let out = ordered(
            "const float k = dot(vec2(1.0), vec2(2.0)); \
             void main() { const float c = dot(vec2(1.0), vec2(3.0)); float v = dot(vec3(c), vec3(k)); }",
        )
        .expect("routes the non-constant call");
        assert!(out.contains("k = dot("), "{out}");
        assert!(out.contains("c = dot("), "{out}");
        assert!(out.contains("v = iris_dot("), "{out}");
    }

    #[test]
    fn stages_without_body_calls_or_with_doubles_are_untouched() {
        assert!(ordered("const float k = dot(vec2(1.0), vec2(2.0)); void main() {}").is_none());
        assert!(ordered("void main() { dvec2 a = dvec2(1.0); double d = dot(a, a); }").is_none());
    }
}
