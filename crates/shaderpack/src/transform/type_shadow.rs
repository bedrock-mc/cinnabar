//! Renames local variables that hide a struct type of the same name.
//!
//! GLSL lets `Face Face = load();` hide the type `Face` from the end of that declarator to the
//! end of its scope, so a later `Face.size` reads the variable. naga's GLSL frontend keeps
//! reading the name as the type and expects a constructor there, so each hiding variable and
//! its uses get one fresh name per struct. Initializers still see the type, as in GLSL.

use super::items::{Item, shadowed_bindings};
use crate::token::{Tok, id};
use std::collections::HashSet;

/// Renames every function-local variable that shares its name with a top-level struct, and
/// the uses that variable hides, leaving the type's own uses (including in the variable's
/// initializer) untouched.
pub(super) fn rename_struct_shadowing_locals(items: &mut [Item], source: &[Tok]) {
    let structs: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::Decl(declaration, _) if declaration.ty == "struct" => {
                declaration.block.as_ref().map(|(name, _, _)| name.clone())
            }
            _ => None,
        })
        .collect();
    if structs.is_empty() {
        return;
    }
    let taken: HashSet<&str> = source.iter().filter_map(Tok::ident).collect();
    let fresh: Vec<(String, String)> = structs
        .iter()
        .map(|name| {
            let renamed = (0..)
                .map(|index| format!("__shaderpack_{name}_local_{index}"))
                .find(|candidate| !taken.contains(candidate.as_str()))
                .expect("a finite source cannot occupy every helper name");
            (name.clone(), renamed)
        })
        .collect();
    for item in items.iter_mut() {
        let Item::Func { toks, .. } = item else {
            continue;
        };
        let hidden = shadowed_bindings(toks, structs.iter().map(String::as_str));
        for index in hidden {
            let token = &toks[index];
            if let Some((_, renamed)) = fresh
                .iter()
                .find(|(name, _)| token.ident() == Some(name.as_str()))
            {
                toks[index] = id(renamed).at(token.line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    /// Renames the struct-shadowing locals of one function and returns its spelling.
    fn renamed(source: &str) -> String {
        let toks = lex(source);
        let split = super::super::items::split_items(&toks);
        let mut items: Vec<Item> = split
            .into_iter()
            .map(|item| {
                if item.last().is_some_and(|token| token.is("}")) {
                    Item::Func {
                        name: "f".to_string(),
                        toks: item,
                    }
                } else {
                    let declaration = super::super::items::parse_decl(&item).unwrap();
                    Item::Decl(declaration, item)
                }
            })
            .collect();
        rename_struct_shadowing_locals(&mut items, &toks);
        let Item::Func { toks, .. } = items.last().unwrap() else {
            panic!("the fixture ends with a function");
        };
        toks.iter().map(Tok::text).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_local_named_after_its_struct_is_renamed_after_its_initializer() {
        assert_eq!(
            renamed("struct S { float x; }; float f() { S S = S(1.0); return S.x; }"),
            "float f ( ) { S __shaderpack_S_local_0 = S ( 1.0 ) ; return __shaderpack_S_local_0 . x ; }"
        );
    }

    #[test]
    fn the_type_keeps_its_name_outside_the_hiding_scope() {
        assert_eq!(
            renamed(
                "struct S { float x; }; float f() { { S S = S(1.0); S.x += 1.0; } S s = S(2.0); return s.x; }"
            ),
            "float f ( ) { { S __shaderpack_S_local_0 = S ( 1.0 ) ; __shaderpack_S_local_0 . x += 1.0 ; } S s = S ( 2.0 ) ; return s . x ; }"
        );
    }

    #[test]
    fn members_and_unrelated_names_are_left_alone() {
        assert_eq!(
            renamed("struct S { float S; }; float f(S value) { return value.S; }"),
            "float f ( S value ) { return value . S ; }"
        );
    }
}
