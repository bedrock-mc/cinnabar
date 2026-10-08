//! Variable scopes and value evaluation as the vanilla client performs them.
//! Each control pushes a
//! frame of its `$` declarations over its parent's; names are exact, with the
//! `$` dropped. A lookup takes the nearest frame holding the name, and only when
//! no frame does, the nearest holding `name|default`.

use std::{collections::BTreeMap, sync::Arc};

use serde_json::Value;

/// A stack of variable frames, cheap to clone and extend.
#[derive(Clone, Debug, Default)]
pub struct Env {
    top: Option<Arc<Frame>>,
}

#[derive(Clone, Debug, Default)]
struct Frame {
    vars: BTreeMap<String, Value>,
    parent: Option<Arc<Frame>>,
}

impl Env {
    pub fn new() -> Self {
        Self::default()
    }

    /// The nearest concrete `name`, else the nearest `name|default`; a `null`
    /// value counts as unset.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.find(name)
            .or_else(|| self.find(&format!("{name}|default")))
    }

    fn find(&self, key: &str) -> Option<&Value> {
        let mut frame = self.top.as_deref();
        while let Some(current) = frame {
            if let Some(value) = current.vars.get(key).filter(|value| !value.is_null()) {
                return Some(value);
            }
            frame = current.parent.as_deref();
        }
        None
    }

    /// Set `name` (without `$`, any `|default` suffix kept) in the top frame.
    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        let top = self.top.get_or_insert_with(Arc::default);
        Arc::make_mut(top).vars.insert(name.into(), value);
    }

    /// A new, empty frame over this scope, for one control's declarations.
    pub(crate) fn child(&self) -> Env {
        Env {
            top: Some(Arc::new(Frame {
                vars: BTreeMap::new(),
                parent: self.top.clone(),
            })),
        }
    }

    /// Drop the top frame when it declared nothing, sharing the parent's.
    pub(crate) fn settle(self) -> Env {
        match &self.top {
            Some(top) if top.vars.is_empty() => Env {
                top: top.parent.clone(),
            },
            _ => self,
        }
    }

    /// Every variable in frames above `root`, nearest first per name.
    pub(crate) fn above(&self, root: &Env) -> BTreeMap<String, Value> {
        let stop = root.top.as_ref().map(Arc::as_ptr);
        let mut vars = BTreeMap::new();
        let mut frame = self.top.as_ref();
        while let Some(current) = frame
            && Some(Arc::as_ptr(current)) != stop
        {
            for (name, value) in &current.vars {
                vars.entry(name.clone()).or_insert_with(|| value.clone());
            }
            frame = current.parent.as_ref();
        }
        vars
    }
}

/// Field evaluation: a string starting with `$` reads that variable
/// (a `__string` wrapper yields its `value`, raw text skipping expressions); a
/// string starting with `(` is replaced by its value while it evaluates without
/// runtime bindings. Anything else, or a lookup that finds nothing, stays as written.
pub fn evaluate(value: &Value, env: &Env) -> Value {
    let Value::String(text) = value else {
        return value.clone();
    };
    let mut current = value.clone();
    if let Some(name) = text.strip_prefix('$') {
        let Some(found) = env.get(name) else {
            return value.clone();
        };
        match found {
            Value::Object(wrapper) if wrapper.get("__string") == Some(&Value::Bool(true)) => {
                let raw = wrapper.get("__rawtext") == Some(&Value::Bool(true));
                current = wrapper.get("value").cloned().unwrap_or(Value::Null);
                if raw {
                    return if current.is_null() {
                        value.clone()
                    } else {
                        current
                    };
                }
            }
            found => current = found.clone(),
        }
    }
    // A result that is itself an expression evaluates again.
    for _ in 0..MAX_FOLDS {
        let Value::String(expression) = &current else {
            break;
        };
        if !expression.starts_with('(') {
            break;
        }
        match crate::predicate::eval_scalar(expression, env, &crate::predicate::NoBindings) {
            Some(crate::predicate::Scalar::Json(_)) | None => break,
            Some(result) => current = result.to_json(),
        }
    }
    if current.is_null() {
        value.clone()
    } else {
        current
    }
}

