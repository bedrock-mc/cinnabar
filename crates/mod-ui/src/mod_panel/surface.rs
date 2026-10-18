//! Shared JSON-UI loading and scalar bindings for extension-authored surfaces.

use json_ui::{Catalog, Context, DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Panel, Surface, SurfaceValue};

/// Loads the validated document in a private catalog and checks its root resolution.
pub(in super::super) fn catalog(surface: &Surface) -> Result<Catalog, String> {
    surface.validate()?;
    let document = surface.resolved_document()?;
    let definitions = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &definitions[..]),
        ("ui/extension_surface.json", document.as_bytes()),
    ])
    .map_err(|error| error.to_string())?
    .with_paint_transforms();
    if !catalog.diagnostics().is_empty() {
        return Err(format!(
            "invalid surface catalog: {}",
            catalog.diagnostics().join("; ")
        ));
    }
    let resolution = json_ui::resolve(&catalog, &surface.screen, &Context::default());
    if resolution.control.is_none() || !resolution.diagnostics.is_empty() {
        return Err(format!(
            "invalid surface root: {}",
            resolution.diagnostics.join("; ")
        ));
    }
    Ok(catalog)
}

/// Adds changing extension values without rebuilding the retained catalog.
pub(in super::super) fn bind(surface: &Surface, data: &mut DataSource) {
    for (name, value) in &surface.bindings {
        data.set_global(name, scalar(value));
    }
}

/// Publishes dimensions after guest values so layouts can depend on the viewport.
pub(in super::super) fn viewport(data: &mut DataSource, size: [f64; 2]) {
    data.set_global("#surface_width", Scalar::Num(size[0]));
    data.set_global("#surface_height", Scalar::Num(size[1]));
}

/// Reuses native control formatting in immutable tile metadata, only when panel values change.
pub(super) fn collections(panel: &Panel, data: &mut DataSource) {
    let Some(surface) = &panel.surface else {
        return;
    };
    for (name, collection) in &surface.collections {
        let items = collection
            .order
            .iter()
            .map(|index| {
                let item = &collection.definition.items[*index];
                let mut entry = json_ui::CollectionItem::default();
                entry
                    .values
                    .insert("#item_exists".into(), Scalar::Bool(true));
                for (key, value) in &item.values {
                    entry.values.insert(key.clone(), scalar(value));
                }
                for (role, control) in &item.controls {
                    for field in [
                        "value",
                        "toggle_enabled",
                        "toggle_disabled",
                        "toggle_progress",
                        "toggle_inverse_progress",
                        "hover",
                    ] {
                        entry = entry.with_global_binding(
                            format!("#item_{role}_{field}"),
                            format!("#row_{control}_{field}"),
                        );
                    }
                }
                if let Some(art) = &item.art {
                    for (key, value) in [
                        ("paths", json!(art.paths)),
                        ("view_box", json!(art.view_box)),
                        ("stroke_width", json!(art.stroke_width)),
                        ("closed", json!(art.closed)),
                    ] {
                        entry
                            .values
                            .insert(format!("#item_art_{key}"), Scalar::Json(value));
                    }
                }
                entry
            })
            .collect();
        data.set_collection(name, items);
        data.set_collection_defaults(name, [("#item_exists".into(), Scalar::Bool(false))].into());
        data.set_global(
            format!("#collection_{name}_dimensions"),
            Scalar::Json(json!([
                collection.definition.columns,
                collection
                    .order
                    .len()
                    .div_ceil(collection.definition.columns)
            ])),
        );
    }
}

/// Converts an admitted scalar through the same representation in globals and tile fields.
fn scalar(value: &SurfaceValue) -> Scalar {
    match value {
        SurfaceValue::Bool(value) => Scalar::Bool(*value),
        SurfaceValue::Number(value) => Scalar::Num(*value),
        SurfaceValue::Text(value) => Scalar::Text(value.clone()),
        SurfaceValue::Vector(value) => Scalar::Json(json!(value)),
    }
}

/// Captures item actions against the exact catalogue order used to paint these hit regions.
pub(super) fn resolve_collection_hits(surface: &Surface, hits: &mut [json_ui::HitRegion]) {
    for hit in hits {
        let Some(role) = hit
            .pressed
            .as_deref()
            .and_then(|s| s.strip_prefix("mod.item:"))
        else {
            continue;
        };
        hit.pressed = hit
            .collection
            .as_ref()
            .and_then(|name| surface.collections.get(name))
            .zip(hit.collection_index)
            .and_then(|(collection, index)| collection.control(index, role))
            .map(|index| format!("mod.control:{index}"));
    }
}
