//! Supplies numeric declarations for unresolved vertex variables required by fragment inputs.

use super::TransformError;
use super::items::{func_params, layout_key, loc_size, parse_decl, split_items};
use crate::compile::CompiledStage;
use crate::preprocess::Preprocessed;
use crate::programs::StageKind;
use crate::token::{Tok, id, lex, p};
use std::collections::HashSet;

/// Repairs parser-reported vertex variables read through numeric fragment inputs.
/// Zero initialization runs at user main entry, preserving assignments made there.
pub(crate) fn vertex_outputs(
    vertex: &Preprocessed,
    fragment: &Preprocessed,
    compiled: &CompiledStage,
    unknown: &HashSet<String>,
) -> Result<Option<Vec<Tok>>, TransformError> {
    if compiled.module.entry_points.is_empty() {
        return Ok(None);
    }
    let live = crate::compile::numeric_reads(compiled).map_err(|failure| {
        TransformError::new(
            format!(
                "could not inspect compiled fragment: {}",
                failure
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            0,
        )
    })?;
    let mut main_body = None;
    let mut offset = 0;
    for item in split_items(&vertex.tokens) {
        if let Some((open, _)) = func_params(&item)
            && open > 0
            && item[open - 1].ident() == Some("main")
            && let Some(brace) = item.iter().position(|token| token.is("{"))
        {
            main_body = Some(offset + brace + 1);
            break;
        }
        offset += item.len();
    }
    let Some(main_body) = main_body else {
        return Ok(None);
    };
    let fragment = super::stage_tokens(&fragment.tokens, StageKind::Fragment, fragment.version)?;
    let mut outputs = Vec::new();
    let mut initializers = Vec::new();
    for item in split_items(&fragment.toks) {
        let Some(declaration) = parse_decl(&item).filter(|declaration| {
            (declaration.has("in") || declaration.has("varying"))
                && declaration.ty_arr.is_empty()
                && declaration
                    .decls
                    .iter()
                    .all(|(_, array, _)| array.is_empty())
        }) else {
            continue;
        };
        let mut location = layout_key(&declaration.layout, "location");
        for (name, _, _) in &declaration.decls {
            if !name.starts_with("gl_") && unknown.contains(name) && live.contains(name.as_str()) {
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
        return Ok(None);
    }
    let mut tokens = vertex.tokens.clone();
    tokens.splice(main_body..main_body, initializers);
    outputs.extend(tokens);
    Ok(Some(outputs))
}
