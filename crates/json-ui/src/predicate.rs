//! The UI expression language shared by `ignored`, `variables[]` `requires` and
//! data bindings, following the client's `UIEval`/`UiExpression`: tokens split
//! at spaces, parentheses, `$` and operator characters; literals typed as int,
//! float, bool or string; prefix `+`/`-`/`not` bound tightest; then `*` `/`,
//! `+` `-`, `=` `<` `>`, and `and` `or`, all left-associative. Values carry the
//! client's JSON types (int32, float32, string, bool, other) and each operator
//! its per-type rules. A `#name` reads the property bag (missing reads null);
//! with no bag at all the expression is undecidable. An unbound `$var` is null.

use std::cell::RefCell;
use std::sync::Arc;

use serde_json::Value;

use crate::env::Env;
use crate::lru::Lru;

mod ops;
mod token;

pub(crate) use token::Operand;
use token::Token;

/// Bounds on a server-supplied expression; beyond any of them it is undecidable.
pub(crate) const MAX_BYTES: usize = 64 * 1024;
const MAX_TOKENS: usize = 16 * 1024;
pub(crate) const MAX_NESTING: usize = 512;
/// Parsed expressions kept per thread; a pack's distinct expressions fit.
const PARSE_CACHE: usize = 4096;

/// A property-bag value: what a binding writes and an expression reads, typed
/// as the client's JSON value is.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Bool(bool),
    Text(String),
    /// A real number.
    Num(f64),
    /// An integral number.
    Int(i64),
    /// Null, an array or an object.
    Json(Value),
}

impl Scalar {
    /// The boolean reading, honouring `"true"`/`"false"` text; `None` otherwise.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Scalar::Bool(value) => Some(*value),
            Scalar::Text(text) => match text.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// The value as JSON, integral numbers staying integers.
    pub fn to_json(&self) -> Value {
        match self {
            Scalar::Bool(value) => Value::Bool(*value),
            Scalar::Text(text) => Value::String(text.clone()),
            Scalar::Num(number) => serde_json::Number::from_f64(*number)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Scalar::Int(number) => Value::from(*number),
            Scalar::Json(value) => value.clone(),
        }
    }

    /// A JSON value as a bag value, keeping integers apart from reals.
    pub fn from_json(value: &Value) -> Scalar {
        match value {
            Value::Bool(flag) => Scalar::Bool(*flag),
            Value::String(text) => Scalar::Text(text.clone()),
            Value::Number(number) => match number.as_i64() {
                Some(int) => Scalar::Int(int),
                None if number.is_u64() => Scalar::Json(value.clone()),
                None => Scalar::Num(number.as_f64().unwrap_or(0.0)),
            },
            other => Scalar::Json(other.clone()),
        }
    }

    /// The number a numeric value holds.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Scalar::Num(number) => Some(*number),
            Scalar::Int(number) => Some(*number as f64),
            Scalar::Json(Value::Number(number)) => number.as_f64(),
            _ => None,
        }
    }
}

/// Resolves `#name` reads for an expression: a control's property bag.
pub trait Bindings {
    fn get(&self, name: &str) -> Option<Scalar>;

    /// Whether there is a bag at all; without one a `#name` is undecidable.
    fn has_bag(&self) -> bool {
        true
    }
}

/// No property bag: every `#name` is undecidable. `ignored`/`requires` never
/// read runtime bindings.
pub struct NoBindings;

impl Bindings for NoBindings {
    fn get(&self, _name: &str) -> Option<Scalar> {
        None
    }

    fn has_bag(&self) -> bool {
        false
    }
}

/// Evaluate a boolean predicate with no binding scope, or `None` when undecidable.
pub fn eval(expression: &str, env: &Env) -> Option<bool> {
    eval_bool(expression, env, &NoBindings)
}

/// Evaluate a predicate to a boolean against `bindings`: a bool, `"true"`/
/// `"false"` text, or a nonzero number; `None` when undecidable.
pub fn eval_bool(expression: &str, env: &Env, bindings: &dyn Bindings) -> Option<bool> {
    match evaluate(expression, env, bindings, 0)? {
        Operand::Bool(value) => Some(value),
        Operand::Int(value) => Some(value != 0),
        Operand::Float(value) => Some(value != 0.0),
        Operand::Str(text) => match text.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        Operand::Other(Value::Null) => Some(false),
        _ => None,
    }
}

/// Evaluate an expression to the value the client's evaluator returns, or
/// `None` when undecidable.
pub fn eval_scalar(expression: &str, env: &Env, bindings: &dyn Bindings) -> Option<Scalar> {
    evaluate(expression, env, bindings, 0).map(|operand| Scalar::from_json(&ops::to_json(operand)))
}

