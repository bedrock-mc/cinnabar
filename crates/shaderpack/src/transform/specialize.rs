//! Monomorphizes helper functions that take samplers or images.
//!
//! Naga needs global resources at each sampling site, so calls with global opaque
//! arguments become resource-specific helpers. Each round plans edits against unchanged
//! tokens, then applies them from the end. Later rounds handle nested helpers.

use super::TransformError;
use super::items::{
    Item, Param, func_params, is_opaque, local_declarators, matching, shadowed_bindings, split_args,
};
use crate::token::{Tok, id, p};
use std::collections::{HashMap, HashSet};

/// Rounds of specialization; each round handles one more level of helper nesting.
const MAX_ROUNDS: usize = 32;

/// Maximum retained token storage generated while specializing one stage.
const MAX_SPECIALIZATION_BYTES: usize = 4 * crate::source::MAX_TEXT_FILE_BYTES;

/// Accounts for original tokens and every helper copy before allocating generated bodies.
struct StorageBudget(usize);

impl StorageBudget {
    /// Reserves the maximum spelling storage of these tokens after identifier substitution.
    fn reserve(&mut self, tokens: &[Tok], identifier_bytes: usize) -> Result<(), TransformError> {
        for token in tokens {
            let spelling = match &token.kind {
                crate::token::TokKind::Punct(_) => 0,
                _ => token.text().len().max(identifier_bytes),
            };
            self.0 = self
                .0
                .checked_sub(size_of::<Tok>() + spelling)
                .ok_or_else(|| {
                    TransformError::new(
                        "shader helper specialization exceeds storage limit",
                        token.line,
                    )
                })?;
        }
        Ok(())
    }
}

/// An original opaque overload, retained unchanged while generated helpers accumulate.
struct Template {
    position: usize,
    tokens: Vec<Tok>,
    params: Vec<Param>,
}

/// One token replacement or deletion in an unchanged item.
struct Edit {
    item: usize,
    token: usize,
    replacement: Option<Tok>,
}

/// Stable templates and resource-specific instances shared by all specialization rounds.
struct Specializer {
    globals: HashMap<String, String>,
    templates: HashMap<String, Vec<Template>>,
    instances: HashMap<(String, Vec<Option<String>>), String>,
    created: HashSet<(String, usize)>,
    occupied: HashSet<String>,
}

impl Specializer {
    /// Collects global resources, original overloads and occupied identifier spellings.
    fn new(items: &[Item]) -> Self {
        let mut pass = Self {
            globals: HashMap::new(),
            templates: HashMap::new(),
            instances: HashMap::new(),
            created: HashSet::new(),
            occupied: HashSet::new(),
        };
        for (position, item) in items.iter().enumerate() {
            let tokens = match item {
                Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
            };
            pass.occupied
                .extend(tokens.iter().filter_map(Tok::ident).map(str::to_string));
            match item {
                Item::Decl(decl, _) if decl.has("uniform") && is_opaque(&decl.ty) => {
                    pass.globals.extend(
                        decl.decls
                            .iter()
                            .map(|(name, _, _)| (name.clone(), decl.ty.clone())),
                    );
                }
                Item::Func { name, toks }
                    if let Some((_, params)) = func_params(toks)
                        && params.iter().any(|param| param.opaque) =>
                {
                    pass.templates
                        .entry(name.clone())
                        .or_default()
                        .push(Template {
                            position,
                            tokens: toks.clone(),
                            params,
                        });
                }
                _ => {}
            }
        }
        pass
    }

