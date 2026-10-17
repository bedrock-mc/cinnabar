//! Rewrites inside function bodies and initializers: separate texture/sampler pairs and
//! integer-backed bool uniforms.

use super::TransformError;
use super::items::{arr_len, mat_cols, matching, sampler_info};
use crate::reflection::{SAMPLER_NAME, SHADOW_SAMPLER_NAME};
use crate::token::{Tok, TokKind, id, int, p};
use std::collections::{HashMap, HashSet};

/// Sampler name → (combined sampler type to construct, compares depth).
pub(super) type SamplerKinds = HashMap<String, (String, bool)>;

/// What body rewriting needs to know about the stage's globals.
pub(super) struct BodyContext<'a> {
    pub samplers: &'a SamplerKinds,
    /// Bool uniform name → constructor (`bool` or `bvecN`) of its integer storage.
    pub bools: &'a HashMap<String, String>,
    pub user_funcs: &'a HashSet<String>,
}

/// Shared samplers the stage ended up referencing.
#[derive(Default)]
pub(super) struct SamplerUse {
    pub filtering: bool,
    pub comparison: bool,
}

/// Function parameters of combined sampler types become separate texture types.
pub(super) fn rewrite_params(header: &[Tok], samplers: &mut SamplerKinds) -> Vec<Tok> {
    let mut out: Vec<Tok> = vec![];
    for (i, t) in header.iter().enumerate() {
        if let Some(n) = t.ident()
            && let Some((tex, shadow)) = sampler_info(n)
            && let Some(Tok {
                kind: TokKind::Ident(pname),
                ..
            }) = header.get(i + 1)
        {
            samplers.insert(pname.clone(), (n.to_string(), shadow));
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

/// Pairs every texture use with the shared sampler (`sampler2D(colortex0, iris_sampler)`)
/// unless it is passed straight to a user function, and reads bool uniforms as bools.
pub(super) fn rewrite_body(toks: &[Tok], cx: &BodyContext<'_>, used: &mut SamplerUse) -> Vec<Tok> {
    let mut out = vec![];
    let mut calls: Vec<Option<String>> = vec![];
    for (i, t) in toks.iter().enumerate() {
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
        let line = t.line;
        if let Some((ctor, shadow)) = cx.samplers.get(n) {
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
                    used.filtering = true;
                    SAMPLER_NAME
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
        if let Some(ctor) = cx.bools.get(n) {
            if ctor == "bool" {
                out.extend([
                    p("(").at(line),
                    t.clone(),
                    p("!=").at(line),
                    int(0).at(line),
                    p(")").at(line),
                ]);
            } else {
                out.extend([
                    id(ctor).at(line),
                    p("(").at(line),
                    t.clone(),
                    p(")").at(line),
                ]);
            }
            continue;
        }
        out.push(t.clone());
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
