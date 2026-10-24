//! Renames local variables that hide a struct type of the same name.
//!
//! GLSL lets `Face Face = load();` hide the type `Face` from the end of that declarator to the
//! end of its scope, so a later `Face.size` reads the variable. naga's GLSL frontend keeps
//! reading the name as the type and expects a constructor there, so each hiding variable and
//! its uses get one fresh name per struct. Initializers still see the type, as in GLSL, and so
//! does a struct of the same name declared in a nested block, until that block ends.

use super::items::{Item, binding_uses, matching, struct_body_end};
use crate::token::{Tok, id};
use std::collections::HashSet;
use std::ops::Range;

/// Renames every function-local variable that shares its name with a top-level struct, and
/// the uses that variable hides, leaving the type's own uses (including in the variable's
/// initializer) untouched. Members of structs declared inside a function keep their names, and
/// a struct declared after the variable takes the name back for the rest of its block.
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
        let local_types = local_structs(toks);
        let bindings = binding_uses(toks, structs.iter().map(String::as_str));
        for (index, declarator) in bindings {
            let token = &toks[index];
            let names_a_type = local_types.iter().any(|local| {
                local.body.contains(&index)
                    || (token.ident() == local.name.as_deref()
                        && local.visible.start > declarator
                        && local.visible.contains(&index))
            });
            if names_a_type {
                continue;
            }
            if let Some((_, renamed)) = fresh
                .iter()
                .find(|(name, _)| token.ident() == Some(name.as_str()))
            {
                toks[index] = id(renamed).at(token.line);
            }
        }
    }
}

/// A struct declared inside a function body.
struct LocalStruct {
    /// The struct's name, if it has one.
    name: Option<String>,
    /// The declaration, from `struct` to its closing brace; its members are not variables.
    body: Range<usize>,
    /// The tokens in which the name means this type: from `struct` to the end of its block.
    visible: Range<usize>,
}

/// Finds the struct declarations in the function tokens `toks`.
fn local_structs(toks: &[Tok]) -> Vec<LocalStruct> {
    let mut blocks = Vec::new();
    let mut found = Vec::new();
    for (index, token) in toks.iter().enumerate() {
        if let Some(end) = struct_body_end(toks, index) {
            let block_end = blocks
                .last()
                .map_or(toks.len(), |&open| matching(toks, open, "{", "}") + 1);
            found.push(LocalStruct {
                name: toks.get(index + 1).and_then(Tok::ident).map(str::to_string),
                body: index..end,
                visible: index..block_end,
            });
        }
        if token.is("{") {
            blocks.push(index);
        } else if token.is("}") {
            blocks.pop();
        }
    }
    found
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
    fn members_of_a_struct_declared_inside_the_function_keep_their_names() {
        assert_eq!(
            renamed(
                "struct S { float x; }; float f() { float S = 0.0; struct T { float S; }; T t = T(1.0); return t.S + S; }"
            ),
            "float f ( ) { float __shaderpack_S_local_0 = 0.0 ; struct T { float S ; } ; T t = T ( 1.0 ) ; return t . S + __shaderpack_S_local_0 ; }"
        );
    }

    #[test]
    fn a_struct_declared_in_an_inner_block_takes_its_name_back_there() {
        assert_eq!(
            renamed(
                "struct S { float x; }; float f() { float S = 0.0; { struct S { float x; }; S value = S(1.0); { S S = S(2.0); S.x += value.x; } } return S; }"
            ),
            "float f ( ) { float __shaderpack_S_local_0 = 0.0 ; { struct S { float x ; } ; S value = S ( 1.0 ) ; { S __shaderpack_S_local_0 = S ( 2.0 ) ; __shaderpack_S_local_0 . x += value . x ; } } return __shaderpack_S_local_0 ; }"
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
