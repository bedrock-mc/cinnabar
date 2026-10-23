//! Separates shared-memory declarators before naga assigns their array types.

use super::items::Item;
use crate::token::{id, p};

/// Gives each shared variable its own declaration so array dimensions do not accumulate
/// across comma-separated declarators in the GLSL frontend.
pub(super) fn split_declarations(items: Vec<Item>) -> Vec<Item> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Item::Decl(mut declaration, original) = item else {
            out.push(item);
            continue;
        };
        if !declaration.has("shared") || declaration.decls.len() < 2 {
            out.push(Item::Decl(declaration, original));
            continue;
        }
        let line = original.first().map_or(0, |token| token.line);
        for declarator in std::mem::take(&mut declaration.decls) {
            let mut declaration = declaration.clone();
            declaration.decls = vec![declarator.clone()];
            let mut tokens = Vec::new();
            if !declaration.layout.is_empty() {
                tokens.extend([id("layout"), p("(")]);
                tokens.extend(declaration.layout.iter().cloned());
                tokens.push(p(")"));
            }
            tokens.extend(declaration.quals.iter().map(|name| id(name)));
            tokens.push(id(&declaration.ty));
            tokens.extend(declaration.ty_arr.iter().cloned());
            tokens.push(id(&declarator.0));
            tokens.extend(declarator.1.iter().cloned());
            if !declarator.2.is_empty() {
                tokens.push(p("="));
                tokens.extend(declarator.2.iter().cloned());
            }
            tokens.push(p(";"));
            for token in &mut tokens {
                token.line = line;
            }
            out.push(Item::Decl(declaration, tokens));
        }
    }
    out
}
