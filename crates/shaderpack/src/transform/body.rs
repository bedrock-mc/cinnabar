//! Rewrites inside function bodies and initializers: separate texture/sampler pairs and
//! integer-backed bool uniforms.

use super::TransformError;
use super::declare::integer_bool;
use super::items::{arr_len, assignment_end, mat_cols, matching, sampler_info, shadowed_bindings};
use crate::reflection::{SHADOW_SAMPLER_NAME, SamplerClass};
use crate::token::{Tok, TokKind, id, int, p};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Sampler name → (combined sampler type to construct, compares depth, filtering class).
pub(super) type SamplerKinds = HashMap<String, (String, bool, SamplerClass)>;

/// A boolean value stored as integers, including its array dimensions.
#[derive(Clone, Debug)]
pub(super) struct BoolStorage {
    pub ty: String,
    pub array_depth: usize,
}

impl BoolStorage {
    /// Records the value type and dimensions of an integer-backed boolean declaration.
    pub fn new<'a>(ty: &str, arrays: impl IntoIterator<Item = &'a Tok>) -> Self {
        Self {
            ty: ty.to_string(),
            array_depth: arrays.into_iter().filter(|token| token.is("[")).count(),
        }
    }
}

/// What body rewriting needs to know about the stage's globals.
pub(super) struct BodyContext<'a> {
    pub samplers: &'a SamplerKinds,
    /// Boolean global names and the value types and array dimensions of their storage.
    pub bools: &'a HashMap<String, BoolStorage>,
    pub user_funcs: &'a HashSet<String>,
}

/// Shared samplers the stage ended up referencing.
#[derive(Default)]
pub(super) struct SamplerUse {
    pub filtering: BTreeSet<SamplerClass>,
    pub comparison: bool,
}

/// Function parameters of combined sampler types become separate texture types, read with
/// the shared repeat/linear sampler: calls with global textures were specialized already.
/// Other parameters named like a global sampler shadow it. `const in` loses the `const`,
/// which naga's parser rejects in that order (SEUS Renewed); the copy is read-only anyway.
pub(super) fn rewrite_params(header: &[Tok], samplers: &mut SamplerKinds) -> Vec<Tok> {
    let mut out: Vec<Tok> = vec![];
    for (i, t) in header.iter().enumerate() {
        if t.ident() == Some("const") && header.get(i + 1).is_some_and(|x| x.ident() == Some("in"))
        {
            continue;
        }
        if let Some(n) = t.ident()
            && let Some((tex, shadow)) = sampler_info(n)
            && let Some(Tok {
                kind: TokKind::Ident(pname),
                ..
            }) = header.get(i + 1)
        {
            samplers.insert(
                pname.clone(),
                (n.to_string(), shadow, SamplerClass::RepeatLinear),
            );
            if out.last().is_some_and(|l| l.ident() == Some("in")) {
                out.pop();
            }
            out.push(id(tex).at(t.line));
            continue;
        }
        out.push(t.clone());
    }
    out
}

