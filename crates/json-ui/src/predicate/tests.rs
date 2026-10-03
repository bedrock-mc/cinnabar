use std::collections::BTreeMap;

use serde_json::json;

use super::{
    Bindings, MAX_BYTES, MAX_NESTING, Scalar, eval, eval_bool, eval_rewritten, eval_scalar,
};
use crate::env::Env;

struct Map(BTreeMap<String, Scalar>);

impl Bindings for Map {
    fn get(&self, name: &str) -> Option<Scalar> {
        self.0.get(name).cloned()
    }
}

fn bindings(entries: &[(&str, Scalar)]) -> Map {
    Map(entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect())
}

fn env() -> Env {
    let mut env = Env::new();
    env.set("desktop_screen", json!(true));
    env.set("pocket_screen", json!(false));
    env.set("use_custom_title_control", json!(false));
    env.set("show_close_button", json!(true));
    env.set("banner_text_binding_name", json!(""));
    env
}

/// The audit's fixture bag.
fn bag() -> Map {
    bindings(&[
        ("#a", Scalar::Int(2)),
        ("#b", Scalar::Int(3)),
        ("#x", Scalar::Text("hello".into())),
        ("#custom:flag", Scalar::Bool(true)),
    ])
}

fn value(expression: &str) -> Option<Scalar> {
    eval_scalar(expression, &env(), &bag())
}

fn text(value: &str) -> Option<Scalar> {
    Some(Scalar::Text(value.into()))
}

// A variable holding an expression evaluates it; a self-reference stays undecidable.
#[test]
fn expression_variables_evaluate() {
    let mut env = Env::new();
    env.set("a", json!(false));
    env.set("b", json!(true));
    env.set("both", json!("($a and $b)"));
    env.set("loop", json!("(not $loop)"));
    assert_eq!(eval("(not $both)", &env), Some(true));
    assert_eq!(eval("$loop", &env), None);
    assert_eq!(eval("($a = '')", &env), Some(true));
}

#[test]
fn predicates_over_variables() {
    assert_eq!(eval("(not $use_custom_title_control)", &env()), Some(true));
    assert_eq!(eval("$use_custom_title_control", &env()), Some(false));
    assert_eq!(eval("(not $show_close_button)", &env()), Some(false));
    assert_eq!(
        eval("$desktop_screen and (not $pocket_screen)", &env()),
        Some(true)
    );
    assert_eq!(
        eval("($pocket_screen or $desktop_screen)", &env()),
        Some(true)
    );
    assert_eq!(eval("($banner_text_binding_name = '')", &env()), Some(true));
}

// An unbound `$var` is null as in `UIEval::evalVariable`; with no bag a `#name`
// is undecidable.
#[test]
fn unbound_variable_is_null_and_bagless_binding_undecidable() {
    assert_eq!(eval("$never_set", &env()), Some(false));
    assert_eq!(eval("(not $never_set)", &env()), Some(true));
    assert_eq!(
        eval("($desktop_screen and $never_set)", &env()),
        Some(false)
    );
    assert_eq!(eval("($never_set = '')", &env()), Some(true));
    assert_eq!(eval("($never_set = 0)", &env()), Some(true));
    assert_eq!(eval("(not #visible)", &env()), None);
}

// A property missing from the bag reads null, never undecidable.
#[test]
fn missing_property_reads_null() {
    let scope = bindings(&[("#other", Scalar::Bool(true))]);
    assert_eq!(
        eval_bool("(not (#texture = ''))", &env(), &scope),
        Some(false)
    );
    assert_eq!(
        eval_scalar("#texture", &env(), &scope),
        Some(Scalar::Json(json!(null)))
    );
}

#[test]
fn view_binding_drives_visibility() {
    let present = bindings(&[("#texture", Scalar::Text("textures/x".into()))]);
    let empty = bindings(&[("#texture", Scalar::Text(String::new()))]);
    let loading = bindings(&[("#texture", Scalar::Text("loading".into()))]);
    let expr = "(not ((#texture = '') or (#texture = 'loading')))";
    assert_eq!(eval_bool(expr, &env(), &present), Some(true));
    assert_eq!(eval_bool(expr, &env(), &empty), Some(false));
    assert_eq!(eval_bool(expr, &env(), &loading), Some(false));
}

