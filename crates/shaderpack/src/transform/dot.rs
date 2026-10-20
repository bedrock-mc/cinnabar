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
    let mut out: Vec<Tok> = Vec::with_capacity(src.len());
    let mut routed = false;
    // Brace depth inside the current function body (0 outside one).
    let mut body_depth = 0_u32;
    let mut constant = false;
    for (i, t) in src.iter().enumerate() {
        if t.is("{") && (body_depth > 0 || i > 0 && src[i - 1].is(")")) {
            body_depth += 1;
        } else if t.is("}") && body_depth > 0 {
            body_depth -= 1;
        } else if t.is(";") {
            constant = false;
        } else if t.ident() == Some("const") {
            constant = true;
        }
        let member = i > 0 && src[i - 1].is(".");
        if body_depth > 0
            && !constant
            && !member
            && t.ident() == Some("dot")
            && src.get(i + 1).is_some_and(|n| n.is("("))
        {
            routed = true;
            out.push(id("iris_dot").at(t.line));
        } else {
            out.push(t.clone());
        }
    }
    if !routed {
        return None;
    }
    out.splice(0..0, lex(HELPERS));
    Some(out)
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
