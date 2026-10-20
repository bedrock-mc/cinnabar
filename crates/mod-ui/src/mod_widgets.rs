//! Private, retained JSON-UI cards supplied as bounded cosmetic data.

mod cards;
mod cells;
mod compass;
pub(super) mod sources;
pub(super) mod template;
pub(crate) mod text_plate;
mod world_labels;

use crate::{ModUi, RowIcons};
use json_ui::{Catalog, DataSource};
use std::sync::Arc;
use ui::{
    IconRef,
    mod_hud::{Crosshair, Hud},
};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{FormEngine, screen::PaintFrame},
};

pub(super) struct ModWidgets {
    published: Arc<Hud>,
    pub(crate) content: Hud,
    pub(super) catalog: Arc<Catalog>,
    screen: CachedScreen,
    data: Arc<DataSource>,
    data_revision: u64,
    pub(super) viewport: [f64; 2],
    projection: Option<[f32; 16]>,
    pub(crate) cards: Vec<cards::CardPaint>,
    card_indices: std::collections::BTreeMap<String, usize>,
}
impl ModWidgets {
    /// Builds one catalog and bounded card cache for a newly published guest HUD.
    fn new(content: Arc<Hud>) -> Result<Self, String> {
        Ok(Self {
            catalog: Arc::new(template::catalog(&content)?),
            data: Arc::new(template::data(&content, [0.; 2])),
            data_revision: 0,
            cards: cards::entries(&content),
            card_indices: content
                .cards
                .iter()
                .enumerate()
                .map(|(index, card)| (card.id.clone(), index))
                .collect(),
            content: content.as_ref().clone(),
            published: content,
            screen: CachedScreen::default(),
            viewport: [0.; 2],
            projection: None,
        })
    }

    /// Looks up a local identifier in the current admitted card set.
    pub(crate) fn card_index(&self, id: &str) -> Option<usize> {
        self.card_indices.get(id).copied()
    }

    /// Updates live values while preserving geometry and prepared card output when possible.
    fn publish(&mut self, content: Arc<Hud>) -> Result<(), String> {
        if self.published == content {
            self.published = content;
            return Ok(());
        }
        if !template::same_shape(&self.content, &content) {
            self.catalog = Arc::new(template::catalog(&content)?);
            self.screen = CachedScreen::default();
            self.cards = cards::entries(&content);
            self.card_indices = content
                .cards
                .iter()
                .enumerate()
                .map(|(index, card)| (card.id.clone(), index))
                .collect();
        }
        self.published = content.clone();
        self.content = content.as_ref().clone();
        self.data = Arc::new(template::data(&content, self.viewport));
        self.data_revision += 1;
        for card in &mut self.cards {
            card.invalidate_icons();
        }
        Ok(())
    }
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
        if self
            .mod_widgets
            .as_ref()
            .is_some_and(|w| Some(w.published.as_ref()) == content)
        {
            return Ok(());
        }
        self.set_shared_mod_hud(content.cloned().map(Arc::new))
    }

    /// Retains immutable published output; identical shared identity needs no validation or scan.
    pub fn set_shared_mod_hud(&mut self, content: Option<Arc<Hud>>) -> Result<(), String> {
        if self
            .mod_widgets
            .as_ref()
            .zip(content.as_ref())
            .is_some_and(|(w, c)| Arc::ptr_eq(&w.published, c))
        {
            return Ok(());
        }
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
            widgets.publish(content)?;
        } else {
            self.mod_widgets = Some(ModWidgets::new(content)?);
        }
        self.refresh_hud_card_sources();
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
        self.paint_hud_cards(engine, paint_frame, resolved);
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
