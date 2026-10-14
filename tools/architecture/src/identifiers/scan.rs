use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    visit::{self, Visit},
};

use super::Vocabulary;
use super::scopes::attribute_scopes;
use crate::rust_scope::{ModulePaths, marks_test_scope};

/// One source occurrence; overlapping rejection shapes count only once.
pub(super) struct Violation {
    pub(super) line: usize,
    pub(super) literal: String,
    pub(super) test: bool,
}

/// Collects literal occurrences and module edges while retaining test scope.
pub(super) struct RustScan<'a> {
    vocabulary: &'a Vocabulary,
    features: &'a BTreeSet<String>,
    in_test: bool,
    bare_context: bool,
    paths: ModulePaths,
    pub(super) file_test: bool,
    pub(super) modules: Vec<(PathBuf, bool)>,
    pub(super) violations: BTreeMap<(usize, usize), Violation>,
}

impl<'a> RustScan<'a> {
    /// Starts a production scan; file-level test attributes are applied by the visitor.
    pub(super) fn new(
        path: &Path,
        crate_root: bool,
        features: &'a BTreeSet<String>,
        vocabulary: &'a Vocabulary,
    ) -> Self {
        Self {
            vocabulary,
            features,
            in_test: false,
            bare_context: false,
            paths: if crate_root {
                ModulePaths::crate_root(path)
            } else {
                ModulePaths::new(path)
            },
            file_test: false,
            modules: Vec::new(),
            violations: BTreeMap::new(),
        }
    }

    /// Applies a node's test attributes for exactly the duration of its traversal.
    fn with_attributes(&mut self, attributes: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        let previous = self.in_test;
        self.in_test |= marks_test_scope(attributes, self.features);
        visit(self);
        self.in_test = previous;
    }

    /// Records a vocabulary literal or a repeated proper affix, once per token position.
    fn record(&mut self, literal: &syn::LitStr, affix: bool) {
        let value = literal.value();
        let identifier = (value.contains(':') && self.vocabulary.full.contains(&value))
            || (self.bare_context && self.vocabulary.bare.contains(&value));
        if identifier || (affix && self.vocabulary.affixes.contains(&value)) {
            let position = literal.span().start();
            self.violations
                .entry((position.line, position.column))
                .or_insert(Violation {
                    line: position.line,
                    literal: value,
                    test: self.in_test,
                });
        }
    }

    /// Applies a bare-name context to one subtree and restores its enclosing context.
    fn with_bare(&mut self, bare: bool, visit: impl FnOnce(&mut Self)) {
        let previous = self.bare_context;
        self.bare_context |= bare;
        visit(self);
        self.bare_context = previous;
    }

    /// Visits a method argument with the identifier and affix rules for that method.
    fn argument(&mut self, method: &str, argument: &syn::Expr) {
        self.with_bare(method == "contains", |scan| scan.visit_expr(argument));
        if is_affix_method(method)
            && let Some(literal) = string_argument(argument)
        {
            self.record(literal, true);
        }
    }

    /// Parses ordinary macro expressions and `matches!` patterns before a token fallback.
    fn macro_tokens(&mut self, name: &str, tokens: TokenStream) {
        if name == "matches"
            && let Ok(arguments) = syn::parse2::<Matches>(tokens.clone())
        {
            self.visit_expr(&arguments.value);
            self.with_bare(true, |scan| scan.visit_pat(&arguments.pattern));
            if let Some(guard) = &arguments.guard {
                self.visit_expr(guard);
            }
            return;
        }
        if let Ok(expressions) = syn::parse::Parser::parse2(
            Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
            tokens.clone(),
        ) {
            self.with_bare(name == "vec", |scan| {
                for expression in expressions {
                    scan.visit_expr(&expression);
                }
            });
            return;
        }
        self.with_bare(name == "vec", |scan| {
            scan.fallback_tokens(tokens, name == "matches");
        });
    }

