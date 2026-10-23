//! Supplies numeric vertex declarations required by live fragment inputs.

use super::TransformError;
use super::items::{
    Decl, func_params, layout_key, loc_size, local_type_names, mat_cols, parse_decl,
    shadowed_bindings, split_items, struct_body_end,
};
use crate::preprocess::Preprocessed;
use crate::programs::StageKind;
use crate::token::{Tok, id, lex, p};
use std::collections::HashSet;

/// Declares missing numeric outputs before parsing, preserving writes in the user's main.
/// Outputs with no vertex references remain the linker's responsibility.
pub(crate) fn vertex_outputs(
    vertex: &mut Preprocessed,
    fragment: &Preprocessed,
) -> Result<(), TransformError> {
    // Each stage's transform reports malformed tokens with that stage's source map.
    if super::validate_delimiters(&vertex.tokens).is_err()
        || super::validate_delimiters(&fragment.tokens).is_err()
    {
        return Ok(());
    }
    let mut declared = HashSet::new();
    let mut main_body = None;
    let mut offset = 0;
    let vertex_items = split_items(&vertex.tokens);
    for item in &vertex_items {
        if let Some(declaration) = parse_decl(item) {
            register_names(declaration, &mut declared);
        } else if let Some((open, _)) = func_params(item)
            && open > 0
            && item[open - 1].ident() == Some("main")
            && let Some(brace) = item.iter().position(|token| token.is("{"))
        {
            main_body = Some(offset + brace + 1);
        }
        offset += item.len();
    }
    let Some(main_body) = main_body else {
        return Ok(());
    };
    let items = split_items(&fragment.tokens);
    let inputs: Vec<Decl> = items
        .iter()
        .filter_map(|tokens| parse_decl(tokens))
        .filter(|declaration| {
            (declaration.has("in") || declaration.has("varying"))
                && numeric(&declaration.ty)
                && declaration.ty_arr.is_empty()
                && declaration
                    .decls
                    .iter()
                    .all(|(_, array, _)| array.is_empty())
        })
        .collect();
    let names: HashSet<_> = inputs
        .iter()
        .flat_map(|d| d.decls.iter().map(|(name, _, _)| name.as_str()))
        .collect();
    let live = references(&items, &names);
    let core = super::legacy::undeclared_core_names(&vertex.tokens, StageKind::Vertex);
    let used_items = core.as_deref().map(split_items);
    let used = references(used_items.as_deref().unwrap_or(&vertex_items), &names);
    let mut outputs = Vec::new();
    let mut initializers = Vec::new();
    for declaration in &inputs {
        let mut location = layout_key(&declaration.layout, "location");
        for (name, _, _) in &declaration.decls {
            if !name.starts_with("gl_")
                && !declared.contains(name)
                && used.contains(name.as_str())
                && live.contains(name.as_str())
            {
                if let Some(location) = location {
                    outputs.extend(lex(&format!("layout(location = {location})")));
                }
                outputs.extend(
                    declaration
                        .quals
                        .iter()
                        .filter(|q| !matches!(q.as_str(), "in" | "varying"))
                        .map(|q| id(q)),
                );
                outputs.extend([id("out"), id(&declaration.ty), id(name), p(";")]);
                initializers.extend(lex(&format!("{name} = {}(0);", declaration.ty)));
                declared.insert(name.clone());
            }
            if let Some(current) = location {
                location = Some(
                    current
                        .checked_add(loc_size(&declaration.ty))
                        .ok_or_else(|| TransformError::new("varying location overflow", 0))?,
                );
            }
        }
    }
    if outputs.is_empty() {
        return Ok(());
    }
    vertex.tokens.splice(main_body..main_body, initializers);
    outputs.extend(std::mem::take(&mut vertex.tokens));
    vertex.tokens = outputs;
    Ok(())
}

/// Registers global variables, including struct instances and anonymous block members.
fn register_names(declaration: Decl, names: &mut HashSet<String>) {
    let mut declarations = vec![declaration];
    while let Some(declaration) = declarations.pop() {
        names.extend(declaration.decls.into_iter().map(|(name, _, _)| name));
        let Some((name, members, instances)) = declaration.block else {
            continue;
        };
        if declaration.ty == "block" && instances.is_empty() {
            declarations.extend(
                split_items(&members)
                    .iter()
                    .filter_map(|item| parse_decl(item)),
            );
        } else {
            let mut tokens = vec![id(&name)];
            tokens.extend(instances);
            tokens.push(p(";"));
            if let Some(instances) = parse_decl(&tokens) {
                names.extend(instances.decls.into_iter().map(|(name, _, _)| name));
            }
        }
    }
}

/// Recognizes the scalar, vector and matrix spellings supported by numeric interface repair.
fn numeric(ty: &str) -> bool {
    matches!(ty, "float" | "double" | "int" | "uint" | "bool")
        || ["vec", "ivec", "uvec", "dvec", "bvec"]
            .iter()
            .any(|prefix| {
                ty.strip_prefix(prefix)
                    .is_some_and(|n| matches!(n, "2" | "3" | "4"))
            })
        || mat_cols(ty.strip_prefix('d').unwrap_or(ty)).is_some()
}

/// Collects global reference expressions, excluding local bindings, fields and function calls.
fn references(items: &[Vec<Tok>], names: &HashSet<&str>) -> HashSet<String> {
    let mut live = HashSet::new();
    for item in items {
        if let Some(declaration) = parse_decl(item) {
            for (_, _, initializer) in declaration.decls {
                for (i, token) in initializer.iter().enumerate() {
                    if let Some(name) = token.ident()
                        && names.contains(name)
                        && !initializer.get(i + 1).is_some_and(|token| token.is("("))
                        && (i == 0 || !initializer[i - 1].is("."))
                    {
                        live.insert(name.to_owned());
                    }
                }
            }
            continue;
        }
        let Some((_, params)) = func_params(item) else {
            continue;
        };
        let Some(brace) = item.iter().position(|token| token.is("{")) else {
            continue;
        };
        let body = &item[brace..];
        let parameters: HashSet<_> = params.into_iter().map(|param| param.name).collect();
        let mut shadowed = shadowed_bindings(body, names.iter().copied());
        shadowed.extend(local_type_names(body));
        let mut type_end = 0;
        for (i, token) in body.iter().enumerate() {
            if let Some(end) = struct_body_end(body, i) {
                type_end = type_end.max(end);
            }
            if i < type_end {
                continue;
            }
            if let Some(name) = token.ident()
                && names.contains(name)
                && !parameters.contains(name)
                && !shadowed.contains(&i)
                && !body.get(i + 1).is_some_and(|token| token.is("("))
                && (i == 0 || !body[i - 1].is("."))
            {
                live.insert(name.to_owned());
            }
        }
    }
    live
}