    /// Plans all calls before changing tokens, then applies disjoint edits in reverse order.
    fn rewrite_calls(
        &mut self,
        items: &mut Vec<(usize, Item)>,
        budget: &mut StorageBudget,
    ) -> Result<bool, TransformError> {
        let mut edits = Vec::new();
        let mut functions = Vec::new();
        for (item_index, (_, item)) in items.iter().enumerate() {
            let (tokens, start, parameters) = match item {
                Item::Func { toks, .. } => (
                    toks,
                    toks.iter()
                        .position(|token| token.is("{"))
                        .unwrap_or(toks.len()),
                    func_params(toks)
                        .map(|(_, params)| params.into_iter().map(|param| param.name).collect())
                        .unwrap_or_default(),
                ),
                Item::Decl(_, toks) | Item::Other(toks) => (toks, 0, HashSet::new()),
            };
            let shadowed = shadowed_bindings(tokens, self.globals.keys().map(String::as_str));
            for index in start..tokens.len() {
                let Some(callee) = tokens[index].ident() else {
                    continue;
                };
                let Some(templates) = self.templates.get(callee) else {
                    continue;
                };
                if !tokens.get(index + 1).is_some_and(|token| token.is("("))
                    || (index > 0 && tokens[index - 1].is("."))
                {
                    continue;
                }
                let close = matching(tokens, index + 1, "(", ")");
                let args = split_args(&tokens[index + 2..close]);
                let mut cursor = index + 2;
                let bindings: Vec<_> = args
                    .iter()
                    .map(|arg| {
                        let start = cursor;
                        cursor += arg.len() + 1;
                        (arg.len() == 1 && !shadowed.contains(&start))
                            .then(|| arg[0].ident())
                            .flatten()
                            .filter(|name| {
                                !parameters.contains(*name) && self.globals.contains_key(*name)
                            })
                            .map(str::to_string)
                    })
                    .collect();
                let compatible: Vec<_> = templates
                    .iter()
                    .enumerate()
                    .filter(|(_, template)| {
                        template.params.len() == bindings.len()
                            && template
                                .params
                                .iter()
                                .zip(&bindings)
                                .all(|(param, binding)| match binding {
                                    Some(name) => {
                                        param.opaque
                                            && param.toks.iter().find_map(|token| {
                                                token.ident().filter(|ty| is_opaque(ty))
                                            }) == self.globals.get(name).map(String::as_str)
                                    }
                                    None => !param.opaque,
                                })
                    })
                    .collect();
                if compatible.is_empty() {
                    continue;
                }
                let name = self
                    .instances
                    .entry((callee.to_string(), bindings.clone()))
                    .or_insert_with(|| fresh_name(&mut self.occupied))
                    .clone();
                edits.push(Edit {
                    item: item_index,
                    token: index,
                    replacement: Some(id(&name).at(tokens[index].line)),
                });
                // Delete only opaque arguments and their separators, so nested value calls
                // keep their own independent edits in this same round.
                let last_value = bindings.iter().rposition(Option::is_none);
                cursor = index + 2;
                for (argument, (arg, binding)) in args.iter().zip(&bindings).enumerate() {
                    if binding.is_some() {
                        edits.push(Edit {
                            item: item_index,
                            token: cursor,
                            replacement: None,
                        });
                    }
                    cursor += arg.len();
                    if argument + 1 < args.len()
                        && (binding.is_some() || Some(argument) == last_value)
                    {
                        edits.push(Edit {
                            item: item_index,
                            token: cursor,
                            replacement: None,
                        });
                    }
                    cursor += 1;
                }
                for (overload, template) in compatible {
                    if self.created.insert((name.clone(), overload)) {
                        // Substitution can lengthen names. Reserve an upper bound before
                        // cloning the body, including names made for colliding locals.
                        let identifier_bytes = bindings
                            .iter()
                            .flatten()
                            .map(String::len)
                            .chain([format!("iris_specialized_{}", usize::MAX).len()])
                            .max()
                            .unwrap_or(0);
                        budget.reserve(&template.tokens, identifier_bytes)?;
                        functions.push((
                            template.position,
                            Item::Func {
                                name: name.clone(),
                                toks: instantiate(template, &name, &bindings, &mut self.occupied),
                            },
                        ));
                    }
                }
            }
        }
        let changed = !edits.is_empty();
        edits.sort_unstable_by_key(|edit| std::cmp::Reverse((edit.item, edit.token)));
        for edit in edits {
            let tokens = match &mut items[edit.item].1 {
                Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
            };
            tokens.splice(edit.token..edit.token + 1, edit.replacement);
        }
        items.extend(functions);
        Ok(changed)
    }
}

/// Allocates an identifier absent from both the source and earlier generated code.
fn fresh_name(occupied: &mut HashSet<String>) -> String {
    let mut index = occupied.len();
    loop {
        let name = format!("iris_specialized_{index}");
        if occupied.insert(name.clone()) {
            return name;
        }
        index += 1;
    }
}

/// Removes opaque parameters and renames colliding value bindings before substitution.
fn instantiate(
    template: &Template,
    name: &str,
    bindings: &[Option<String>],
    occupied: &mut HashSet<String>,
) -> Vec<Tok> {
    let tokens = &template.tokens;
    let lp = tokens.iter().position(|token| token.is("(")).unwrap();
    let brace = tokens.iter().position(|token| token.is("{")).unwrap();
    let body = &tokens[brace..];
    let substitutions: HashMap<_, _> = template
        .params
        .iter()
        .zip(bindings)
        .filter_map(|(param, binding)| {
            binding
                .as_ref()
                .map(|name| (param.name.as_str(), name.as_str()))
        })
        .collect();
    let targets: HashSet<_> = substitutions.values().copied().collect();
    let parameters: HashSet<_> = template
        .params
        .iter()
        .filter(|param| !param.opaque)
        .map(|param| param.name.as_str())
        .collect();
    let locals = shadowed_bindings(body, targets.iter().copied());
    let shadowed = shadowed_bindings(body, substitutions.keys().copied());
    let mut renamed = HashMap::new();
    for binding in template
        .params
        .iter()
        .filter(|param| !param.opaque)
        .map(|param| param.name.as_str())
        .chain(
            body.iter()
                .enumerate()
                .filter(|(index, _)| locals.contains(index))
                .filter_map(|(_, token)| token.ident()),
        )
        .filter(|name| targets.contains(name))
    {
        renamed
            .entry(binding)
            .or_insert_with(|| fresh_name(occupied));
    }
    let line = tokens[lp].line;
    let mut out = tokens[..lp - 1].to_vec();
    out.extend([id(name).at(line), p("(").at(line)]);
    let mut first = true;
    for param in template.params.iter().filter(|param| !param.opaque) {
        if !first {
            out.push(p(",").at(line));
        }
        first = false;
        let mut declaration = param.toks.clone();
        declaration.push(p(";"));
        let declarators = local_declarators(&declaration);
        out.extend(param.toks.iter().enumerate().map(|(index, token)| {
            if declarators.contains(&index)
                && let Some(name) = renamed.get(param.name.as_str())
            {
                id(name).at(token.line)
            } else {
                token.clone()
            }
        }));
    }
    out.push(p(")").at(line));
    out.extend(body.iter().enumerate().map(|(index, token)| {
        let Some(name) = token
            .ident()
            .filter(|_| index == 0 || !body[index - 1].is("."))
        else {
            return token.clone();
        };
        if (parameters.contains(name) || locals.contains(&index))
            && let Some(name) = renamed.get(name)
        {
            id(name).at(token.line)
        } else if !shadowed.contains(&index)
            && let Some(name) = substitutions.get(name)
        {
            id(name).at(token.line)
        } else {
            token.clone()
        }
    }));
    out
}

