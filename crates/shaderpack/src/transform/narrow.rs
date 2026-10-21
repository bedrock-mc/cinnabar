//! Lowers the 16- and 8-bit types of `GL_NV_gpu_shader5` and
//! `GL_EXT_shader_explicit_arithmetic_types_*` to their 32-bit counterparts.
//!
//! naga has no 16- or 8-bit integers. This lowering also avoids requiring native
//! half-precision arithmetic. Packs
//! use these types for register pressure and ALU rate, and fall back to 32-bit
//! definitions themselves when the extensions are missing (Noble's `primitive_types.glsl`
//! defines `u16vec2` as `uvec2`), so lowering keeps their meaning. Values gain range and
//! precision: 16-bit integer arithmetic no longer wraps at 16 bits and `float16_t` math
//! runs at 32-bit precision. Buffer and shared-memory layouts declared with these types
//! would grow, so packs whose storage blocks use them would need their buffer sizes
//! revisited.

use crate::token::{Tok, TokKind, id, lex};
use std::collections::{BTreeMap, HashSet};

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

/// A lowered built-in's signature and expression, shared by recognition and emission.
struct HelperBuiltin {
    name: &'static str,
    arg: &'static str,
    ret: &'static str,
    body: &'static str,
    vector_overloads: bool,
}

/// Built-ins replaced by generated helpers, or inlined in integer constant expressions.
const HELPER_BUILTINS: &[HelperBuiltin] = &[
    HelperBuiltin {
        name: "packUint2x16",
        arg: "uvec2",
        ret: "uint",
        body: "((uint(v.x) & 65535u) | ((uint(v.y) & 65535u) << 16u))",
        vector_overloads: false,
    },
    HelperBuiltin {
        name: "unpackUint2x16",
        arg: "uint",
        ret: "uvec2",
        body: "uvec2(uint(v) & 65535u, uint(v) >> 16u)",
        vector_overloads: false,
    },
    HelperBuiltin {
        name: "packInt2x16",
        arg: "ivec2",
        ret: "int",
        body: "int((uint(v.x) & 65535u) | ((uint(v.y) & 65535u) << 16u))",
        vector_overloads: false,
    },
    HelperBuiltin {
        name: "unpackInt2x16",
        arg: "int",
        ret: "ivec2",
        body: "ivec2(int(uint(v) & 65535u) - int((uint(v) & 32768u) * 2u), int(uint(v) >> 16u) - int((uint(v) >> 31u) * 65536u))",
        vector_overloads: false,
    },
    HelperBuiltin {
        name: "float16BitsToUint16",
        arg: "float",
        ret: "uint",
        body: "bitfieldExtract(packHalf2x16(vec2(v, 0.0)), 0, 16)",
        vector_overloads: true,
    },
    HelperBuiltin {
        name: "float16BitsToInt16",
        arg: "float",
        ret: "int",
        body: "bitfieldExtract(int(packHalf2x16(vec2(v, 0.0))), 0, 16)",
        vector_overloads: true,
    },
    HelperBuiltin {
        name: "uint16BitsToFloat16",
        arg: "uint",
        ret: "float",
        body: "unpackHalf2x16(bitfieldExtract(v, 0, 16)).x",
        vector_overloads: true,
    },
    HelperBuiltin {
        name: "int16BitsToFloat16",
        arg: "int",
        ret: "float",
        body: "unpackHalf2x16(uint(v)).x",
        vector_overloads: true,
    },
];

/// Returns the vector type prefix for a helper's scalar type.
fn vector_type(scalar: &str) -> &'static str {
    match scalar {
        "float" => "vec",
        "uint" => "uvec",
        "int" => "ivec",
        _ => unreachable!("vector helpers take scalar types"),
    }
}