    /// Handles macro templates and other token grammars without expanding macro code.
    fn fallback_tokens(&mut self, tokens: TokenStream, matches: bool) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        let mut pattern = false;
        let mut index = 0;
        while index < tokens.len() {
            match &tokens[index] {
                TokenTree::Punct(punct) if matches && punct.as_char() == ',' => pattern = true,
                TokenTree::Ident(ident) if matches && ident == "if" => pattern = false,
                TokenTree::Ident(ident) => {
                    if let [TokenTree::Punct(bang), TokenTree::Group(group), ..] =
                        &tokens[index + 1..]
                        && bang.as_char() == '!'
                    {
                        self.macro_tokens(&ident.to_string(), group.stream());
                        index += 2;
                    } else if let Some(TokenTree::Group(group)) = tokens.get(index + 1)
                        && group.delimiter() == Delimiter::Parenthesis
                        && is_affix_method(&ident.to_string())
                    {
                        let method = ident.to_string();
                        if let Ok(arguments) = syn::parse::Parser::parse2(
                            Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
                            group.stream(),
                        ) {
                            for argument in arguments {
                                self.argument(&method, &argument);
                            }
                        } else {
                            self.fallback_tokens(group.stream(), false);
                        }
                        index += 1;
                    }
                }
                TokenTree::Group(group) => {
                    self.with_bare(pattern || group.delimiter() == Delimiter::Bracket, |scan| {
                        if group.delimiter() == Delimiter::Brace
                            && let Ok(block) =
                                syn::parse2::<syn::Block>(TokenTree::Group(group.clone()).into())
                        {
                            scan.visit_block(&block);
                        } else {
                            scan.macro_tokens("", group.stream());
                        }
                    });
                }
                TokenTree::Literal(token) => {
                    if let Ok(literal) =
                        syn::parse2::<syn::LitStr>(TokenTree::Literal(token.clone()).into())
                    {
                        let comparison = adjacent_comparison(&tokens, index);
                        self.with_bare(pattern || comparison, |scan| scan.record(&literal, false));
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
}

/// Recognizes the string methods whose literal arguments can encode identity heuristics.
fn is_affix_method(name: &str) -> bool {
    matches!(
        name,
        "ends_with" | "starts_with" | "strip_suffix" | "strip_prefix" | "contains"
    ) || name == "trim_matches"
        || (name.starts_with("trim_") && name.ends_with("_matches"))
}

/// Unwraps parentheses, groups and references around a literal method argument.
fn string_argument(expression: &syn::Expr) -> Option<&syn::LitStr> {
    match expression {
        syn::Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Str(string) => Some(string),
            _ => None,
        },
        syn::Expr::Paren(paren) => string_argument(&paren.expr),
        syn::Expr::Group(group) => string_argument(&group.expr),
        syn::Expr::Reference(reference) => string_argument(&reference.expr),
        _ => None,
    }
}

/// Recognizes immediate equality operands in otherwise unparseable macro templates.
fn adjacent_comparison(tokens: &[TokenTree], index: usize) -> bool {
    let comparison = |pair: &[TokenTree]| {
        matches!(pair,
        [TokenTree::Punct(first), TokenTree::Punct(second)]
            if matches!(first.as_char(), '=' | '!') && second.as_char() == '=')
    };
    (index >= 2 && comparison(&tokens[index - 2..index]))
        || (index + 2 < tokens.len() && comparison(&tokens[index + 1..index + 3]))
}

/// The separate expression, pattern and optional guard of `matches!`.
struct Matches {
    value: syn::Expr,
    pattern: syn::Pat,
    guard: Option<syn::Expr>,
}

impl Parse for Matches {
    /// Parses a pattern without treating the scrutinee or guard as pattern literals.
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let value = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            input.parse::<syn::Token![if]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        Ok(Self {
            value,
            pattern,
            guard,
        })
    }
}

impl RustScan<'_> {
    /// Remembers file-level test scope before visiting its contents.
    fn scan_file(&mut self, file: &syn::File) {
        self.file_test |= self.in_test;
        visit::visit_file(self, file);
    }

    /// Tracks ordinary and explicitly named module files with their declaration scope.
    fn scan_module(&mut self, module: &syn::ItemMod) {
        if module.content.is_some() {
            let previous = self.paths.clone();
            self.paths.push_inline(&module.ident.to_string());
            visit::visit_item_mod(self, module);
            self.paths = previous;
        } else if let Some(path) = self.paths.resolve(module) {
            self.modules.push((path, self.in_test));
        }
    }

    /// Scans a free constant's value as an identity table.
    fn scan_const(&mut self, item: &syn::ItemConst) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_type(&item.ty);
        self.visit_generics(&item.generics);
        self.with_bare(true, |scan| scan.visit_expr(&item.expr));
    }