/// Pairs every texture use with its sampler (`sampler2D(colortex0, iris_samplerClampLinear)`)
/// unless it is passed straight to a user function, and reads bool uniforms as bools. Locals
/// that shadow a sampler or bool global (Complementary's `float specular`) are left alone in
/// their block from the end of their declarator on, as GLSL scopes them: SEUS's
/// `vec4 tex = texture2D(tex, …)` still samples the global in the initializer.
pub(super) fn rewrite_body(toks: &[Tok], cx: &BodyContext<'_>, used: &mut SamplerUse) -> Vec<Tok> {
    let shadowed = shadowed_bindings(
        toks,
        cx.samplers
            .keys()
            .chain(cx.bools.keys())
            .map(String::as_str),
    );
    let mut out = vec![];
    let mut calls: Vec<Option<String>> = vec![];
    let mut writes: Vec<usize> = vec![];
    let mut read_ends: Vec<(usize, bool)> = Vec::new();
    let mut write_starts: HashMap<usize, String> = HashMap::new();
    for (i, t) in toks.iter().enumerate() {
        while read_ends.last().is_some_and(|(end, _)| *end == i) {
            let (_, scalar) = read_ends.pop().unwrap();
            if scalar {
                out.extend([p("!=").at(t.line), int(0).at(t.line)]);
            }
            out.push(p(")").at(t.line));
        }
        while writes.last() == Some(&i) {
            writes.pop();
            out.extend([p(")").at(t.line), p(")").at(t.line)]);
        }
        if let Some(ctor) = write_starts.remove(&i) {
            out.extend([
                t.clone(),
                id(&integer_bool(&ctor)).at(t.line),
                p("(").at(t.line),
            ]);
            writes.push(assignment_end(toks, i + 1));
            continue;
        }
        if t.is("(") {
            calls.push(
                i.checked_sub(1)
                    .and_then(|j| toks[j].ident())
                    .map(str::to_string),
            );
        } else if t.is(")") {
            calls.pop();
        }
        let TokKind::Ident(n) = &t.kind else {
            out.push(t.clone());
            continue;
        };
        if i > 0 && toks[i - 1].is(".") {
            out.push(t.clone());
            continue;
        }
        if shadowed.contains(&i) {
            out.push(t.clone());
            continue;
        }
        let line = t.line;
        if let Some((ctor, shadow, class)) = cx.samplers.get(n) {
            let prev_ok = i > 0 && (toks[i - 1].is("(") || toks[i - 1].is(","));
            let next_ok = toks.get(i + 1).is_some_and(|x| x.is(")") || x.is(","));
            let in_user_call = calls
                .last()
                .and_then(|c| c.as_ref())
                .is_some_and(|c| cx.user_funcs.contains(c));
            if prev_ok && next_ok && in_user_call {
                out.push(t.clone());
            } else {
                let sampler = if *shadow {
                    used.comparison = true;
                    SHADOW_SAMPLER_NAME
                } else {
                    let class = if ctor.starts_with("isampler") || ctor.starts_with("usampler") {
                        if class.clamps() {
                            SamplerClass::ClampNearest
                        } else {
                            SamplerClass::RepeatNonFiltering
                        }
                    } else {
                        *class
                    };
                    used.filtering.insert(class);
                    class.glsl_name()
                };
                out.extend([
                    id(ctor).at(line),
                    p("(").at(line),
                    t.clone(),
                    p(",").at(line),
                    id(sampler).at(line),
                    p(")").at(line),
                ]);
            }
            continue;
        }
        if let Some(storage) = cx.bools.get(n) {
            let Some((end, ctor)) = boolean_access(toks, i, storage) else {
                out.push(t.clone());
                continue;
            };
            if toks.get(end).is_some_and(|token| token.is("=")) {
                out.extend([id(&ctor).at(line), p("(").at(line), t.clone()]);
                write_starts.insert(end, ctor);
            } else {
                let scalar = ctor == "bool";
                if !scalar {
                    out.push(id(&ctor).at(line));
                }
                out.extend([p("(").at(line), t.clone()]);
                read_ends.push((end, scalar));
            }
            continue;
        }
        out.push(t.clone());
    }
    out
}

/// Classifies a boolean value access, leaving array methods on their integer-backed receiver.
fn boolean_access(toks: &[Tok], start: usize, storage: &BoolStorage) -> Option<(usize, String)> {
    let mut end = start + 1;
    let mut arrays = storage.array_depth;
    let mut ty = storage.ty.clone();
    loop {
        if toks.get(end).is_some_and(|token| token.is("[")) {
            end = matching(toks, end, "[", "]") + 1;
            if arrays > 0 {
                arrays -= 1;
            } else {
                ty = "bool".to_string();
            }
        } else if toks.get(end).is_some_and(|token| token.is("."))
            && let Some(member) = toks.get(end + 1).and_then(Tok::ident)
        {
            if arrays > 0 && member == "length" {
                return None;
            }
            ty = if member.len() == 1 {
                "bool".to_string()
            } else {
                format!("bvec{}", member.len())
            };
            end += 2;
        } else {
            break;
        }
    }
    Some((end, ty))
}

