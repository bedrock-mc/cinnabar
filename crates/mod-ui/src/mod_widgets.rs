//! Private, retained JSON-UI cards supplied as bounded cosmetic data.

mod cells;
pub(super) mod template;
mod world_labels;

use crate::{ModUi, RowIcons};
use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use std::sync::Arc;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, IconRef,
    mod_hud::{Crosshair, Hud},
};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        item_renderer::ItemRenderer,
        screen::{CustomRenderer, PaintFrame},
    },
};

pub(super) struct ModWidgets {
    content: Hud,
    pub(super) catalog: Arc<Catalog>,
    screen: CachedScreen,
    data: Arc<DataSource>,
    icons: Vec<IconRef>,
    resolved: Vec<Option<IconRef>>,
    pub(super) viewport: [f64; 2],
    projection: Option<[f32; 16]>,
}
impl ModUi {
    /// Native food presentation preferences of the published component.
    pub fn mod_hunger_overlay(&self) -> Option<&ui::mod_hud::HungerOverlay> {
        self.mod_widgets.as_ref()?.content.hunger_overlay.as_ref()
    }
    /// Reads the validated cosmetic actor style of the currently published component.
    pub fn mod_actor_style(&self) -> Option<&ui::mod_hud::ActorStyle> {
        self.mod_widgets.as_ref()?.content.actor_style.as_ref()
    }

    /// Replaces card values while retaining the template when its geometry is unchanged. Chat
    /// state bound to the previous [`Self::mod_chat_actions`] is the caller's to revoke.
    pub fn set_mod_hud(&mut self, content: Option<&Hud>) -> Result<(), String> {
        let Some(content) = content.filter(|hud| {
            hud.hunger_overlay.is_some()
                || hud.actor_style.is_some()
                || !hud.cards.is_empty()
                || hud.hide_effect_icons
                || hud.hide_scoreboard
                || !hud.world_labels.is_empty()
                || !hud.chat_actions.is_empty()
        }) else {
            self.mod_widgets = None;
            return Ok(());
        };
        content.validate()?;
        if let Some(widgets) = &mut self.mod_widgets {
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
            self.mod_widgets = Some(ModWidgets {
                content: content.clone(),
                catalog: Arc::new(template::catalog(content)?),
                screen: CachedScreen::default(),
                data: Arc::new(template::data(content, [0.; 2])),
                icons: Vec::new(),
                resolved: Vec::new(),
                viewport: [0.; 2],
                projection: None,
            });
        }
        Ok(())
    }

    /// Current literal chat spans, revoked with the owning cosmetic HUD output.
    pub fn mod_chat_actions(&self) -> &[ui::mod_hud::ChatAction] {
        self.mod_widgets
            .as_ref()
            .map_or(&[], |w| &w.content.chat_actions)
    }

    /// Supplies the current actual camera projection; absent cameras hide every world anchor.
    pub fn set_mod_world_projection(&mut self, projection: Option<[f32; 16]>) {
        if let Some(widgets) = &mut self.mod_widgets {
            widgets.projection = projection.filter(|m| m.iter().all(|v| v.is_finite()));
        }
    }
    /// A sidebar replacement immediately restores vanilla when revoked.
    pub fn mod_scoreboard_hidden(&self) -> bool {
        self.mod_widgets
            .as_ref()
            .is_some_and(|w| w.content.hide_scoreboard)
    }

    /// A published replacement may hide ordinary effect icons even with no card rows.
    pub fn mod_effect_icons_hidden(&self) -> bool {
        self.mod_widgets
            .as_ref()
            .is_some_and(|widgets| widgets.content.hide_effect_icons)
    }

    /// Retains a validated cosmetic replacement; `None` immediately restores the ordinary cursor.
    pub fn set_mod_crosshair(&mut self, crosshair: Option<&Crosshair>) -> Result<(), String> {
        if let Some(crosshair) = crosshair {
            crosshair.validate()?;
        }
        self.mod_crosshair = crosshair.cloned();
        Ok(())
    }

    /// The validated cosmetic cursor, drawn only while personal HUD content shows.
    pub fn mod_crosshair(&self) -> Option<&Crosshair> {
        self.mod_crosshair.as_ref()
    }

    /// Each card row's item icon from `icon`; `None` without published cards.
    pub fn mod_widget_icons(
        &self,
        icon: impl Fn(&str, u32) -> Option<IconRef>,
    ) -> Option<RowIcons> {
        let widgets = self.mod_widgets.as_ref()?;
        Some(RowIcons::resolve(&widgets.content, icon))
    }

    /// Draws the cards; the caller shows them only while personal HUD content follows gameplay
    /// focus. `resolved` holds [`Self::mod_widget_icons`].
    pub fn paint_mod_widgets(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        resolved: &RowIcons,
    ) {
        let resolved = resolved.as_slice();
        let Some(widgets) = self.mod_widgets.as_mut() else {
            return;
        };
        let px = paint_frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let content = paint_frame.content;
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
        if !widgets.content.world_labels.is_empty() {
            world_labels::data(
                Arc::make_mut(&mut widgets.data),
                &widgets.content,
                viewport,
                widgets.projection,
            );
        }
        let Some(renderer) = engine else {
            return;
        };
        let rollback = (paint_frame.nodes.len(), *paint_frame.next);
        let language = paint_frame.language;
        let mut items = ItemRenderer {
            icons: &widgets.icons,
            id_aux: &[],
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut items];
        let result = renderer.draw(
            ScreenArt::default(),
            &mut paint_frame.with_renderers(&mut renderers),
            |env, root| {
                widgets.screen.render_shared_with(
                    "cinnabar_personal_hud.cards",
                    &widgets.catalog,
                    &Context::default(),
                    Arc::clone(&widgets.data),
                    (root, px, language),
                    env,
                    &ViewState::default(),
                )
            },
        );
        if let Err(error) = result {
            paint_frame.nodes.truncate(rollback.0);
            *paint_frame.next = rollback.1;
            self.mod_widgets = None;
            tracing::warn!(%error, "personal HUD cards disabled after rendering failure");
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModUi {
    pub fn mod_widgets_state(&self) -> Option<crate::test_support::ModWidgetsState<'_>> {
        let widgets = self.mod_widgets.as_ref()?;
        Some(crate::test_support::ModWidgetsState {
            catalog: &widgets.catalog,
            content: &widgets.content,
            viewport: widgets.viewport,
        })
    }
}
