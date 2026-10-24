//! Folds proven scalar constants only where the compiler needs an array size.

use super::constant_expr::{Scalar, Value, evaluate};
use super::items::{binding_uses, declarations, matching};
use crate::token::{Tok, int};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

/// Replaces array bounds that use local constants with typed integer literals. Global
/// dependencies remain visible to the linker. Declarations, initializers,
/// case labels and runtime expressions retain their original tokens. Values are memoized
/// per declarator, including failed evaluations, instead of expanding dependency expressions.
pub(super) fn fold_array_sizes(src: &[Tok]) -> Option<Vec<Tok>> {
    if !src.iter().any(|token| token.ident() == Some("const")) {
        return None;
    }
    let mut constants = Vec::new();
    let mut bounds = BTreeMap::new();
    for (_, end, declaration) in declarations(src) {
        let scalar = Scalar::parse(&declaration.ty).filter(|_| {
            declaration.has("const")
                && declaration.ty_arr.is_empty()
                && !declaration
                    .layout
                    .iter()
                    .any(|token| token.ident() == Some("constant_id"))
        });
        let mut cursor = end;
        for (_, array, initializer) in declaration.decls.iter().rev() {
            let visible_from = cursor;
            cursor -= initializer.len() + usize::from(!initializer.is_empty()) + array.len() + 1;
            array_bounds(src, cursor + 1..cursor + 1 + array.len(), &mut bounds);
            if let Some(ty) = scalar
                && array.is_empty()
                && !initializer.is_empty()
            {
                constants.push((cursor, ty, cursor + 2..visible_from));
            }
            cursor = cursor.saturating_sub(1);
        }
        if !declaration.ty_arr.is_empty() {
            array_bounds(
                src,
                cursor + 1 - declaration.ty_arr.len()..cursor + 1,
                &mut bounds,
            );
        }
    }
    // Array constructors can occur inside any expression, beyond declaration suffixes.
    for open in 1..src.len() {
        if src[open].is("[") && src[open - 1].ident().is_some() {
            let mut after = matching(src, open, "[", "]") + 1;
            while src.get(after).is_some_and(|token| token.is("[")) {
                after = matching(src, after, "[", "]") + 1;
            }
            if src.get(after).is_some_and(|token| token.is("(")) {
                array_bounds(src, open..after, &mut bounds);
            }
        }
    }
    if bounds.is_empty() || constants.is_empty() {
        return None;
    }
    let mut depth = 0;
    let in_block: Vec<_> = src
        .iter()
        .map(|token| {
            if token.is("{") {
                depth += 1;
            } else if token.is("}") {
                depth -= 1;
            }
            depth > 0
        })
        .collect();
    let locals: HashSet<_> = constants
        .iter()
        .filter_map(|(index, _, _)| in_block[*index].then_some(*index))
        .collect();
    let uses = binding_uses(
        src,
        constants
            .iter()
            .filter_map(|(index, _, _)| src[*index].ident()),
    );
    constants.sort_unstable_by_key(|(_, _, initializer)| initializer.end);
    let mut constants = constants.into_iter().peekable();
    let mut values: HashMap<usize, Option<Value>> = HashMap::new();
    let mut replacements = BTreeMap::new();
    for (start, end) in bounds {
        while constants
            .peek()
            .is_some_and(|(_, _, initializer)| initializer.end <= start)
        {
            let (index, ty, initializer) = constants.next()?;
            let value = evaluate(&src[initializer.clone()], |at| {
                values
                    .get(uses.get(&(initializer.start + at))?)
                    .copied()
                    .flatten()
            })
            .and_then(|value| value.assign(ty));
            values.insert(index, value);
        }
        if !(start..end).any(|at| uses.get(&at).is_some_and(|index| locals.contains(index))) {
            continue;
        }
        if let Some(literal) = evaluate(&src[start..end], |at| {
            values.get(uses.get(&(start + at))?).copied().flatten()
        })
        .and_then(Value::array_literal)
        {
            replacements.insert(start, (end, int(literal).at(src[start].line)));
        }
    }
    if replacements.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(src.len());
    let mut cursor = 0;
    for (start, (end, literal)) in replacements {
        out.extend_from_slice(&src[cursor..start]);
        out.push(literal);
        cursor = end;
    }
    out.extend_from_slice(&src[cursor..]);
    Some(out)
}

/// Records the expressions inside an array declaration or constructor's bracket suffixes.
fn array_bounds(src: &[Tok], suffix: Range<usize>, bounds: &mut BTreeMap<usize, usize>) {
    let mut open = suffix.start;
    while open < suffix.end && src[open].is("[") {
        let close = matching(src, open, "[", "]");
        if close > open + 1 {
            bounds.insert(open + 1, close);
        }
        open = close + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    #[test]
    fn dependency_folding_keeps_output_linear() {
        let mut source = String::from("void f() { const int v0 = 1;");
        for index in 1..16 {
            let previous = index - 1;
            source.push_str(&format!("const int v{index} = v{previous} + v{previous};"));
        }
        source.push_str("float values[v15]; }");
        let tokens = lex(&source);
        let folded = fold_array_sizes(&tokens).unwrap();
        assert_eq!(folded.len(), tokens.len());
        let array = folded.iter().rposition(|token| token.is("[")).unwrap();
        assert_eq!(folded[array + 1].text().parse::<i32>().unwrap(), 1 << 15);
    }
}
