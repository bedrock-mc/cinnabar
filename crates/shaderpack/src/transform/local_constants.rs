//! Inlines block-scoped constants where GLSL requires a constant expression.

use super::items::{constant_contexts, declarations, matching};
use crate::token::{Tok, p};
use std::collections::HashMap;
use std::ops::Range;

/// One local name in scope and, for an inlinable constant, its already-inlined value.
type Binding<'a> = (&'a str, Option<Vec<Tok>>);

/// Replaces function-local constants used in array sizes, array constructor sizes, constant
/// initializers and case labels with their parenthesized values. GLSL accepts
/// `const int n = 4; float v[n];`, but naga's GLSL frontend evaluates those expressions in
/// global scope, where block-scoped names are unknown. A later local declaration of the same
/// name hides the constant until its block ends. Returns `None` when nothing changes.
pub(super) fn inline_local_constants(src: &[Tok]) -> Option<Vec<Tok>> {
    if !src.iter().any(|token| token.ident() == Some("const")) {
        return None;
    }
    // Declarator index -> (index where the name becomes visible, inlinable initializer range).
    let mut declarators: HashMap<usize, (usize, Option<Range<usize>>)> = HashMap::new();
    for (_, end, declaration) in declarations(src) {
        let constant = declaration.has("const") && declaration.ty_arr.is_empty();
        let mut cursor = end;
        for (_, array, initializer) in declaration.decls.iter().rev() {
            let visible_from = cursor;
            cursor -= initializer.len() + usize::from(!initializer.is_empty()) + array.len() + 1;
            let value = (constant && array.is_empty() && !initializer.is_empty())
                .then_some(cursor + 2..visible_from);
            declarators.insert(cursor, (visible_from, value));
            cursor = cursor.saturating_sub(1);
        }
    }
    let mut contexts = constant_contexts(src);
    for (open, token) in src.iter().enumerate().skip(1) {
        if token.is("[") && src[open - 1].ident().is_some() {
            let close = matching(src, open, "[", "]");
            if src.get(close + 1).is_some_and(|token| token.is("(")) {
                contexts.extend(open..close);
            }
        }
    }

    let mut scopes: Vec<Vec<Binding>> = Vec::new();
    let mut pending: Vec<(usize, Binding)> = Vec::new();
    let mut out = Vec::with_capacity(src.len());
    let mut changed = false;
    for (i, token) in src.iter().enumerate() {
        while let Some(index) = pending.iter().position(|(from, _)| *from <= i) {
            let (_, binding) = pending.swap_remove(index);
            if let Some(scope) = scopes.last_mut() {
                scope.push(binding);
            }
        }
        if token.is("{") {
            scopes.push(Vec::new());
        } else if token.is("}") {
            scopes.pop();
        }
        let after_member = i > 0 && src[i - 1].is(".");
        match token.ident() {
            Some(name) if !scopes.is_empty() && declarators.contains_key(&i) => {
                let (visible_from, value) = &declarators[&i];
                let value = value.clone().map(|range| {
                    let mut inlined = Vec::with_capacity(range.len());
                    for at in range {
                        let prior = at > 0 && src[at - 1].is(".");
                        if !inline(&mut inlined, &src[at], prior, &scopes) {
                            inlined.push(src[at].clone());
                        }
                    }
                    inlined
                });
                pending.push((*visible_from, (name, value)));
            }
            Some(_) if contexts.contains(&i) => {
                if inline(&mut out, token, after_member, &scopes) {
                    changed = true;
                    continue;
                }
            }
            _ => {}
        }
        out.push(token.clone());
    }
    changed.then_some(out)
}

/// Appends `( value )` for an identifier naming a visible inlinable constant; reports whether
/// it did. Member names after `.` are never constants.
fn inline(out: &mut Vec<Tok>, token: &Tok, after_member: bool, scopes: &[Vec<Binding>]) -> bool {
    let Some(name) = token.ident().filter(|_| !after_member) else {
        return false;
    };
    let binding = scopes
        .iter()
        .rev()
        .flat_map(|scope| scope.iter().rev())
        .find(|(bound, _)| *bound == name);
    let Some((_, Some(value))) = binding else {
        return false;
    };
    out.push(p("(").at(token.line));
    out.extend(value.iter().map(|part| part.clone().at(token.line)));
    out.push(p(")").at(token.line));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    /// Spells the inlined token stream, or the input when nothing changed.
    fn inlined(src: &str) -> String {
        let tokens = lex(src);
        inline_local_constants(&tokens)
            .unwrap_or(tokens)
            .iter()
            .map(Tok::text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn local_constants_inline_only_in_constant_contexts() {
        assert_eq!(
            inlined("void f() { const int n = 2; float a[n]; a[n - 1] = float(n); }"),
            "void f ( ) { const int n = 2 ; float a [ ( 2 ) ] ; a [ n - 1 ] = float ( n ) ; }"
        );
        assert_eq!(
            inlined("void f() { const int n = 2; const int m = n + 1; vec2 v[m] = vec2[m](); }"),
            "void f ( ) { const int n = 2 ; const int m = ( 2 ) + 1 ; \
             vec2 v [ ( ( 2 ) + 1 ) ] = vec2 [ ( ( 2 ) + 1 ) ] ( ) ; }"
        );
    }

    #[test]
    fn local_constants_respect_shadows_members_and_globals() {
        assert_eq!(
            inlined("const int g = 3; void f() { float a[g]; }"),
            "const int g = 3 ; void f ( ) { float a [ g ] ; }"
        );
        assert_eq!(
            inlined("void f() { const int n = 2; { int n = 4; float a[n]; } float b[s.n]; }"),
            "void f ( ) { const int n = 2 ; { int n = 4 ; float a [ n ] ; } float b [ s . n ] ; }"
        );
        assert_eq!(
            inlined("void f() { { const int n = 2; } float a[n]; }"),
            "void f ( ) { { const int n = 2 ; } float a [ n ] ; }"
        );
    }
}