/// How many times a folded expression's result may fold again.
const MAX_FOLDS: usize = 8;

/// A property value as its consumer reads it: strings evaluate, arrays and
/// objects evaluate member by member, and an expression still needing runtime
/// bindings gets its `$vars` written in as operands for the binder.
pub fn substitute(value: &Value, env: &Env, unresolved: &mut Vec<String>) -> Value {
    substitute_within(value, env, unresolved, 0)
}

/// How deep structured variable values are followed into.
const MAX_SUBSTITUTION_DEPTH: usize = 8;

fn substitute_within(
    value: &Value,
    env: &Env,
    unresolved: &mut Vec<String>,
    depth: usize,
) -> Value {
    match value {
        Value::String(text) => {
            let evaluated = evaluate(value, env);
            match &evaluated {
                Value::String(result) if result.starts_with('(') => {
                    Value::String(bind_operands(result, env))
                }
                Value::String(result) if result == text && text.starts_with('$') => {
                    unresolved.push(text[1..].to_owned());
                    evaluated
                }
                Value::Array(_) | Value::Object(_)
                    if text.starts_with('$') && depth < MAX_SUBSTITUTION_DEPTH =>
                {
                    substitute_within(&evaluated, env, unresolved, depth + 1)
                }
                _ => evaluated,
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute_within(item, env, unresolved, depth))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), substitute_within(item, env, unresolved, depth)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Write each `$var` operand of a binding expression in as the literal token its
/// value makes: a string quoted (a `#name` stays a binding), numbers and bools
/// spelled out, an unset variable `null`-like `''`. Quoted text is left alone.
fn bind_operands(expression: &str, env: &Env) -> String {
    let bytes = expression.as_bytes();
    let mut out = String::with_capacity(expression.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'\'' | b'"') => {
                let end = bytes[i + 1..]
                    .iter()
                    .position(|&byte| byte == quote)
                    .map_or(bytes.len(), |offset| i + 2 + offset);
                out.push_str(&expression[i..end]);
                i = end;
            }
            b'$' => {
                let start = i;
                i += 1;
                while i < bytes.len() && !is_operand_end(bytes[i]) {
                    i += 1;
                }
                let name = &expression[start + 1..i];
                match env.get(name) {
                    Some(value) => out.push_str(&operand_token(value)),
                    None => out.push_str(&expression[start..i]),
                }
            }
            _ => {
                let ch = expression[i..].chars().next().unwrap_or(' ');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

fn is_operand_end(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'$'
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b'-'
            | b'/'
            | b'<'
            | b'='
            | b'>'
            | b'\''
            | b'"'
    )
}

fn operand_token(value: &Value) -> String {
    match value {
        Value::String(text) if text.starts_with('#') => text.clone(),
        Value::String(text) if !text.contains('\'') => format!("'{text}'"),
        Value::String(text) => format!("\"{text}\""),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        _ => "''".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Env, evaluate, substitute};
    use serde_json::json;
    #[test]
    fn dotted_controller_variables_resolve_labels_and_button_targets() {
        // Vanilla global resources declares `$button.remove` and `$button.move_left`.
        let mut env = Env::new();
        env.set("button.remove", json!("resourcePack.selected.remove"));
        env.set("button.move_left", json!("button.move_left_global"));
        let mut missing = Vec::new();
        assert_eq!(
            substitute(&json!("$button.remove"), &env, &mut missing),
            json!("resourcePack.selected.remove")
        );
        assert_eq!(
            substitute(&json!("$button.move_left"), &env, &mut missing),
            json!("button.move_left_global")
        );
        assert!(missing.is_empty());
    }

    fn env() -> Env {
        let mut env = Env::new();
        env.set("title_size", json!(["100% - 15px", 10]));
        env.set("name", json!("#title_text"));
        env
    }

    // A child frame shares its parent until it declares; settling drops an empty one.
    #[test]
    fn frames_share_their_parent_until_they_declare() {
        let parent = env();
        let empty = parent.child().settle();
        assert!(std::ptr::eq(
            empty.top.as_deref().unwrap(),
            parent.top.as_deref().unwrap()
        ));
        let mut child = parent.child();
        child.set("name", json!("child"));
        let child = child.settle();
        assert_eq!(child.get("name"), Some(&json!("child")));
        assert_eq!(parent.get("name"), Some(&json!("#title_text")));
        assert_eq!(child.get("title_size"), parent.get("title_size"));
        assert_eq!(child.above(&parent).len(), 1);
    }

    // Any concrete value beats every default; among defaults the nearest wins.
    #[test]
    fn concrete_values_beat_defaults_at_any_depth() {
        let mut outer = Env::new();
        outer.set("x", json!("concrete"));
        outer.set("y|default", json!("outer"));
        let mut inner = outer.child();
        inner.set("x|default", json!("fallback"));
        inner.set("y|default", json!("inner"));
        assert_eq!(inner.get("x"), Some(&json!("concrete")));
        assert_eq!(inner.get("y"), Some(&json!("inner")));
        assert_eq!(inner.get("y|default"), Some(&json!("inner")));
        inner.set("z|weird", json!(1));
        assert_eq!(inner.get("z"), None);
    }

    #[test]
    fn exact_reference_preserves_array_type() {
        assert_eq!(
            substitute(&json!("$title_size"), &env(), &mut Vec::new()),
            json!(["100% - 15px", 10])
        );
    }

    #[test]
    fn arithmetic_strings_are_left_symbolic() {
        let out = substitute(&json!("100% - 15px"), &env(), &mut Vec::new());
        assert_eq!(out, json!("100% - 15px"));
    }

    #[test]
    fn unknown_variable_is_reported_and_kept() {
        let mut missing = Vec::new();
        let out = substitute(&json!("$nope"), &env(), &mut missing);
        assert_eq!(out, json!("$nope"));
        assert_eq!(missing, vec!["nope".to_owned()]);
    }

    #[test]
    fn parenthesised_expressions_fold_and_runtime_ones_bind_their_operands() {
        let mut env = Env::new();
        env.set("dropdown_name", json!("custom_dropdown"));
        env.set("selected", json!("#is_selected_slot"));
        env.set("boxes", json!("@mineville/boxes"));
        assert_eq!(
            evaluate(&json!("('#' + $dropdown_name)"), &env),
            json!("#custom_dropdown")
        );
        assert_eq!(evaluate(&json!("(Beta)"), &env), json!("Beta"));
        let runtime = json!("(not #enabled)");
        assert_eq!(substitute(&runtime, &env, &mut Vec::new()), runtime);
        assert_eq!(
            substitute(&json!("(not $selected)"), &env, &mut Vec::new()),
            json!("(not #is_selected_slot)")
        );
        assert_eq!(
            substitute(&json!("(not ((#t - $boxes) = #t))"), &env, &mut Vec::new()),
            json!("(not ((#t - '@mineville/boxes') = #t))")
        );
    }

    // A structured value's nested `$vars` resolve where it is consumed.
    #[test]
    fn a_substituted_value_has_its_own_variables_replaced() {
        let mut env = Env::new();
        env.set(
            "visible_binding",
            json!([{ "binding_type": "view", "source_property_name": "$condition" }]),
        );
        env.set("condition", json!("(#a = '')"));
        let out = substitute(&json!("$visible_binding"), &env, &mut Vec::new());
        assert_eq!(out[0]["source_property_name"], json!("(#a = '')"));
    }
}
