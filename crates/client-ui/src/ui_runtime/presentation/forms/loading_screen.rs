//! Joining a world through vanilla's world-loading progress screen for the
//! dimension (dirt, netherrack or end-stone backdrop): "Locating server" until
//! the world starts, then "Generating World" / "Building terrain" until the
//! first view settles.

use std::sync::Arc;

use json_ui::{DataSource, Scalar, ViewState};
use ui::UiNode;

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationError, UiPresentationRuntime,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use crate::ui_runtime::UiRuntime;

/// The world-loading screens by dimension: overworld, nether, the end.
pub const LOADING_SCREENS: [&str; 3] = [
    "progress.overworld_loading_progress_screen",
    "progress.nether_loading_progress_screen",
    "progress.theend_loading_progress_screen",
];
pub const LOADING_SCREEN: &str = LOADING_SCREENS[0];

/// The shipped brand layout stays below server UI files, as the other menus do.
pub(super) fn install_brand_layout(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_loading_brand.json", BRAND_LAYOUT);
}

// Reuse the pack's dialog and image controls. The desktop title in the pinned
// common-art file starts ten percent from the top. The user's loading layout
// lowers that group slightly while keeping the logo and dialog near the top.
const BRAND_LAYOUT: &str = r##"{
  "namespace": "progress",
  "cinnabar_loading_group": {
    "type": "stack_panel", "orientation": "vertical", "use_child_anchors": true,
    "size": ["100%", "100%c"],
    "anchor_from": "top_middle", "anchor_to": "top_middle",
    "offset": [0, "15%"],
    "controls": [
      {"cinnabar_loading_title@common_art.title_image": {
        "size": ["55%", 64], "keep_ratio": true,
        "anchor_from": "top_middle", "anchor_to": "top_middle"
      }},
      {"cinnabar_loading_title_padding": {"type": "panel", "size": [0, 16]}},
      {"world_modal_progress_panel@progress.world_modal_progress_panel": {
        "$modal_button_panel_type": "progress.modal_button_panel"
      }}
    ]
  },
  "world_convert_modal_progress_screen_content": {
    "modifications": [
      {"array_name": "controls", "operation": "replace", "control_name": "title_panel_content",
       "value": {"cinnabar_loading_group@progress.cinnabar_loading_group": {}}},
      {"array_name": "controls", "operation": "remove", "control_name": "world_modal_progress_panel"}
    ]
  }
}"##;

/// Which part of joining the loading screen reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadingStage {
    Connecting,
    BuildingTerrain,
}

impl UiPresentationRuntime {
    /// Draw the world-loading screen for `stage`; `Ok(false)` when nothing drew.
    pub(in super::super) fn append_loading_screen(
        &mut self,
        runtime: &UiRuntime,
        stage: LoadingStage,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(false);
        };
        let reference = match self.hud_frame.dimension {
            1 => LOADING_SCREENS[1],
            2 => LOADING_SCREENS[2],
            _ => LOADING_SCREEN,
        };
        let translate = |key: &str| runtime.translation(key);
        let text = |key: &str, fallback: &str| {
            Scalar::Text(
                translate(key).map_or_else(|| fallback.to_owned(), |text| text.to_string()),
            )
        };
        let mut data = DataSource::new();
        data.set_strict(true);
        // Vanilla's world generation handler titles the terrain wait.
        data.set_global(
            "#title_text",
            match stage {
                LoadingStage::Connecting => text(
                    "progressScreen.title.connectingExternal",
                    "Connecting to external server",
                ),
                LoadingStage::BuildingTerrain => {
                    text("progressScreen.generating", "Generating World")
                }
            },
        );
        data.set_global(
            "#progress_text",
            match stage {
                LoadingStage::Connecting => {
                    text("progressScreen.message.locating", "Locating server")
                }
                LoadingStage::BuildingTerrain => {
                    text("progressScreen.message.building", "Building terrain")
                }
            },
        );
        data.set_global("#bar_animation_visible", Scalar::Bool(true));
        let context = renderer.context().clone();
        let catalog = Arc::clone(renderer.catalog());
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let screen = &mut self.form_presentation.hud.loading;
        let view = ViewState::default();
        let frame = renderer.draw(
            ScreenArt {
                now: self.menu_seconds,
                clocks: Some(&self.scene_clock),
                images: Some(&self.menu_artwork.refs),
                ..ScreenArt::default()
            },
            inputs,
            out,
            |env, root| {
                screen.render_with(
                    reference,
                    &catalog,
                    &context,
                    data,
                    (root, px, runtime.text_generation()),
                    env,
                    &view,
                )
            },
        )?;
        Ok(frame.is_some())
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The loading screen's last laid-out draw nodes, in virtual px.
    pub fn loading_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.form_presentation.hud.loading.nodes()
    }
}