/// The `#name` tokens an expression reads, in order, nested groups included.
pub(crate) fn property_tokens(expression: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(tokens) = parsed(expression) {
        collect_properties(&tokens, &mut names);
    }
    names
}

/// Evaluate `expression` with its first `rewrite` `#name` tokens replaced by
/// the token `replacement` parses to, as a global binding rewrites its
/// expression to read the property it writes; `None` when undecidable.
pub(crate) fn eval_rewritten(
    expression: &str,
    rewrite: usize,
    replacement: &str,
    env: &Env,
    bindings: &dyn Bindings,
) -> Option<Scalar> {
    let mut tokens = (*parsed(expression)?).clone();
    let with = match replacement {
        "" => Token::Value(Operand::Str(String::new())),
        text => token::parse_word(text),
    };
    rewrite_properties(&mut tokens, &mut rewrite.clone(), &with);
    let scope = EvalScope {
        env,
        bindings,
        depth: 0,
    };
    let result = ops::resolve_final(ops::evaluate(&tokens, &scope)?, &scope)?;
    Some(Scalar::from_json(&ops::to_json(result)))
}

fn rewrite_properties(tokens: &mut [Token], left: &mut usize, with: &Token) {
    for token in tokens {
        if *left == 0 {
            return;
        }
        match token {
            Token::Property(_) => {
                *token = with.clone();
                *left -= 1;
            }
            Token::Group(inner) => rewrite_properties(inner, left, with),
            _ => {}
        }
    }
}

fn collect_properties(tokens: &[Token], names: &mut Vec<String>) {
    for token in tokens {
        match token {
            Token::Property(name) => names.push(name.clone()),
            Token::Group(inner) => collect_properties(inner, names),
            _ => {}
        }
    }
}

thread_local! {
    static CACHE: RefCell<Lru<String, Option<Arc<Vec<Token>>>>> = RefCell::new(Lru::new(PARSE_CACHE));
}

/// The tokens of `expression`, parsed once per thread.
fn parsed(expression: &str) -> Option<Arc<Vec<Token>>> {
    if expression.len() > MAX_BYTES {
        return None;
    }
    CACHE.with(|cache| {
        if let Some(hit) = cache.borrow().get(expression) {
            return hit.clone();
        }
        let tokens = token::tokenize(expression).map(Arc::new);
        cache
            .borrow_mut()
            .insert(expression.to_owned(), tokens.clone());
        tokens
    })
}

fn evaluate(expression: &str, env: &Env, bindings: &dyn Bindings, depth: usize) -> Option<Operand> {
    if depth >= MAX_NESTING {
        return None;
    }
    let tokens = parsed(expression)?;
    let scope = EvalScope {
        env,
        bindings,
        depth,
    };
    let result = ops::evaluate(&tokens, &scope)?;
    ops::resolve_final(result, &scope)
}

struct EvalScope<'a> {
    env: &'a Env,
    bindings: &'a dyn Bindings,
    depth: usize,
}

impl ops::Scope for EvalScope<'_> {
    fn result_property(&self, name: &str) -> Option<Operand> {
        if !self.bindings.has_bag() {
            // A property read without a bag yields the name as text.
            return Some(Operand::Str(name.to_owned()));
        }
        self.property(name)
    }

    fn property(&self, name: &str) -> Option<Operand> {
        if !self.bindings.has_bag() {
            return None;
        }
        Some(match self.bindings.get(name) {
            Some(value) => Operand::from_json(&value.to_json()),
            None => Operand::Other(Value::Null),
        })
    }

    fn variable(&self, name: &str) -> Option<Operand> {
        let name = name.split_once('|').map_or(name, |(name, _)| name);
        let Some(value) = self.env.get(name) else {
            return Some(Operand::Other(Value::Null));
        };
        match value {
            // A variable naming a binding reads it, so a resolve-time fold
            // leaves `(not $selected)` for the binder.
            Value::String(text) if text.starts_with('#') => self.property(text),
            // A variable holding a parenthesised expression evaluates it, as
            // `$include_world_section: "($a and $b)"` does in vanilla.
            Value::String(text) if is_expression(text) => {
                let result = evaluate(text, self.env, self.bindings, self.depth + 1)?;
                Some(Operand::from_json(&ops::to_json(result)))
            }
            other => Some(Operand::from_json(other)),
        }
    }
}

/// `(...)` wrapping the whole text: an expression, not a string value.
fn is_expression(text: &str) -> bool {
    let text = text.trim();
    text.len() > 2 && text.starts_with('(') && text.ends_with(')')
}

#[cfg(test)]
mod tests;
