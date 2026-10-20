//! Conservative comparison of resolved screens and their deferred grid templates.

use crate::{Catalog, CatalogLibrary, Context, ControlLibrary, ResolvedControl};

#[cfg(test)]
mod tests;

/// Proves that a screen and its grid cells resolve identically under two catalogs.
/// Data-fed factories are deliberately excluded because their variables are not known here.
pub fn screen_templates_unchanged(
    current: &ResolvedControl,
    reference: &str,
    catalog: &Catalog,
    baseline: &Catalog,
    context: &Context,
    data: &crate::DataSource,
) -> bool {
    if data.has_factory_inputs() {
        return false;
    }
    let Some(original) = crate::resolve_screen(reference, baseline, context) else {
        return false;
    };
    let current_library = CatalogLibrary { catalog, context };
    let original_library = CatalogLibrary {
        catalog: baseline,
        context,
    };
    let mut budget = crate::resolve::MAX_NODES;
    compare(
        current,
        &original,
        &current_library,
        &original_library,
        &mut budget,
        0,
    )
}

fn compare(
    current: &ResolvedControl,
    original: &ResolvedControl,
    library: &CatalogLibrary<'_>,
    baseline: &CatalogLibrary<'_>,
    budget: &mut usize,
    depth: usize,
) -> bool {
    if *budget == 0 || depth >= crate::resolve::MAX_DEPTH || current != original {
        return false;
    }
    *budget -= 1;
    if current.factory.is_some() && current.control_type.as_deref() == Some("slider") {
        return false;
    }
    let mut templates = Vec::new();
    if let Some(reference) = crate::bind::grid_template(current) {
        templates.push((reference, std::collections::BTreeMap::new()));
    }
    if crate::bind::feed::has_collection_factory(current) {
        let factory = current.factory.as_ref().unwrap();
        if let Some(reference) = &factory.control_name {
            templates.push((reference.clone(), std::collections::BTreeMap::new()));
        } else {
            for reference in factory.control_ids.values() {
                templates.push((
                    reference.clone(),
                    factory.creation_vars(&Default::default()),
                ));
            }
        }
    }
    for (reference, extra) in templates {
        let vars = || {
            let mut scope = crate::bind::factory_scope(current);
            scope.extend(extra.clone());
            scope
        };
        let (Some(cell), Some(original_cell)) = (
            library.resolve_with(&reference, "", &vars),
            baseline.resolve_with(&reference, "", &vars),
        ) else {
            return false;
        };
        if !compare(&cell, &original_cell, library, baseline, budget, depth + 1) {
            return false;
        }
    }
    current
        .children
        .iter()
        .zip(&original.children)
        .all(|(child, original)| compare(child, original, library, baseline, budget, depth + 1))
}
