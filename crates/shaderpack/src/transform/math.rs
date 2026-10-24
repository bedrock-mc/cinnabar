//! Keeps undefined constant square roots from failing front-end constant evaluation.

use super::items::Item;
use crate::token::{Tok, TokKind, id, lex};
use std::collections::HashSet;

/// Defers only built-in square roots of negative float literals inside functions.
/// The helper preserves the operation and its argument rather than clamping its result.
pub(super) fn defer_negative_sqrt(
    items: &mut Vec<Item>,
    user_functions: &HashSet<String>,
    source: &[Tok],
) {
    if user_functions.contains("sqrt") {
        return;
    }
    let names: HashSet<_> = source.iter().filter_map(Tok::ident).collect();
    let name = (0..)
        .map(|index| format!("__shaderpack_negative_sqrt_{index}"))
        .find(|name| !names.contains(name.as_str()))
        .expect("a finite source cannot occupy every helper name");
    let mut used = false;
    for item in items.iter_mut() {
        let Item::Func { toks, .. } = item else {
            continue;
        };
        for i in 0..toks.len().saturating_sub(4) {
            if toks[i].ident() != Some("sqrt")
                || i > 0 && toks[i - 1].is(".")
                || !toks[i + 1].is("(")
                || !toks[i + 2].is("-")
                || !toks[i + 4].is(")")
            {
                continue;
            }
            let TokKind::Float(literal) = &toks[i + 3].kind else {
                continue;
            };
            if literal
                .trim_end_matches(['f', 'F'])
                .parse::<f32>()
                .is_ok_and(|value| value > 0.0 && value.is_finite())
            {
                toks[i] = id(&name).at(toks[i].line);
                used = true;
            }
        }
    }
    if used {
        items.insert(
            0,
            Item::Func {
                toks: lex(&format!(
                    "float {name}(float value) {{ return sqrt(value); }}"
                )),
                name,
            },
        );
    }
}
