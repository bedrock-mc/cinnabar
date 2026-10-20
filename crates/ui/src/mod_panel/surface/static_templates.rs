//! A static local-template subset, evaluated by the owning JSON-UI resolver.
use super::{MAX_SURFACE_NODES, identifier};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
/// Parses a literal instance/template pair restricted to its owning namespace.
pub(super) fn local_reference(
    key: &str,
    namespace: &str,
) -> Result<(String, Option<String>), String> {
    let (name, reference) = key
        .split_once('@')
        .map_or((key, None), |(name, reference)| (name, Some(reference)));
    if !identifier(name) || name.starts_with('$') {
        return Err("invalid static surface control name".into());
    }
    let reference = reference
        .map(|reference| {
            let (owner, local) = reference.split_once('.').unwrap_or((namespace, reference));
            if owner != namespace || !identifier(local) || local.contains('.') {
                return Err("surface templates must reference the same local namespace".into());
            }
            Ok::<String, String>(local.to_owned())
        })
        .transpose()?;
    Ok((name.to_owned(), reference))
}
/// Bounds immutable parameters before the resolver can replicate them.
/// Runtime binding expressions remain literal; only exact scalar aliases may reference parameters.
/// Embedded references cannot inject a new expression into the owner's later evaluation folds.
fn parameter(value: &Value) -> Result<(), String> {
    match value {
        Value::Bool(_) | Value::Null => Ok(()),
        Value::Number(number)
            if number
                .as_f64()
                .is_some_and(|value| value.is_finite() && value.abs() <= 1_000_000.) =>
        {
            Ok(())
        }
        Value::String(text)
            if text.len() <= 1024
                && (!text.contains('$') || text.strip_prefix('$').is_some_and(identifier))
                && !text.chars().any(char::is_control) =>
        {
            Ok(())
        }
        Value::Array(values)
            if values.len() <= 4
                && values.iter().all(|value| {
                    !value.is_array()
                        && !value.is_object()
                        && !value.as_str().is_some_and(|text| text.starts_with('$'))
                }) =>
        {
            for value in values {
                parameter(value)?;
            }
            Ok(())
        }
        _ => Err("surface template parameter exceeds scalar bounds".into()),
    }
}
/// Rejects dynamic graphs even when hidden in an unused template or parameter payload.
fn safe_property(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("surface property exceeds structural limits".into());
    }
    match value {
        Value::Array(values) => {
            for value in values {
                safe_property(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key.starts_with('$')
                    || matches!(
                        key.as_str(),
                        "anim_type"
                            | "factory"
                            | "variables"
                            | "anims"
                            | "animations"
                            | "collection_name"
                            | "grid_dimensions"
                            | "property_bag"
                            | "property_bag_for_children"
                    )
                {
                    return Err("surface dynamic expansion is unsupported".into());
                }
                safe_property(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
/// Bounds all authored definitions and gathers their literal inheritance edges.
pub(super) fn authored(
    value: &Value,
    namespace: &str,
    depth: usize,
    count: &mut usize,
    references: &mut Vec<String>,
) -> Result<(), String> {
    *count += 1;
    if depth > 32 || *count > MAX_SURFACE_NODES {
        return Err("surface authored definitions exceed structural limits".into());
    }
    let object = value
        .as_object()
        .ok_or("surface control must be an object")?;
    for (key, value) in object {
        if key == "controls" {
            for child in value
                .as_array()
                .ok_or("surface child lists must remain static")?
            {
                let child = child
                    .as_object()
                    .filter(|child| child.len() == 1)
                    .ok_or("surface child must have one name")?;
                let (key, body) = child.iter().next().expect("one member checked");
                let (_, reference) = local_reference(key, namespace)?;
                if let Some(reference) = reference {
                    references.push(reference);
                }
                authored(body, namespace, depth + 1, count, references)?;
            }
        } else if let Some(name) = key.strip_prefix('$') {
            if !identifier(name.strip_suffix("|default").unwrap_or(name)) {
                return Err("surface parameter name is invalid".into());
            }
            parameter(value)?;
        } else {
            if matches!(
                key.as_str(),
                "factory"
                    | "variables"
                    | "anims"
                    | "animations"
                    | "collection_name"
                    | "grid_dimensions"
                    | "property_bag"
                    | "property_bag_for_children"
            ) {
                return Err("surface dynamic expansion is unsupported".into());
            }
            if matches!(
                key.as_str(),
                "alpha" | "uv" | "color" | "clip_ratio" | "size" | "offset"
            ) && value.as_str().is_some_and(|text| text.starts_with('@'))
            {
                return Err("surface animation references are unsupported".into());
            }
            safe_property(value, 0)?;
        }
    }
    Ok(())
}
/// Rejects missing definitions, cycles and long chains before inheritance flattening.
pub(super) fn acyclic(
    key: &str,
    graph: &BTreeMap<String, Vec<String>>,
    path: &mut Vec<String>,
    done: &mut BTreeSet<String>,
) -> Result<(), String> {
    if done.contains(key) {
        return Ok(());
    }
    if path.iter().any(|entry| entry == key) || path.len() > 32 {
        return Err("surface template cycle or excessive inheritance depth".into());
    }
    let references = graph.get(key).ok_or("surface local template is missing")?;
    path.push(key.to_owned());
    for reference in references {
        acyclic(reference, graph, path, done)?;
    }
    path.pop();
    done.insert(key.to_owned());
    Ok(())
}
/// Caps replicated property payloads before the resolver clones inherited values.
/// Untemplated documents retain the existing source bound; templates have a finite
/// expanded budget without increasing the accepted source bytes or node count.
pub(super) fn bound_expansion(
    root: &str,
    namespace: &str,
    definitions: &BTreeMap<String, (&Value, Option<String>)>,
) -> Result<(), String> {
    let parameter_bytes = definitions
        .values()
        .map(|(body, _)| largest_parameter(body))
        .max()
        .unwrap_or(0)
        .saturating_mul(2);
    definition_bytes(
        root,
        namespace,
        definitions,
        parameter_bytes,
        &mut BTreeMap::new(),
    )?;
    Ok(())
}
const MAX_EXPANDED_PROPERTY_BYTES: usize = super::MAX_SURFACE_BYTES * 16;
/// Finds the largest literal parameter, including parameters on inline instances.
fn largest_parameter(value: &Value) -> usize {
    match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                if key.starts_with('$') {
                    value.to_string().len()
                } else {
                    largest_parameter(value)
                }
            })
            .max()
            .unwrap_or(0),
        Value::Array(values) => values.iter().map(largest_parameter).max().unwrap_or(0),
        _ => 0,
    }
}
/// Counts literal bytes and a conservative bound for every substituted operand.
fn property_bytes(value: &Value, parameter_bytes: usize) -> usize {
    match value {
        Value::String(text) => value.to_string().len().saturating_add(
            text.bytes()
                .filter(|byte| *byte == b'$')
                .count()
                .saturating_mul(parameter_bytes),
        ),
        Value::Array(values) => {
            2 + values
                .iter()
                .map(|value| property_bytes(value, parameter_bytes).saturating_add(1))
                .sum::<usize>()
        }
        Value::Object(object) => {
            2 + object
                .iter()
                .map(|(key, value)| {
                    key.len()
                        .saturating_add(property_bytes(value, parameter_bytes))
                        .saturating_add(4)
                })
                .sum::<usize>()
        }
        _ => value.to_string().len(),
    }
}
/// Rejects replicated payloads as soon as a subtree exceeds the budget.
fn bounded_add(left: usize, right: usize) -> Result<usize, String> {
    let bytes = left.saturating_add(right);
    if bytes > MAX_EXPANDED_PROPERTY_BYTES {
        Err("surface expanded properties exceed byte budget".into())
    } else {
        Ok(bytes)
    }
}
/// Memoizes each immutable template's upper bound before counting repeated instances.
fn definition_bytes(
    name: &str,
    namespace: &str,
    definitions: &BTreeMap<String, (&Value, Option<String>)>,
    parameter_bytes: usize,
    cache: &mut BTreeMap<String, usize>,
) -> Result<usize, String> {
    if let Some(bytes) = cache.get(name) {
        return Ok(*bytes);
    }
    let (body, base) = definitions
        .get(name)
        .ok_or("surface local template is missing")?;
    let inherited = base
        .as_ref()
        .map(|base| definition_bytes(base, namespace, definitions, parameter_bytes, cache))
        .transpose()?
        .unwrap_or(0);
    let bytes = bounded_add(
        inherited,
        control_bytes(body, namespace, definitions, parameter_bytes, cache)?,
    )?;
    cache.insert(name.into(), bytes);
    Ok(bytes)
}
/// Includes inherited children conservatively even when an inline override replaces them.
fn control_bytes(
    value: &Value,
    namespace: &str,
    definitions: &BTreeMap<String, (&Value, Option<String>)>,
    parameter_bytes: usize,
    cache: &mut BTreeMap<String, usize>,
) -> Result<usize, String> {
    let mut bytes = 2;
    for (key, value) in value
        .as_object()
        .ok_or("surface control must be an object")?
    {
        if key == "controls" {
            for child in value
                .as_array()
                .ok_or("surface child lists must remain static")?
            {
                let (key, body) = child
                    .as_object()
                    .and_then(|child| child.iter().next())
                    .ok_or("surface child must be named")?;
                let (_, base) = local_reference(key, namespace)?;
                let inherited = base
                    .map(|base| {
                        definition_bytes(&base, namespace, definitions, parameter_bytes, cache)
                    })
                    .transpose()?
                    .unwrap_or(0);
                bytes = bounded_add(bytes, inherited)?;
                bytes = bounded_add(
                    bytes,
                    control_bytes(body, namespace, definitions, parameter_bytes, cache)?,
                )?;
            }
        } else if !key.starts_with('$') {
            bytes = bounded_add(
                bytes,
                key.len()
                    .saturating_add(property_bytes(value, parameter_bytes))
                    .saturating_add(4),
            )?;
        }
    }
    Ok(bytes)
}

/// Converts the owning resolver output into the exact canonical document rendered natively.
pub(super) fn resolved_value(control: &json_ui::ResolvedControl) -> Result<Value, String> {
    if control.factory.is_some() || control.unresolved_base.is_some() {
        return Err("surface template is not fully static".into());
    }
    let mut object: Map<String, Value> = control
        .properties
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if let Some(kind) = &control.control_type {
        object.insert("type".into(), Value::String(kind.clone()));
    }
    if !control.children.is_empty() {
        let children = control
            .children
            .iter()
            .map(|child| {
                let mut entry = Map::new();
                entry.insert(child.name.clone(), resolved_value(child)?);
                Ok(Value::Object(entry))
            })
            .collect::<Result<Vec<_>, String>>()?;
        object.insert("controls".into(), Value::Array(children));
    }
    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use super::super::Surface;
    use super::*;
    /// Builds a caller-owned local document for structural validation.
    fn surface(document: Value) -> Surface {
        Surface {
            collections: Default::default(),
            previews: Default::default(),
            transitions: Default::default(),
            screen: "local.root".into(),
            document: document.to_string(),
            bindings: Default::default(),
        }
    }
    /// A reusable button with one immutable action parameter.
    fn template() -> Value {
        serde_json::json!({"type":"button","$action|default":"mod.control:0","button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":"$action"}]})
    }
    #[test]
    fn scalar_template_actions_resolve_with_local_overrides() {
        let doc = surface(
            serde_json::json!({"namespace":"local","template":template(),"root":{"type":"panel","controls":[{"button@local.template":{"$action":"mod.control:7"}}]}}),
        );
        assert_eq!(doc.actions().unwrap(), ["mod.control:7"]);
        let expanded: Value = serde_json::from_str(&doc.resolved_document().unwrap()).unwrap();
        assert_eq!(
            expanded["root"]["controls"][0]["button"]["button_mappings"][0]["to_button_id"],
            "mod.control:7"
        );
    }
    #[test]
    fn cross_namespace_cycles_dynamic_children_and_resolved_actions_fail() {
        let mut doc = serde_json::json!({"namespace":"local","template":template(),"root":{"type":"panel","controls":[{"button@foreign.template":{}}]}});
        assert!(surface(doc.clone()).validate().is_err());
        doc["root"]["controls"] = serde_json::json!([{"button@local.template":{}}]);
        doc["template"]["controls"] = serde_json::json!([{"cycle@local.root":{}}]);
        assert!(surface(doc.clone()).validate().is_err());
        doc["template"]["controls"] = Value::String("$dynamic".into());
        assert!(surface(doc.clone()).validate().is_err());
        doc["template"].as_object_mut().unwrap().remove("controls");
        doc["root"]["controls"][0]["button@local.template"]["$action"] =
            Value::String("a".repeat(97));
        assert!(surface(doc).validate().is_err());
    }
    #[test]
    fn expanded_tree_budget_counts_repeated_templates() {
        const LEAVES: usize = 30;
        let leaf = serde_json::json!({"type":"panel","controls":(0..LEAVES).map(|i|serde_json::json!({format!("leaf{i}"):{"type":"panel"}})).collect::<Vec<_>>()});
        let maximum = (MAX_SURFACE_NODES - 1) / (LEAVES + 1);
        for instances in [maximum, maximum + 1] {
            let children = (0..instances)
                .map(|i| serde_json::json!({format!("instance{i}@local.template"): {}}))
                .collect::<Vec<_>>();
            let result = surface(serde_json::json!({"namespace":"local","template":leaf,"root":{"type":"panel","controls":children}})).validate();
            assert_eq!(
                result.is_ok(),
                instances == maximum,
                "{instances} instances: {result:?}"
            );
        }
    }
    #[test]
    fn large_literal_template_replication_is_rejected_before_materialization() {
        let template = serde_json::json!({"type":"label","text":"x".repeat(60*1024)});
        let children = (0..400)
            .map(|index| serde_json::json!({format!("copy{index}@local.template"): {}}))
            .collect::<Vec<_>>();
        let mut document = serde_json::json!({"namespace":"local","template":template,"root":{"type":"panel","controls":children}});
        let amplified = surface(document.clone());
        assert!(amplified.document.len() < super::super::MAX_SURFACE_BYTES);
        assert!(
            amplified
                .validate()
                .unwrap_err()
                .contains("expanded properties")
        );
        document["root"]["controls"] = serde_json::json!([{"once@local.template":{}}]);
        assert!(
            surface(document).validate().is_ok(),
            "one bounded large literal preserves old semantics"
        );
    }
    #[test]
    fn expression_parameters_cannot_double_prior_expanded_strings() {
        let document = surface(
            serde_json::json!({"namespace":"local","root":{"type":"label","$first":"x".repeat(1000),"$second":"($first + $first)","text":"$second"}}),
        );
        assert!(document.validate().is_err());
    }
    #[test]
    fn plain_parameter_cannot_inject_references_into_later_expression_folds() {
        let code = std::iter::repeat_n("$a", 100)
            .collect::<Vec<_>>()
            .join(" + ");
        let doc = surface(
            serde_json::json!({"namespace":"local","template":{"type":"label","$a":"x".repeat(1000),"$code":code,"text":"('(' + $code + ')')"},"root":{"type":"panel","controls":[{"one@local.template":{}}]}}),
        );
        let error = doc.validate().unwrap_err();
        assert!(
            error.contains("parameter"),
            "reject before expression materialization: {error}"
        );
        let aliases = surface(
            serde_json::json!({"namespace":"local","root":{"type":"label","$a":"bounded","$b":"$a","text":"$b"}}),
        );
        assert!(
            aliases.validate().is_ok(),
            "exact scalar aliases remain valid"
        );
    }
}
