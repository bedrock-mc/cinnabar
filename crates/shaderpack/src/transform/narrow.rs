//! Lowers the 16- and 8-bit types of `GL_NV_gpu_shader5` and
//! `GL_EXT_shader_explicit_arithmetic_types_*` to their 32-bit counterparts.
//!
//! naga has no 16- or 8-bit integers, and its GLSL frontend has no `float16_t`. Packs
//! use these types for register pressure and ALU rate, and fall back to 32-bit
//! definitions themselves when the extensions are missing (Noble's `primitive_types.glsl`
//! defines `u16vec2` as `uvec2`), so lowering keeps their meaning. Values gain range and
//! precision: 16-bit integer arithmetic no longer wraps at 16 bits and `float16_t` math
//! runs at 32-bit precision. Buffer and shared-memory layouts declared with these types
//! would grow, so packs whose storage blocks use them would need their buffer sizes
//! revisited.

use crate::token::{Tok, TokKind, id, lex};
use std::collections::BTreeSet;

/// The 32-bit type for a 16- or 8-bit scalar, vector or matrix type name.
fn wide_type(name: &str) -> Option<String> {
    let scalar = match name {
        "int16_t" | "int8_t" => Some("int"),
        "uint16_t" | "uint8_t" => Some("uint"),
        "float16_t" => Some("float"),
        _ => None,
    };
    if let Some(s) = scalar {
        return Some(s.to_string());
    }
    for (narrow, wide) in [
        ("i16vec", "ivec"),
        ("i8vec", "ivec"),
        ("u16vec", "uvec"),
        ("u8vec", "uvec"),
        ("f16vec", "vec"),
        ("f16mat", "mat"),
    ] {
        if let Some(size) = name.strip_prefix(narrow)
            && matches!(
                size,
                "2" | "3"
                    | "4"
                    | "2x2"
                    | "2x3"
                    | "2x4"
                    | "3x2"
                    | "3x3"
                    | "3x4"
                    | "4x2"
                    | "4x3"
                    | "4x4"
            )
            && (wide == "mat" || size.len() == 1)
        {
            return Some(format!("{wide}{size}"));
        }
    }
    None
}

/// 16-bit built-in functions that have a 32-bit equivalent of the same shape.
fn wide_builtin(name: &str) -> Option<&'static str> {
    Some(match name {
        "packFloat2x16" => "packHalf2x16",
        "unpackFloat2x16" => "unpackHalf2x16",
        _ => return None,
    })
}

/// 16-bit built-in functions replaced by generated `iris_` helpers.
const HELPER_BUILTINS: &[&str] = &[
    "packUint2x16",
    "unpackUint2x16",
    "packInt2x16",
    "unpackInt2x16",
    "float16BitsToUint16",
    "float16BitsToInt16",
    "uint16BitsToFloat16",
    "int16BitsToFloat16",
];

/// GLSL for the helper standing in for `builtin` on lowered values. The bit-cast helpers
/// have overloads for one to four components, as the built-ins do.
fn helper_source(builtin: &str) -> String {
    let (arg, ret, body) = match builtin {
        "packUint2x16" => {
            return "uint iris_packUint2x16(uvec2 v) { return bitfieldInsert(v.x, v.y, 16, 16); }\n"
                .into();
        }
        "unpackUint2x16" => {
            return "uvec2 iris_unpackUint2x16(uint v) { return uvec2(bitfieldExtract(v, 0, 16), bitfieldExtract(v, 16, 16)); }\n"
                .into();
        }
        "packInt2x16" => {
            return "int iris_packInt2x16(ivec2 v) { return bitfieldInsert(v.x, v.y, 16, 16); }\n"
                .into();
        }
        "unpackInt2x16" => {
            return "ivec2 iris_unpackInt2x16(int v) { return ivec2(bitfieldExtract(v, 0, 16), bitfieldExtract(v, 16, 16)); }\n"
                .into();
        }
        "float16BitsToUint16" => (
            ("float", "vec"),
            ("uint", "uvec"),
            "bitfieldExtract(packHalf2x16(vec2(v, 0.0)), 0, 16)",
        ),
        "float16BitsToInt16" => (
            ("float", "vec"),
            ("int", "ivec"),
            "bitfieldExtract(int(packHalf2x16(vec2(v, 0.0))), 0, 16)",
        ),
        "uint16BitsToFloat16" => (
            ("uint", "uvec"),
            ("float", "vec"),
            "unpackHalf2x16(bitfieldExtract(v, 0, 16)).x",
        ),
        _ => (
            ("int", "ivec"),
            ("float", "vec"),
            "unpackHalf2x16(uint(v)).x",
        ),
    };
    let name = format!("iris_{builtin}");
    let mut src = format!("{} {name}({} v) {{ return {body}; }}\n", ret.0, arg.0);
    for n in 2..=4 {
        let parts: Vec<String> = ["v.x", "v.y", "v.z", "v.w"][..n]
            .iter()
            .map(|c| format!("{name}({c})"))
            .collect();
        src.push_str(&format!(
            "{}{n} {name}({}{n} v) {{ return {}{n}({}); }}\n",
            ret.1,
            arg.1,
            ret.1,
            parts.join(", ")
        ));
    }
    src
}