/// The return type of a function header (`vec3 f(…)` → `vec3`), empty if there is none.
pub(super) fn return_type(header: &[Tok]) -> String {
    header
        .iter()
        .position(|t| t.is("("))
        .and_then(|lp| lp.checked_sub(2))
        .and_then(|i| header[i].ident())
        .unwrap_or_default()
        .to_owned()
}

/// GLSL converts a returned value to the function's type (Eclipse returns `0` from a
/// `uint` function); naga's parser does not, so every return of a numeric scalar or vector
/// function constructs the type, which is free when the value already has it.
pub(super) fn convert_returns(body: Vec<Tok>, ty: &str) -> Vec<Tok> {
    let numeric = matches!(ty, "float" | "int" | "uint")
        || ["vec", "ivec", "uvec"].iter().any(|v| {
            ty.strip_prefix(v)
                .is_some_and(|n| matches!(n, "2" | "3" | "4"))
        });
    if !numeric {
        return body;
    }
    let mut out = Vec::with_capacity(body.len());
    let mut open = false;
    for (i, t) in body.iter().enumerate() {
        if open && t.is(";") {
            out.extend([p(")").at(t.line), p(")").at(t.line)]);
            open = false;
        }
        out.push(t.clone());
        if t.ident() == Some("return") && body.get(i + 1).is_some_and(|n| !n.is(";")) {
            out.extend([id(ty).at(t.line), p("(").at(t.line), p("(").at(t.line)]);
            open = true;
        }
    }
    out
}

/// Struct name → members as (type, name, array tokens).
pub(super) type Structs = HashMap<String, Vec<(String, String, Vec<Tok>)>>;

