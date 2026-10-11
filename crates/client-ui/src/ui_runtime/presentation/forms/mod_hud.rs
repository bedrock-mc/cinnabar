//! Host-owned extension island: guests supply text, never JSON or draw commands.

use std::sync::Arc;

use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use ui::UiNode;

use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
};
use crate::ui_runtime::UiRuntime;
use {
    super::super::{TextMetrics, UiPresentationRuntime},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

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
    screen: CachedScreen,
}

impl UiPresentationRuntime {
    /// Replaces the extension's bounded text, clearing all its UI on revocation.
    pub fn set_mod_label(&mut self, label: Option<&str>) -> Result<(), String> {
        let Some(label) = label.filter(|label| !label.is_empty()) else {
            self.form_presentation.mod_hud = None;
            return Ok(());
        };
        if let Some(hud) = self.form_presentation.mod_hud.as_mut() {
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
            self.form_presentation.mod_hud = Some(ModHud {
                text: label.to_owned(),
                catalog: Arc::new(catalog),
                screen: CachedScreen::default(),
            });
        }
        Ok(())
    }

    /// Draws the extension through JSON-UI only while the gameplay HUD owns focus.
    pub(in super::super) fn append_mod_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if !self.mod_hud_visible(player_runtime, runtime) {
            return;
        }
        let Some(hud) = self.form_presentation.mod_hud.as_mut() else {
            return;
        };
        if runtime.ui_focused(player_runtime)
            || self.loading_stage.is_some()
            || !self.form_presentation.hud.hud.has_visible_content()
        {
            return;
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return;
        };
        let mut data = DataSource::new();
        data.set_global("#mod_text", Scalar::Text(hud.text.clone()));
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &|_| None,
            language: runtime.text_generation(),
        };
        let rollback = (nodes.len(), *next);
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let result = renderer.draw(ScreenArt::default(), inputs, out, |env, root| {
            hud.screen.render_with(
                SCREEN,
                &hud.catalog,
                &Context::default(),
                data,
                (root, px, runtime.text_generation()),
                env,
                &ViewState::default(),
            )
        });
        if let Err(error) = result {
            nodes.truncate(rollback.0);
            *next = rollback.1;
            self.form_presentation.mod_hud = None;
            bevy::log::warn!(%error, "extension HUD disabled after rendering failure");
        }
    }
}

#[cfg(test)]
mod tests;
