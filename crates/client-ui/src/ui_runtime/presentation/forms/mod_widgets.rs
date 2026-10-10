//! Private, retained JSON-UI cards supplied as bounded cosmetic data.

pub(super) mod template;
#[cfg(test)]
pub(super) mod tests;

use super::super::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationRuntime};
use super::{
    engine::{EngineInputs, EngineOutput, ScreenArt},
    hud::CachedScreen,
};
use crate::ui_runtime::UiRuntime;
use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use std::sync::Arc;
use ui::{
    IconRef, UiNode,
    mod_hud::{Crosshair, Hud, MAX_HUD_ROWS},
};

pub(super) struct ModWidgets {
    content: Hud,
    pub(super) catalog: Arc<Catalog>,
    screen: CachedScreen,
    data: Arc<DataSource>,
    icons: Vec<IconRef>,
    resolved: Vec<Option<IconRef>>,
    pub(super) viewport: [f64; 2],
}
impl UiPresentationRuntime {
    /// Replaces card values while retaining the template when its geometry is unchanged.
    pub fn set_mod_hud(&mut self, content: Option<&Hud>) -> Result<(), String> {
        let Some(content) = content.filter(|hud| !hud.cards.is_empty() || hud.hide_effect_icons)
        else {
            self.form_presentation.mod_widgets = None;
            return Ok(());
        };
        content.validate()?;
        if let Some(widgets) = &mut self.form_presentation.mod_widgets {
            if widgets.content == *content {
                return Ok(());
            }
            if !template::same_shape(&widgets.content, content) {
                widgets.catalog = Arc::new(template::catalog(content)?);
                widgets.screen = CachedScreen::default();
            }
            widgets.content = content.clone();
            widgets.data = Arc::new(template::data(content, widgets.viewport));
            widgets.resolved.clear();
        } else {
            self.form_presentation.mod_widgets = Some(ModWidgets {
                content: content.clone(),
                catalog: Arc::new(template::catalog(content)?),
                screen: CachedScreen::default(),
                data: Arc::new(template::data(content, [0.; 2])),
                icons: Vec::new(),
                resolved: Vec::new(),
                viewport: [0.; 2],
            });
        }
        Ok(())
    }

    /// A published replacement may hide ordinary effect icons even with no card rows.
    pub(super) fn mod_effect_icons_hidden(&self) -> bool {
        self.form_presentation
            .mod_widgets
            .as_ref()
            .is_some_and(|widgets| widgets.content.hide_effect_icons)
    }

    /// Retains a validated cosmetic replacement; `None` immediately restores the ordinary cursor.
    pub fn set_mod_crosshair(&mut self, crosshair: Option<&Crosshair>) -> Result<(), String> {
        if let Some(crosshair) = crosshair {
            crosshair.validate()?;
        }
        self.form_presentation.mod_crosshair = crosshair.cloned();
        Ok(())
    }

    /// All personal HUD content follows gameplay focus and the player's HUD preference.
    pub(in super::super) fn mod_hud_visible(
        &self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> bool {
        !runtime.ui_focused(player)
            && self.menu_view.is_none()
            && self.loading_stage.is_none()
            && self
                .form_presentation
                .chat
                .settings
                .options
                .value("hide_hud")
                == 0
            && !self.mod_panel_open()
    }

    pub(in super::super) fn append_mod_widgets(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if !self.mod_hud_visible(player, runtime)
            || !self.form_presentation.hud.hud.has_visible_content()
        {
            return;
        }
        let Some(widgets) = &self.form_presentation.mod_widgets else {
            return;
        };
        let mut resolved = [None; MAX_HUD_ROWS];
        let mut count = 0;
        for row in widgets.content.cards.iter().flat_map(|card| &card.rows) {
            resolved[count] = row
                .item
                .as_deref()
                .and_then(|id| self.item_icon(id, row.metadata));
            count += 1;
        }
        let resolved = &resolved[..count];
        let widgets = self
            .form_presentation
            .mod_widgets
            .as_mut()
            .expect("checked above");
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = [f64::from(content[0] / px), f64::from(content[1] / px)];
        if widgets.viewport != viewport {
            widgets.viewport = viewport;
            widgets.data = Arc::new(template::data(&widgets.content, viewport));
            widgets.resolved.clear();
        }
        if widgets.resolved.as_slice() != resolved {
            widgets.icons.clear();
            let data = Arc::make_mut(&mut widgets.data);
            for (index, icon) in resolved.iter().enumerate() {
                let scalar = match icon {
                    Some(icon) => {
                        let id = widgets.icons.len();
                        widgets.icons.push(*icon);
                        Scalar::Num(id as f64)
                    }
                    None => Scalar::Json(serde_json::Value::Null),
                };
                data.set_global(format!("#row_{index}_icon"), scalar);
            }
            widgets.resolved.clear();
            widgets.resolved.extend_from_slice(resolved);
        }
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return;
        };
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
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &[],
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let art = ScreenArt {
            icons: &widgets.icons,
            ..Default::default()
        };
        let result = renderer.draw(art, inputs, out, |env, root| {
            widgets.screen.render_shared_with(
                "cinnabar_personal_hud.cards",
                &widgets.catalog,
                &Context::default(),
                Arc::clone(&widgets.data),
                (root, px, runtime.text_generation()),
                env,
                &ViewState::default(),
            )
        });
        if let Err(error) = result {
            nodes.truncate(rollback.0);
            *next = rollback.1;
            self.form_presentation.mod_widgets = None;
            bevy::log::warn!(%error, "personal HUD cards disabled after rendering failure");
        }
    }
}
