//! Host-owned extension island: guests supply text, never JSON or draw commands.

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use ui::FONT_DESIGN_PIXEL_TEXELS;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{FormEngine, ScreenArt, screen::PaintFrame},
};

use crate::ModUi;

const SCREEN: &str = "cinnabar_mod.label";
// Vanilla hud_screen.json:3355 uses this corner anchor and inset for its label.
const TEMPLATE: &[u8] = br##"{
    "namespace": "cinnabar_mod",
    "label": {
        "type": "panel",
        "size": ["100%", "100%"],
        "controls": [{"text": {
            "type": "label", "size": ["default", "default"],
            "anchor_from": "top_right", "anchor_to": "top_right",
            "offset": [-4, 4], "color": [1, 1, 1], "shadow": true,
            "text_alignment": "right", "text": "#mod_text",
            "bindings": [{"binding_name": "#mod_text"}]
        }}]
    }
}"##;

pub(super) struct ModHud {
    text: String,
    catalog: Arc<Catalog>,
    pub(super) screen: CachedScreen,
}

impl ModUi {
    /// Replaces the extension's bounded text, clearing all its UI on revocation.
    pub fn set_mod_label(&mut self, label: Option<&str>) -> Result<(), String> {
        let Some(label) = label.filter(|label| !label.is_empty()) else {
            self.mod_hud = None;
            return Ok(());
        };
        if let Some(hud) = self.mod_hud.as_mut() {
            if hud.text != label {
                hud.text = label.to_owned();
            }
        } else {
            let catalog = Catalog::from_files([
                ("ui/_global_variables.json", &b"{}"[..]),
                (
                    "ui/_ui_defs.json",
                    &br#"{"ui_defs":["ui/cinnabar_mod.json"]}"#[..],
                ),
                ("ui/cinnabar_mod.json", TEMPLATE),
            ])
            .map_err(|error| error.to_string())?;
            self.mod_hud = Some(ModHud {
                text: label.to_owned(),
                catalog: Arc::new(catalog),
                screen: CachedScreen::default(),
            });
        }
        Ok(())
    }

    /// Draws the extension's label; the caller shows it only while the gameplay HUD owns focus.
    pub fn paint_mod_hud(&mut self, renderer: &FormEngine, paint_frame: &mut PaintFrame<'_>) {
        let Some(hud) = self.mod_hud.as_mut() else {
            return;
        };
        let mut data = DataSource::new();
        data.set_global("#mod_text", Scalar::Text(hud.text.clone()));
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let result = renderer.draw(ScreenArt::default(), paint_frame, |env, root| {
            hud.screen.render_with(
                SCREEN,
                &hud.catalog,
                &Context::default(),
                data,
                (root, px, language),
                env,
                &ViewState::default(),
            )
        });
        if let Err(error) = result {
            paint_frame.nodes.truncate(rollback.0);
            *paint_frame.next = rollback.1;
            self.mod_hud = None;
            tracing::warn!(%error, "extension HUD disabled after rendering failure");
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    /// Bind and layout passes the installed label's screen ran.
    pub fn mod_label_passes(&self) -> Option<usize> {
        Some(self.mod_hud.as_ref()?.screen.passes)
    }
}