/// Flattens a matrix, array or struct value into (access path, leaf type) pairs.
pub(super) fn flatten_leaves(
    path: String,
    ty: &str,
    arr: &[Tok],
    structs: &Structs,
    line: u32,
    out: &mut Vec<(String, String)>,
) -> Result<(), TransformError> {
    if !arr.is_empty() {
        let close = matching(arr, 0, "[", "]");
        let n = arr_len(&arr[..=close]).ok_or_else(|| {
            TransformError::new(
                format!("varying `{path}` has a non-literal array size"),
                line,
            )
        })?;
        for a in 0..n {
            flatten_leaves(
                format!("{path}[{a}]"),
                ty,
                &arr[close + 1..],
                structs,
                line,
                out,
            )?;
        }
        return Ok(());
    }
    if let Some((n, col)) = mat_cols(ty) {
        for c in 0..n {
            out.push((format!("{path}[{c}]"), col.to_string()));
        }
        return Ok(());
    }
    if let Some(members) = structs.get(ty) {
        for (mty, mname, marr) in members {
            flatten_leaves(format!("{path}.{mname}"), mty, marr, structs, line, out)?;
        }
        return Ok(());
    }
    out.push((path, ty.to_string()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    /// Joins fixture token spellings.
    fn text(toks: &[Tok]) -> String {
        toks.iter().map(Tok::text).collect::<Vec<_>>().join(" ")
    }

    /// Creates the sampled-resource bindings used by the body fixtures.
    fn samplers() -> SamplerKinds {
        SamplerKinds::from([
            (
                "colortex0".to_owned(),
                ("sampler2D".to_owned(), false, SamplerClass::ClampLinear),
            ),
            (
                "shadowtex0".to_owned(),
                (
                    "sampler2DShadow".to_owned(),
                    true,
                    SamplerClass::ClampLinear,
                ),
            ),
        ])
    }

    #[test]
    fn texture_uses_pair_with_the_shared_sampler_of_their_class() {
        let samplers = samplers();
        let cx = BodyContext {
            samplers: &samplers,
            bools: &HashMap::new(),
            user_funcs: &HashSet::new(),
        };
        let mut used = SamplerUse::default();
        let body = rewrite_body(&lex("texture(colortex0, uv).r;"), &cx, &mut used);
        assert_eq!(
            text(&body),
            format!(
                "texture ( sampler2D ( colortex0 , {} ) , uv ) . r ;",
                SamplerClass::ClampLinear.glsl_name()
            )
        );
        assert_eq!(used.filtering, BTreeSet::from([SamplerClass::ClampLinear]));
        assert!(!used.comparison);

        let body = rewrite_body(&lex("texture(shadowtex0, p);"), &cx, &mut used);
        assert_eq!(
            text(&body),
            format!("texture ( sampler2DShadow ( shadowtex0 , {SHADOW_SAMPLER_NAME} ) , p ) ;")
        );
        assert!(used.comparison);
    }

    #[test]
    fn textures_passed_to_user_functions_stay_separate_and_bool_uniforms_read_as_bools() {
        let samplers = samplers();
        let bools = HashMap::from([
            ("isDay".to_owned(), BoolStorage::new("bool", &[])),
            ("mask".to_owned(), BoolStorage::new("bvec2", &[])),
        ]);
        let user_funcs = HashSet::from(["blur".to_owned()]);
        let cx = BodyContext {
            samplers: &samplers,
            bools: &bools,
            user_funcs: &user_funcs,
        };
        let mut used = SamplerUse::default();
        let body = rewrite_body(
            &lex("blur(colortex0, uv); if (isDay && mask.x) {}"),
            &cx,
            &mut used,
        );
        assert_eq!(
            text(&body),
            "blur ( colortex0 , uv ) ; if ( ( isDay != 0 ) && ( mask . x != 0 ) ) { }"
        );
        assert!(used.filtering.is_empty());

        let mut params = SamplerKinds::new();
        let header = rewrite_params(&lex("vec4 blur(in sampler2D tex, vec2 uv)"), &mut params);
        assert_eq!(text(&header), "vec4 blur ( texture2D tex , vec2 uv )");
        assert_eq!(
            params["tex"],
            ("sampler2D".to_owned(), false, SamplerClass::RepeatLinear)
        );
    }

    #[test]
    fn values_flatten_to_their_leaves() {
        let structs = Structs::from([(
            "Light".to_owned(),
            vec![
                ("vec3".to_owned(), "dir".to_owned(), vec![]),
                ("mat2".to_owned(), "rot".to_owned(), vec![]),
            ],
        )]);
        let mut leaves = vec![];
        flatten_leaves(
            "lights".to_owned(),
            "Light",
            &lex("[2]"),
            &structs,
            1,
            &mut leaves,
        )
        .unwrap();
        let leaves: Vec<_> = leaves.iter().map(|(p, t)| format!("{p}: {t}")).collect();
        assert_eq!(
            leaves,
            [
                "lights[0].dir: vec3",
                "lights[0].rot[0]: vec2",
                "lights[0].rot[1]: vec2",
                "lights[1].dir: vec3",
                "lights[1].rot[0]: vec2",
                "lights[1].rot[1]: vec2",
            ]
        );
        let mut leaves = vec![];
        let error = flatten_leaves(
            "v".to_owned(),
            "float",
            &lex("[N]"),
            &structs,
            7,
            &mut leaves,
        )
        .unwrap_err();
        assert_eq!(error.line, 7);
    }
    #[test]
    fn integer_gathers_use_nearest_classes_without_changing_float_filtering() {
        let mut samplers = samplers();
        for (name, ctor, class) in [
            ("signedTex", "isampler2D", SamplerClass::ClampLinear),
            ("unsignedTex", "usampler2D", SamplerClass::RepeatLinear),
        ] {
            samplers.insert(name.into(), (ctor.into(), false, class));
        }
        let cx = BodyContext {
            samplers: &samplers,
            bools: &HashMap::new(),
            user_funcs: &HashSet::new(),
        };
        let mut used = SamplerUse::default();
        rewrite_body(
            &lex(
                "textureGather(signedTex, uv); textureGather(unsignedTex, uv); texture(colortex0, uv);",
            ),
            &cx,
            &mut used,
        );
        assert_eq!(
            used.filtering,
            BTreeSet::from([
                SamplerClass::ClampNearest,
                SamplerClass::RepeatNonFiltering,
                SamplerClass::ClampLinear,
            ])
        );
    }
}