#[test]
fn string_subtraction_detects_and_strips_markers() {
    let scope = bindings(&[("#t", Scalar::Text("@mineville/boxes:Spirit Bundle".into()))]);
    let strip = eval_scalar("(#t - '@mineville/boxes' - ':')", &env(), &scope);
    assert_eq!(strip, text("Spirit Bundle"));
    let found = "(not ((#t - '@mineville/boxes') = #t))";
    assert_eq!(eval_bool(found, &env(), &scope), Some(true));
    let absent = "(not ((#t - '§j' - '§z') = #t))";
    assert_eq!(eval_bool(absent, &env(), &scope), Some(false));
}

// E01, E12, E13, E24, E25: grouping, one and/or level, binary precedence,
// division by zero keeping the left side, integer text.
#[test]
fn precedence_and_numeric_basics() {
    let scope = bindings(&[("#n", Scalar::Num(7.0))]);
    let eval = |expression| eval_scalar(expression, &env(), &scope);
    assert_eq!(eval("(#n - 1) * 2 + 3"), Some(Scalar::Num(15.0)));
    assert_eq!(eval("true or false and false"), Some(Scalar::Bool(false)));
    assert_eq!(eval("-2 + #n"), Some(Scalar::Num(5.0)));
    assert_eq!(eval("#n / 0"), Some(Scalar::Num(7.0)));
    assert_eq!(value("('v' + 12)"), text("v12"));
    assert_eq!(value("(#a + #b)"), Some(Scalar::Int(5)));
}

// E06, E17: int32 and float32 literal types; integer division; a literal parses
// its leading integer as `strtol` does.
#[test]
fn numeric_types_follow_int32_and_float32() {
    assert_eq!(value("(5 / 2)"), Some(Scalar::Int(2)));
    assert_eq!(value("(5 / .5)"), Some(Scalar::Num(10.0)));
    assert_eq!(value("(1.5)"), Some(Scalar::Int(1)));
    assert_eq!(value("100%"), Some(Scalar::Int(100)));
    assert_eq!(value("(2147483647 + 1)"), Some(Scalar::Int(-2147483648)));
}

// E14: numeric comparison is float32.
#[test]
fn numeric_comparison_is_float32() {
    assert_eq!(value("(16777216 = 16777217)"), Some(Scalar::Bool(true)));
    assert_eq!(value("(2 < 3)"), Some(Scalar::Bool(true)));
    assert_eq!(value("('b' > 'a')"), Some(Scalar::Bool(true)));
}

// E11: logical operators read JSON truthiness.
#[test]
fn logical_operators_use_json_truthiness() {
    assert_eq!(value("('abc' and true)"), Some(Scalar::Bool(true)));
    assert_eq!(value("(not 'false')"), Some(Scalar::Bool(false)));
    assert_eq!(value("(not '')"), Some(Scalar::Bool(true)));
}

// E15, E16: mixed-type equality and ordering.
#[test]
fn mixed_type_comparisons() {
    assert_eq!(value("('2' = 2)"), Some(Scalar::Bool(false)));
    assert_eq!(value("(true = 'true')"), Some(Scalar::Bool(false)));
    assert_eq!(value("('' = 0)"), Some(Scalar::Bool(true)));
    assert_eq!(value("(true = 1)"), Some(Scalar::Bool(true)));
    assert_eq!(value("(true < 2)"), Some(Scalar::Bool(true)));
    assert_eq!(value("('a' > false)"), Some(Scalar::Bool(true)));
}

// E08, E10: prefix operators reduce as soon as their operand arrives.
#[test]
fn prefix_operators_bind_tightest() {
    assert_eq!(value("(not 1 < 2)"), Some(Scalar::Bool(true)));
    assert_eq!(value("(-#a)"), Some(Scalar::Int(-2)));
    assert_eq!(value("(+2)"), Some(Scalar::Int(2)));
    assert_eq!(value("(-(1 + 2))"), Some(Scalar::Int(-3)));
    assert_eq!(value("(-.5)"), Some(Scalar::Num(-0.5)));
    assert_eq!(value("(1 - -2)"), Some(Scalar::Int(3)));
}

// E18: bools add as integers.
#[test]
fn boolean_arithmetic_is_integral() {
    assert_eq!(value("(true + true)"), Some(Scalar::Int(2)));
}

