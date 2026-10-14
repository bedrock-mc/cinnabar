//! Shared off-frame preparation of a component and its owner-scoped presentation resources.

use assets::RuntimeFontCatalog;
use client_extensions::modding::font;
use mod_host::{ModGrants, ModHost};
use std::{path::Path, sync::Arc};

pub(super) struct PreparedMod {
    pub(super) host: ModHost,
    pub(super) font: Option<Arc<RuntimeFontCatalog>>,
    pub(super) panel_ui: Option<Arc<mod_ui::PreparedPanelUi>>,
    pub(super) branding:
        Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
}

/// Prepares one immutable source snapshot; explicit sidecars override optional embedded data.
/// Settings activation and publication follow only after every resource has been accepted.
pub(super) fn prepare(
    path: &Path,
    component: &[u8],
    grants: ModGrants,
    font_bytes: Option<&[u8]>,
    ui: Option<&[u8]>,
    settings: Option<(&Path, &str)>,
) -> Result<PreparedMod, String> {
    let controls = grants.controls;
    let mut host = ModHost::prepare_snapshot_with_grants(path, component, grants, settings)
        .map_err(|error| format!("{error:#}"))?;
    let font = if controls {
        font_bytes
            .or_else(|| host.panel_font_source())
            .map(font::rasterize)
            .transpose()?
            .map(Arc::new)
    } else {
        None
    };
    if controls && let Some(ui) = ui {
        let json = std::str::from_utf8(ui).map_err(|error| error.to_string())?;
        host.set_panel_bundle(json)
            .map_err(|error| format!("{error:#}"))?;
    }
    let panel_ui = host
        .panel_bundle()
        .map(mod_ui::PreparedPanelUi::prepare)
        .transpose()?;
    let branding = prepare_branding(&host)?;
    Ok(PreparedMod {
        host,
        font,
        panel_ui,
        branding,
    })
}

/// Decodes generic static branding only when the host's active environment grant permits it.
pub(super) fn prepare_branding(
    host: &ModHost,
) -> Result<
    Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
    String,
> {
    if host.menu_logo_source().is_none() && host.menu_title().is_none() {
        return Ok(None);
    }
    client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding::prepare(
        host.menu_logo_source(),
        host.menu_title(),
        [mod_api::MAX_MENU_LOGO_WIDTH, mod_api::MAX_MENU_LOGO_HEIGHT],
        mod_api::MAX_MENU_LOGO_PIXELS,
    )
    .map(|branding| Some(Arc::new(branding)))
}
