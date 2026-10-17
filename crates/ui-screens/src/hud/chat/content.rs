//! Retains the named definitions selected by message content in its resolution scope.

use super::*;
use json_ui::Context;

/// Saves content dependencies before placement suppression, following resolved variables,
/// inline inheritance and deferred factories/grids with their captured scopes.
pub(super) fn dependencies<'a>(
    catalog: &Catalog,
    content: impl Iterator<Item = &'a RawControl>,
) -> Vec<RawControl> {
    let roots = content
        .map(|control| ControlRef::new(&control.owner_ns, &control.name))
        .collect::<BTreeSet<_>>();
    let context = crate::menu_screens::retail_context();
    let mut pending = roots
        .iter()
        .map(|root| (root.clone(), context.clone()))
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut saved = BTreeMap::new();
    let mut expansions = 0;
    while let Some((reference, scope)) = pending.pop() {
        let key = (
            reference.clone(),
            serde_json::to_string(scope.vars()).unwrap_or_default(),
        );
        if !seen.insert(key) {
            continue;
        }
        if expansions == MAX_FACTORY_EXPANSIONS {
            tracing::warn!(
                limit = MAX_FACTORY_EXPANSIONS,
                "chat dependency scan skipped deferred templates beyond the expansion limit"
            );
            break;
        }
        expansions += 1;
        save(catalog, &reference, &roots, &mut saved);
        if let Some(control) = json_ui::resolve(catalog, &reference.to_string(), &scope).control {
            visit(catalog, &control, &scope, &roots, &mut saved, &mut pending);
        }
    }
    saved.into_values().collect()
}

/// Saves one definition and its literal inheritance chain, excluding placement owners.
fn save(
    catalog: &Catalog,
    reference: &ControlRef,
    roots: &BTreeSet<ControlRef>,
    saved: &mut BTreeMap<ControlRef, RawControl>,
) {
    let mut reference = reference.clone();
    loop {
        if roots.contains(&reference)
            || saved.contains_key(&reference)
            || (reference.namespace == "hud"
                && ROOTS.contains(&reference.name.as_str())
                && reference.name != "chat_label")
        {
            return;
        }
        let Some(control) = catalog.lookup(&reference.namespace, &reference.name) else {
            return;
        };
        saved.insert(reference.clone(), control.clone());
        let Some(base) = control
            .base
            .as_deref()
            .filter(|base| !base.starts_with('$'))
        else {
            return;
        };
        reference = ControlRef::parse(base, &control.owner_ns);
    }
}

/// Records resolved inline bases and queues deferred templates under their runtime scopes.
fn visit(
    catalog: &Catalog,
    control: &ResolvedControl,
    scope: &Context,
    roots: &BTreeSet<ControlRef>,
    saved: &mut BTreeMap<ControlRef, RawControl>,
    pending: &mut Vec<(ControlRef, Context)>,
) {
    if let Some(base) = &control.base {
        save(catalog, base, roots, saved);
    }
    let enclosing = control
        .properties
        .get("factory_scope")
        .and_then(Value::as_object)
        .map_or_else(|| scope.clone(), |vars| with_vars(scope.clone(), vars));
    if control.control_type.as_deref() == Some("grid")
        && let Some(template) = control
            .properties
            .get("grid_item_template")
            .and_then(Value::as_str)
    {
        let namespace = control
            .base
            .as_ref()
            .map_or("", |base| base.namespace.as_str());
        pending.push((ControlRef::parse(template, namespace), enclosing.clone()));
    }
    if let Some(factory) = &control.factory {
        if let Some(template) = &factory.control_name {
            pending.push((template.clone(), with_vars(enclosing, &factory.variables)));
        } else {
            for template in factory.control_ids.values() {
                pending.push((
                    template.clone(),
                    with_vars(scope.clone(), &factory.variables),
                ));
            }
        }
    }
    for child in &control.children {
        visit(catalog, child, scope, roots, saved, pending);
    }
}