// E03, E07: double quotes and case-insensitive bool words.
#[test]
fn literal_spellings() {
    assert_eq!(value("(\"ab\" + \"cd\")"), text("abcd"));
    assert_eq!(value("(not TRUE)"), Some(Scalar::Bool(false)));
    assert_eq!(value("(not yes)"), Some(Scalar::Bool(false)));
    assert_eq!(value("(No)"), Some(Scalar::Bool(false)));
}

// E19-E23: string operators, float text reading empty, full reparse.
#[test]
fn string_operators_follow_the_vanilla_rules() {
    assert_eq!(value("('v=' + 1.5)"), text("v=1"));
    assert_eq!(value("('v=' + .5)"), text("v="));
    assert_eq!(value("(('x-1' - 'x') = -1)"), Some(Scalar::Bool(true)));
    assert_eq!(value("('ye' + 's')"), Some(Scalar::Bool(true)));
    assert_eq!(value("('#' + 'x')"), text("hello"));
    assert_eq!(value("(('aaa' / 'a') / 2)"), Some(Scalar::Int(1)));
    assert_eq!(value("(2 * 'abc')"), Some(Scalar::Int(2)));
    assert_eq!(value("('%.2s' * 'éé')"), text("é"));
    assert_eq!(value("('%.3s' * 'abcdef')"), text("abc"));
    assert_eq!(value("('a-b-a' / 'a')"), Some(Scalar::Int(2)));
    assert_eq!(value("('x12' - 'x')"), Some(Scalar::Int(12)));
    assert_eq!(value("('ab' - 3)"), text("ab"));
    assert_eq!(value("('abc' * 'xyz')"), text("xyz"));
}

// E05: any non-delimiter byte belongs to a property token.
#[test]
fn property_alphabet_includes_punctuation() {
    assert_eq!(value("(#custom:flag)"), Some(Scalar::Bool(true)));
    let scope = bindings(&[("#naïve", Scalar::Int(1))]);
    assert_eq!(
        eval_scalar("(#naïve + 1)", &env(), &scope),
        Some(Scalar::Int(2))
    );
}

// E27: `<=` is two operators, an ill-formed expression that reads null.
#[test]
fn no_combined_comparison_operators() {
    assert_eq!(value("(1 <= 2)"), Some(Scalar::Json(json!(null))));
}

// E26: nesting well past 64 still evaluates; server-supplied depth stays bounded.
#[test]
fn nesting_bounds() {
    let deep = format!("{}true{}", "(".repeat(65), ")".repeat(65));
    assert_eq!(eval(&deep, &env()), Some(true));
    let nots = format!("{}true", "not ".repeat(10_000));
    assert_eq!(eval(&nots, &env()), Some(true));
    let parens = format!("{}true{}", "(".repeat(20_000), ")".repeat(20_000));
    assert_eq!(eval(&parens, &env()), None);
    let within = MAX_NESTING - 1;
    let parens = format!("{}true{}", "(".repeat(within), ")".repeat(within));
    assert_eq!(eval(&parens, &env()), Some(true));
    let long = format!("'{}' = ''", "x".repeat(MAX_BYTES + 1));
    assert_eq!(eval(&long, &env()), None);
}

// A quoted `$` stays text.
#[test]
fn quoted_dollar_is_literal() {
    assert_eq!(value("('$5' + '!')"), text("$5!"));
}

// A global binding's expression reads the property it writes.
#[test]
fn rewritten_expression_reads_the_target() {
    let scope = bindings(&[("#visible", Scalar::Bool(true))]);
    let result = eval_rewritten("(not #x)", 1, "#visible", &env(), &scope);
    assert_eq!(result, Some(Scalar::Bool(false)));
    let both = eval_rewritten(
        "(#a and #b)",
        1,
        "#t",
        &env(),
        &bindings(&[("#t", Scalar::Bool(true))]),
    );
    assert_eq!(both, Some(Scalar::Bool(false)));
}

#[test]
fn review_oversized_expression_is_never_retained_in_the_parse_cache() {
    let expression = "x".repeat(MAX_BYTES + 1);
    assert!(super::parsed(&expression).is_none());
    super::CACHE.with(|cache| assert!(!cache.borrow().contains_key(&expression)));
}
