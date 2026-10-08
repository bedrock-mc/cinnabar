//! Shared JSON-UI loading and scalar bindings for extension-authored surfaces.

use json_ui::{Catalog, Context, DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue};

/// Loads the validated document in a private catalog and checks its root resolution.
pub(in super::super) fn catalog(surface: &Surface) -> Result<Catalog, String> {
    surface.validate()?;
    let definitions = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &definitions[..]),
        ("ui/extension_surface.json", surface.document.as_bytes()),
    ])
    .map_err(|error| error.to_string())?;
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
        let scalar = match value {
            SurfaceValue::Bool(value) => Scalar::Bool(*value),
            SurfaceValue::Number(value) => Scalar::Num(*value),
            SurfaceValue::Text(value) => Scalar::Text(value.clone()),
            SurfaceValue::Vector(value) => Scalar::Json(json!(value)),
        };
        data.set_global(name, scalar);
    }
}

/// Publishes dimensions after guest values so layouts can depend on the viewport.
pub(in super::super) fn viewport(data: &mut DataSource, size: [f64; 2]) {
    data.set_global("#surface_width", Scalar::Num(size[0]));
    data.set_global("#surface_height", Scalar::Num(size[1]));
}
