//! Monomorphizes helper functions that take samplers or images.
//!
//! OpenGL code passes samplers and images to helpers. With separate textures and samplers,
//! naga needs to see each global resource where it is sampled (depth comparison and storage
//! details cannot flow through parameters), so every call whose opaque arguments are global
//! uniforms is specialized: `f(colortex0, uv)` becomes `f__colortex0(uv)`. Iterating lets
//! nested helpers specialize too.

use super::items::{Item, Param, func_params, is_opaque, matching, shadowed_bindings, split_args};
use crate::token::{Tok, TokKind, id, p};
use std::collections::{HashMap, HashSet};

/// Rounds of specialization; each round handles one more level of helper nesting.
const MAX_ROUNDS: usize = 32;

/// Generates every resource-compatible overload and leaves value typing to naga.
pub(super) fn specialize_opaque_params(mut items: Vec<Item>) -> Vec<Item> {
    let globals: HashMap<String, String> = items
        .iter()
        .filter_map(|it| match it {
            Item::Decl(d, _) if d.has("uniform") && is_opaque(&d.ty) => Some(
                d.decls
                    .iter()
                    .map(|x| (x.0.clone(), d.ty.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    // A template signature stays stable when generated definitions shift item indices.
    let mut created: HashSet<(String, Vec<Vec<String>>, Vec<String>)> = HashSet::new();
    let mut template_names: HashSet<String> = HashSet::new();
    let mut generated = HashSet::new();
    let mut occupied: HashSet<String> = items
        .iter()
        .flat_map(|item| match item {
            Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => {
                toks.iter().filter_map(Tok::ident).map(str::to_string)
            }
        })
        .collect();
    let mut instances = HashMap::new();
    let mut next_name = 0usize;
    for _ in 0..MAX_ROUNDS {
        let mut templates: HashMap<String, Vec<(usize, Vec<Param>)>> = HashMap::new();
        for (idx, it) in items.iter().enumerate() {
            if let Item::Func { name, toks } = it
                && let Some((_, params)) = func_params(toks)
                && params.iter().any(|p| p.opaque)
            {
                templates
                    .entry(name.clone())
                    .or_default()
                    .push((idx, params));
                template_names.insert(name.clone());
            }
        }
        if templates.is_empty() {
            break;
        }
        // (template index, specialized name, params, opaque argument names)
        let mut new_funcs: Vec<(usize, String, Vec<Param>, Vec<String>)> = vec![];
        let mut changed = false;
        for it in items.iter_mut() {
            let (toks, start) = match it {
                Item::Func { toks, .. } => {
                    let b = toks.iter().position(|t| t.is("{")).unwrap_or(toks.len());
                    (toks, b)
                }
                Item::Decl(_, toks) | Item::Other(toks) => (toks, 0),
            };
            let shadowed = shadowed_bindings(toks, globals.keys().map(String::as_str));
            let parameters: HashSet<_> = if start > 0 {
                func_params(toks)
                    .map(|(_, params)| params.into_iter().map(|p| p.name).collect())
                    .unwrap_or_default()
            } else {
                HashSet::new()
            };
            let mut i = start;
            while i < toks.len() {
                let callee = match &toks[i].kind {
                    TokKind::Ident(n)
                        if templates.contains_key(n)
                            && toks.get(i + 1).is_some_and(|t| t.is("("))
                            && !(i > 0 && toks[i - 1].is(".")) =>
                    {
                        n.clone()
                    }
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let close = matching(toks, i + 1, "(", ")");
                let args = split_args(&toks[i + 2..close]);
                let mut argument_start = i + 2;
                let resources: Vec<_> = args
                    .iter()
                    .map(|arg| {
                        let start = argument_start;
                        argument_start += arg.len() + 1;
                        (arg.len() == 1 && !shadowed.contains(&start))
                            .then(|| arg[0].ident())
                            .flatten()
                            .filter(|name| !parameters.contains(*name))
                            .and_then(|name| globals.get(name))
                    })
                    .collect();
                let compatible: Vec<_> = templates[&callee]
                    .iter()
                    .filter(|(_, params)| {
                        params.len() == args.len()
                            && params.iter().zip(&resources).all(|(param, global)| {
                                if param.opaque {
                                    param
                                        .toks
                                        .iter()
                                        .find_map(|t| t.ident().filter(|ty| is_opaque(ty)))
                                        == global.map(String::as_str)
                                } else {
                                    global.is_none()
                                }
                            })
                    })
                    .cloned()
                    .collect();
                if let Some((_, params)) = compatible.first() {
                    let opaque_args: Vec<String> = params
                        .iter()
                        .zip(&args)
                        .filter(|(param, _)| param.opaque)
                        .map(|(_, arg)| arg[0].text().to_string())
                        .collect();
                    let sname = instances
                        .entry((
                            callee.clone(),
                            params
                                .iter()
                                .enumerate()
                                .filter_map(|(index, param)| param.opaque.then_some(index))
                                .collect::<Vec<_>>(),
                            opaque_args.clone(),
                        ))
                        .or_insert_with(|| {
                            loop {
                                let name = format!("iris_specialized_{next_name}");
                                next_name += 1;
                                if occupied.insert(name.clone()) {
                                    break name;
                                }
                            }
                        })
                        .clone();
                    let line = toks[i].line;
                    let mut repl = vec![id(&sname).at(line), p("(").at(line)];
                    let mut first = true;
                    for (param, arg) in params.iter().zip(&args) {
                        if param.opaque {
                            continue;
                        }
                        if !first {
                            repl.push(p(",").at(line));
                        }
                        first = false;
                        repl.extend(arg.iter().cloned());
                    }
                    repl.push(p(")").at(line));
                    toks.splice(i..=close, repl);
                    for (tidx, params) in compatible {
                        let signature = params
                            .iter()
                            .map(|param| {
                                param
                                    .toks
                                    .iter()
                                    .map(|token| token.text().to_string())
                                    .collect()
                            })
                            .collect();
                        if created.insert((callee.clone(), signature, opaque_args.clone())) {
                            new_funcs.push((tidx, sname.clone(), params, opaque_args.clone()));
                        }
                    }
                    changed = true;
                }
                i += 1;
            }
        }
        new_funcs.sort_by_key(|f| std::cmp::Reverse(f.0));
        for (tidx, sname, params, opaque_args) in new_funcs {
            let Item::Func { toks, .. } = &items[tidx] else {
                continue;
            };
            let Some((lp, _)) = func_params(toks) else {
                continue;
            };
            let Some(brace) = toks.iter().position(|t| t.is("{")) else {
                continue;
            };
            let line = toks[lp].line;
            let mut out: Vec<Tok> = toks[..lp.saturating_sub(1)].to_vec();
            out.extend([id(&sname).at(line), p("(").at(line)]);
            let mut first = true;
            let mut subst: HashMap<String, String> = HashMap::new();
            let mut oa = opaque_args.iter();
            for pa in &params {
                if pa.opaque {
                    if let Some(arg) = oa.next() {
                        subst.insert(pa.name.clone(), arg.clone());
                    }
                    continue;
                }
                if !first {
                    out.push(p(",").at(line));
                }
                first = false;
                out.extend(pa.toks.iter().cloned());
            }
            out.push(p(")").at(line));
            let shadowed = shadowed_bindings(&toks[brace..], subst.keys().map(String::as_str));
            for (k, t) in toks[brace..].iter().enumerate() {
                let after_dot = k > 0 && toks[brace + k - 1].is(".");
                match &t.kind {
                    TokKind::Ident(n)
                        if !after_dot && !shadowed.contains(&k) && subst.contains_key(n) =>
                    {
                        out.push(id(&subst[n]).at(t.line))
                    }
                    _ => out.push(t.clone()),
                }
            }
            generated.insert(sname.clone());
            items.insert(
                tidx + 1,
                Item::Func {
                    name: sname,
                    toks: out,
                },
            );
        }
        if !changed {
            break;
        }
    }
    // Rebuild opaque prototypes from the definitions that remain after specialization.
    items.retain(|item| {
        !matches!(item, Item::Other(tokens)
        if func_params(tokens).is_some_and(|(_, params)| params.iter().any(|param| param.opaque)))
    });
    // Drop templates nothing calls any more.
    let referenced = |name: &str, items: &[Item]| {
        items.iter().any(|it| {
            let toks = match it {
                Item::Func { name: n, .. } if n == name => return false,
                Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
            };
            toks.windows(2)
                .any(|w| w[0].ident() == Some(name) && w[1].is("("))
        })
    };
    let keep: Vec<bool> = items
        .iter()
        .map(|it| match it {
            Item::Func { name, toks }
                if template_names.contains(name)
                    && func_params(toks).is_some_and(|(_, ps)| ps.iter().any(|p| p.opaque)) =>
            {
                referenced(name, &items)
            }
            _ => true,
        })
        .collect();
    let mut keep = keep.into_iter();
    items.retain(|_| keep.next().unwrap_or(true));
    let prototypes: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let Item::Func { name, toks } = item else {
                return None;
            };
            if !generated.contains(name)
                && !func_params(toks)
                    .is_some_and(|(_, params)| params.iter().any(|param| param.opaque))
            {
                return None;
            }
            let brace = toks.iter().position(|token| token.is("{"))?;
            let mut header = toks[..brace].to_vec();
            header.push(p(";"));
            Some(Item::Other(header))
        })
        .collect();
    items.splice(0..0, prototypes);
    items
}