/// Generates every resource-compatible overload and leaves value typing to naga.
pub(super) fn specialize_opaque_params(items: Vec<Item>) -> Result<Vec<Item>, TransformError> {
    specialize_with_limit(items, MAX_SPECIALIZATION_BYTES)
}

/// Specializes helpers within a cumulative token-storage budget shared by all rounds.
fn specialize_with_limit(items: Vec<Item>, limit: usize) -> Result<Vec<Item>, TransformError> {
    let mut budget = StorageBudget(limit);
    for item in &items {
        let tokens = match item {
            Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
        };
        budget.reserve(tokens, 0)?;
    }
    let mut pass = Specializer::new(&items);
    let mut items: Vec<_> = items.into_iter().enumerate().collect();
    for _ in 0..MAX_ROUNDS {
        if !pass.rewrite_calls(&mut items, &mut budget)? {
            break;
        }
    }
    // Keep helpers beside their original overloads and the declarations they depend on.
    items.sort_by_key(|(position, _)| *position);
    let mut items: Vec<_> = items.into_iter().map(|(_, item)| item).collect();
    // Rebuild opaque prototypes from the definitions that remain after specialization.
    items.retain(|item| {
        !matches!(item, Item::Other(tokens)
        if func_params(tokens).is_some_and(|(_, params)| params.iter().any(|param| param.opaque)))
    });
    let referenced = |name: &str| {
        items.iter().any(|item| {
            let tokens = match item {
                Item::Func { name: own, .. } if own == name => return false,
                Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
            };
            tokens
                .windows(2)
                .any(|pair| pair[0].ident() == Some(name) && pair[1].is("("))
        })
    };
    let keep: Vec<_> = items
        .iter()
        .map(|item| match item {
            Item::Func { name, toks }
                if pass.templates.contains_key(name)
                    && func_params(toks)
                        .is_some_and(|(_, params)| params.iter().any(|param| param.opaque)) =>
            {
                referenced(name)
            }
            _ => true,
        })
        .collect();
    let mut keep = keep.into_iter();
    items.retain(|_| keep.next().unwrap());
    let prototypes: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let Item::Func { name, toks } = item else {
                return None;
            };
            if !pass.instances.values().any(|generated| generated == name)
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
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::super::items::{parse_decl, split_items};
    use super::*;
    use crate::token::lex;

    /// Builds a program whose two resource bindings require distinct helper copies.
    fn fixture() -> Vec<Item> {
        split_items(&lex("uniform sampler2D a, b;
            vec4 read(sampler2D source) { return texture(source, vec2(0.5)); }
            void main() { vec4 result = read(a) + read(b); }"))
        .into_iter()
        .map(|tokens| {
            if let Some(decl) = parse_decl(&tokens) {
                Item::Decl(decl, tokens)
            } else {
                let open = super::super::func_params(&tokens).unwrap().0;
                let name = tokens[open - 1].ident().unwrap().to_string();
                Item::Func { name, toks: tokens }
            }
        })
        .collect()
    }

    #[test]
    fn helper_copies_cannot_exceed_the_translation_storage_budget() {
        let items = fixture();
        let bytes = items
            .iter()
            .flat_map(|item| match item {
                Item::Func { toks, .. } | Item::Decl(_, toks) | Item::Other(toks) => toks,
            })
            .map(|token| {
                size_of::<Tok>()
                    + if matches!(token.kind, crate::token::TokKind::Punct(_)) {
                        0
                    } else {
                        token.text().len()
                    }
            })
            .sum();
        let result = specialize_with_limit(items, bytes);
        assert!(result.is_err(), "specialization must reject excess storage");
        assert!(specialize_with_limit(fixture(), usize::MAX).is_ok());
    }
}