/// Whether `src` defines a function called `name` (`<type> name(` outside a call).
fn defines_function(src: &[Tok], name: &str) -> bool {
    src.windows(3).any(|w| {
        w[1].ident() == Some(name) && w[2].is("(") && w[0].ident().is_some_and(|t| t != "return")
    })
}

/// Rewrites 16- and 8-bit types and built-ins, or `None` if the stage uses none.
pub(super) fn lower(src: &[Tok]) -> Option<Vec<Tok>> {
    let narrow = |t: &Tok| {
        t.ident().is_some_and(|n| {
            wide_type(n).is_some() || wide_builtin(n).is_some() || HELPER_BUILTINS.contains(&n)
        })
    };
    if !src.iter().any(narrow) {
        return None;
    }
    let mut helpers: BTreeSet<&'static str> = BTreeSet::new();
    let mut out: Vec<Tok> = Vec::with_capacity(src.len());
    for (i, t) in src.iter().enumerate() {
        let TokKind::Ident(name) = &t.kind else {
            out.push(t.clone());
            continue;
        };
        let member = i > 0 && src[i - 1].is(".");
        let called = src.get(i + 1).is_some_and(|n| n.is("("));
        if !member && let Some(wide) = wide_type(name) {
            out.push(id(&wide).at(t.line));
        } else if !member
            && called
            && let Some(f) = wide_builtin(name)
            && !defines_function(src, name)
        {
            out.push(id(f).at(t.line));
        } else if !member
            && called
            && let Some(h) = HELPER_BUILTINS.iter().find(|h| **h == name.as_str())
            && !defines_function(src, name)
        {
            helpers.insert(h);
            out.push(id(&format!("iris_{h}")).at(t.line));
        } else {
            out.push(t.clone());
        }
    }
    if !helpers.is_empty() {
        let src: String = helpers.iter().map(|h| helper_source(h)).collect();
        out.splice(0..0, lex(&src));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Emitter;

    /// Lowers a narrow-type fixture and emits the resulting token stream.
    fn lowered(src: &str) -> String {
        let mut e = Emitter::default();
        e.tokens(&lower(&lex(src)).expect("lowers"));
        e.finish().0
    }

    #[test]
    fn narrow_types_become_their_32_bit_counterparts() {
        let out = lowered(
            "struct S { int8_t n; }; u16vec2 f(float16_t x, f16vec3 c, i16vec4 d, f16mat3 m) \
             { uint16_t u = uint16_t(1); return u16vec2(u, u); }",
        );
        for narrow in [
            "int8_t",
            "u16vec2",
            "float16_t",
            "f16vec3",
            "i16vec4",
            "f16mat3",
            "uint16_t",
        ] {
            assert!(!out.contains(narrow), "{narrow} left in {out}");
        }
        for wide in [
            "int n",
            "uvec2 f",
            "float x",
            "vec3 c",
            "ivec4 d",
            "mat3 m",
            "uint u = uint(1)",
        ] {
            assert!(out.contains(wide), "{wide} missing from {out}");
        }
    }

    #[test]
    fn sixteen_bit_builtins_map_to_32_bit_equivalents_and_helpers() {
        let out = lowered(
            "void main() { uint a = packFloat2x16(vec2(1.0)); ivec2 b = unpackInt2x16(a); \
             vec3 c = int16BitsToFloat16(ivec3(b, 0)); }",
        );
        assert!(out.contains("packHalf2x16"), "{out}");
        assert!(out.contains("ivec2 iris_unpackInt2x16(int v)"), "{out}");
        assert!(
            out.contains("vec3 iris_int16BitsToFloat16(ivec3 v)"),
            "{out}"
        );
        assert!(!out.contains("iris_packFloat2x16"), "{out}");
    }

    #[test]
    fn a_pack_defined_function_keeps_its_name() {
        let out = lowered(
            "uint packUint2x16(uvec2 v) { return v.x; } uint u16 = packUint2x16(uvec2(1u));",
        );
        assert!(!out.contains("iris_"), "{out}");
    }
}