/// Emits a helper and, for bit casts, its two- to four-component overloads.
fn helper_source(helper: &HelperBuiltin) -> String {
    let HelperBuiltin {
        name: builtin,
        arg,
        ret,
        body,
        vector_overloads,
    } = helper;
    let name = format!("iris_{builtin}");
    let mut src = format!("{ret} {name}({arg} v) {{ return {body}; }}\n");
    if !vector_overloads {
        return src;
    }
    for n in 2..=4 {
        let parts: Vec<String> = ["v.x", "v.y", "v.z", "v.w"][..n]
            .iter()
            .map(|c| format!("{name}({c})"))
            .collect();
        src.push_str(&format!(
            "{}{n} {name}({}{n} v) {{ return {}{n}({}); }}\n",
            vector_type(ret),
            vector_type(arg),
            vector_type(ret),
            parts.join(", ")
        ));
    }
    src
}

/// Rewrites 16- and 8-bit types and built-ins, or `None` if the stage uses none.
pub(super) fn lower(src: &[Tok]) -> Option<Vec<Tok>> {
    let narrow = |t: &Tok| {
        t.ident().is_some_and(|n| {
            wide_type(n).is_some()
                || wide_builtin(n).is_some()
                || HELPER_BUILTINS.iter().any(|helper| helper.name == n)
        })
    };
    if !src.iter().any(narrow) {
        return None;
    }
    let user_functions: HashSet<_> = super::items::split_items(src)
        .into_iter()
        .filter(|item| super::items::parse_decl(item).is_none())
        .filter_map(|item| {
            super::items::func_params(&item).and_then(|(lp, _)| {
                lp.checked_sub(1)
                    .and_then(|name| item[name].ident())
                    .map(str::to_string)
            })
        })
        .collect();
    let mut helpers = BTreeMap::new();
    let identifiers = src.iter().filter_map(Tok::ident).collect();
    let mut out = lower_tokens(src, false, &identifiers, &user_functions, &mut helpers);
    if !helpers.is_empty() {
        let src: String = helpers
            .iter()
            .map(|(&index, name)| {
                let helper = &HELPER_BUILTINS[index];
                helper_source(helper).replace(&format!("iris_{}", helper.name), name)
            })
            .collect();
        out.splice(0..0, lex(&src));
    }
    Some(out)
}

/// Lowers tokens recursively, inlining integer packing where GLSL requires a constant.
fn lower_tokens(
    src: &[Tok],
    constant: bool,
    identifiers: &HashSet<&str>,
    user_functions: &HashSet<String>,
    helpers: &mut BTreeMap<usize, String>,
) -> Vec<Tok> {
    let constants = super::items::constant_contexts(src);
    let mut out = Vec::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        let t = &src[i];
        let TokKind::Ident(name) = &t.kind else {
            out.push(t.clone());
            i += 1;
            continue;
        };
        let member = i > 0 && src[i - 1].is(".");
        let called = src.get(i + 1).is_some_and(|n| n.is("("));
        if !member && let Some(wide) = wide_type(name) {
            out.push(id(&wide).at(t.line));
        } else if !member
            && called
            && !user_functions.contains(name)
            && let Some(f) = wide_builtin(name)
        {
            out.push(id(f).at(t.line));
        } else if !member
            && called
            && !user_functions.contains(name)
            && let Some((index, helper)) = HELPER_BUILTINS
                .iter()
                .enumerate()
                .find(|(_, helper)| helper.name == name.as_str())
        {
            let inline = (!helper.vector_overloads).then_some(helper.body);
            if let Some(inline) = inline.filter(|_| constant || constants.contains(&i)) {
                let end = super::items::matching(src, i + 1, "(", ")");
                let argument =
                    lower_tokens(&src[i + 2..end], true, identifiers, user_functions, helpers);
                for token in lex(inline) {
                    if token.ident() == Some("v") {
                        out.push(crate::token::p("(").at(t.line));
                        out.extend(argument.iter().cloned());
                        out.push(crate::token::p(")").at(t.line));
                    } else {
                        out.push(token.at(t.line));
                    }
                }
                i = end;
            } else {
                let name = helpers.entry(index).or_insert_with(|| {
                    let mut name = format!("iris_{}", helper.name);
                    while identifiers.contains(name.as_str()) {
                        name.push('_');
                    }
                    name
                });
                out.push(id(name).at(t.line));
            }
        } else {
            out.push(t.clone());
        }
        i += 1;
    }
    out
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