    /// Scans a static value as an identity table.
    fn scan_static(&mut self, item: &syn::ItemStatic) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_type(&item.ty);
        self.with_bare(true, |scan| scan.visit_expr(&item.expr));
    }

    /// Scans an impl constant's value as an identity table.
    fn scan_impl_const(&mut self, item: &syn::ImplItemConst) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_type(&item.ty);
        self.visit_generics(&item.generics);
        self.with_bare(true, |scan| scan.visit_expr(&item.expr));
    }

    /// Scans a trait constant's default value as an identity table.
    fn scan_trait_const(&mut self, item: &syn::TraitItemConst) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_type(&item.ty);
        self.visit_generics(&item.generics);
        if let Some((_, value)) = &item.default {
            self.with_bare(true, |scan| scan.visit_expr(value));
        }
    }

    /// Checks literal method arguments and visits the receiver in its normal context.
    fn scan_method_call(&mut self, call: &syn::ExprMethodCall) {
        for attribute in &call.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_expr(&call.receiver);
        if let Some(arguments) = &call.turbofish {
            self.visit_angle_bracketed_generic_arguments(arguments);
        }
        for argument in &call.args {
            self.argument(&call.method.to_string(), argument);
        }
    }

    /// Checks fully qualified string-method calls as well as ordinary function syntax.
    fn scan_call(&mut self, call: &syn::ExprCall) {
        for attribute in &call.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_expr(&call.func);
        let method = match &*call.func {
            syn::Expr::Path(path) => path.path.segments.last().map(|part| part.ident.to_string()),
            _ => None,
        };
        for argument in &call.args {
            if let Some(method) = &method
                && is_affix_method(method)
            {
                self.argument(method, argument);
            } else {
                self.visit_expr(argument);
            }
        }
    }
}

impl<'ast> Visit<'ast> for RustScan<'_> {
    attribute_scopes!('ast);

    /// Checks attribute values while leaving documentation prose out of the scan.
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if attribute.path().is_ident("doc") {
            return;
        }
        match &attribute.meta {
            syn::Meta::List(list) => self.macro_tokens("", list.tokens.clone()),
            syn::Meta::NameValue(pair) => self.visit_expr(&pair.value),
            syn::Meta::Path(_) => {}
        }
    }

    /// Rejects full identifiers everywhere and bare names in selector and table contexts.
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.record(literal, false);
    }

    /// Applies the same bare-name context to every Rust pattern position.
    fn visit_pat(&mut self, pattern: &'ast syn::Pat) {
        self.with_bare(true, |scan| visit::visit_pat(scan, pattern));
    }

    /// Arrays and comparisons select identities without changing unrelated expression contexts.
    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        let bare = matches!(
            expression,
            syn::Expr::Array(_) | syn::Expr::Repeat(_) | syn::Expr::Const(_)
        ) || matches!(expression, syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)));
        self.with_bare(bare, |scan| visit::visit_expr(scan, expression));
    }

    /// Scans macro arguments without executing or expanding macros.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.macro_tokens(
            &mac.path
                .segments
                .last()
                .map_or(String::new(), |part| part.ident.to_string()),
            mac.tokens.clone(),
        );
    }
}
