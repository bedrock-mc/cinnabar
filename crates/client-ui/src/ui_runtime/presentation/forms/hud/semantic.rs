//! Retains bounded HUD components before allocating their text and collection models.

use std::sync::Arc;

use json_ui::{Catalog, HudChanges, HudModel, HudSlot, HudTitle, Timed};
use launcher::menu::settings_options::{SettingId, SettingsOptions};
use ui::{BossBarView, IconRef, TimedText};
use ui_screens::text::bounded_text;

use super::super::super::{HudFrame, bounded_visible_text, resolve_chat_line_formatted};
use super::{ITEM_NAME_MILLIS, MAX_BOSS_BARS, MAX_CHAT_LINES};
use crate::ui_runtime::{Translator, UiRuntime};

mod components;
use components::{boss_model, sidebar_model, world_text_input, world_text_lines};

/// The current HUD model and the exact icon table its item references index.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct SemanticHud {
    pub(super) model: HudModel,
    pub(in crate::ui_runtime::presentation) icons: Vec<IconRef>,
    session: Option<u64>,
    catalog: Option<Arc<Catalog>>,
    language: [usize; 3],
    // Keep the language tables alive so their addresses cannot be reused under this key.
    _translator: Translator,
    chat_rows: Vec<u64>,
    chat_options: Option<(f64, char, char, bool)>,
    sidebar_key: Option<(u64, u64, bool)>,
    bosses: Vec<BossBarView>,
    world_text: components::WorldTextInput,
    #[cfg(test)]
    pub(in crate::ui_runtime::presentation) constructions: [usize; json_ui::HudChanges::COUNT],
}

impl SemanticHud {
    /// Updates only changed semantic components; paint and preview state stay with the caller.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn refresh(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        sidebar: Option<&super::super::super::retained_hud::PresentedScoreboard>,
        sidebar_key: (u64, u64, bool),
        settings: &SettingsOptions,
        hide_effects: bool,
        catalog: &Arc<Catalog>,
    ) -> HudChanges {
        let reset = self.session != Some(runtime.session_id())
            || self
                .catalog
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, catalog));
        let language = runtime.text_generation();
        let language_changed = self.language != language;
        if reset {
            self.session = Some(runtime.session_id());
            self.catalog = Some(Arc::clone(catalog));
        }
        if reset || language_changed {
            self.language = language;
            self._translator = runtime.translator();
        }
        let old_hotbar_visible = self.model.hotbar_visible;
        let old_survival = self.model.survival_ui;
        let mut changes = HudChanges {
            status: self.refresh_status(player, runtime, frame, settings, hide_effects) || reset,
            ..Default::default()
        };
        if reset || !self.hotbar_matches(player, runtime, frame) {
            self.refresh_hotbar(player, runtime, frame);
            changes.hotbar = true;
        }
        changes.hotbar |= old_hotbar_visible != self.model.hotbar_visible;
        changes.text = self.refresh_text(player, runtime, frame) || reset;
        // Item-name controls bind survival padding at creation.
        changes.text |= old_survival != self.model.survival_ui;
        changes.chat = self.refresh_chat(
            runtime,
            frame.now_millis,
            settings,
            reset || language_changed,
        );
        if reset || self.sidebar_key != Some(sidebar_key) {
            self.model.sidebar = sidebar.map(sidebar_model);
            self.sidebar_key = Some(sidebar_key);
            changes.sidebar = true;
        }
        if reset
            || !runtime
                .boss_bars()
                .stacked_iter()
                .take(MAX_BOSS_BARS)
                .eq(self.bosses.iter().cloned())
        {
            self.bosses = runtime
                .boss_bars()
                .stacked_iter()
                .take(MAX_BOSS_BARS)
                .collect();
            self.model.boss_bars = self.bosses.iter().map(boss_model).collect();
            changes.boss_bars = true;
        }
        let world_text = world_text_input(runtime, frame);
        if reset || language_changed || world_text != self.world_text {
            (self.model.player_position, self.model.days_played) =
                world_text_lines(runtime, world_text);
            self.world_text = world_text;
            changes.world_text = true;
        }
        changes.world_text |= replace(
            &mut self.model.text_background_alpha,
            f64::from(settings.value(SettingId::HudTextBackgroundOpacity)) / 100.0,
        );
        #[cfg(test)]
        for (count, changed) in self.constructions.iter_mut().zip(changes.components()) {
            *count += usize::from(changed);
        }
        changes
    }

    /// Refreshes cheap visibility and numeric bindings without allocating a model key.
    fn refresh_status(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        settings: &SettingsOptions,
        hide_effects: bool,
    ) -> bool {
        let model = &mut self.model;
        let survival = player.facts.survival_stats_visible();
        let hotbar = player
            .facts
            .player_game_mode()
            .is_none_or(|mode| mode.shows_hotbar());
        let experience = runtime.hud().experience();
        let riding = survival && frame.mount_health.is_some();
        let bubbles = survival
            && runtime
                .hud()
                .air()
                .is_some_and(|air| air.current() < air.maximum());
        let mut changed = replace(&mut model.survival_ui, survival);
        changed |= replace(
            &mut model.armor_visible,
            runtime
                .hud()
                .armor()
                .is_some_and(|armor| armor.current() > 0),
        );
        changed |= replace(
            &mut model.hotbar_visible,
            hotbar && player.selected_hotbar_slot().is_some(),
        );
        changed |= replace(
            &mut model.xp_bar,
            survival && experience.is_some() && frame.mount_jump.is_none(),
        );
        changed |= replace(
            &mut model.exp_progress,
            experience.map_or(0.0, |xp| f64::from(xp.progress)),
        );
        changed |= replace(&mut model.level, experience.map_or(0, |xp| xp.level));
        changed |= replace(&mut model.riding_hearts, riding);
        changed |= replace(&mut model.bubbles_visible, bubbles);
        changed |= replace(
            &mut model.paper_doll,
            frame.paper_doll_visible && hotbar && settings.value(SettingId::HidePaperdoll) == 0,
        );
        changed |= replace(
            &mut model.effects_visible,
            !hide_effects
                && runtime.gameplay_hud().effects().iter().any(|effect| {
                    effect.visible_at_tick(runtime.estimated_server_tick(frame.now_millis))
                }),
        );
        changed |= replace(&mut model.spectator, !hotbar);
        changed
    }

    /// Compares the already captured inventory projection by the values the HUD displays.
    fn hotbar_matches(
        &self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> bool {
        let facts = player.inventory.ledger().item_facts();
        self.model.hotbar.len() == frame.hotbar_stacks.len()
            && self.model.hotbar.iter().enumerate().all(|(index, slot)| {
                self.slot_matches(
                    facts,
                    slot,
                    frame.hotbar_stacks[index].as_ref(),
                    frame.hotbar_icons[index],
                    frame.hotbar_durability[index],
                    player.selected_hotbar_slot() == Some(index as u8),
                )
            })
            && match (&self.model.offhand, runtime.local_offhand_stack(player)) {
                (None, None) => true,
                (Some(slot), Some(stack)) => self.slot_matches(
                    facts,
                    slot,
                    Some(stack),
                    frame.offhand_icon,
                    frame.offhand_durability,
                    false,
                ),
                _ => false,
            }
    }

    /// Checks an item cell against its retained icon index and displayed stack values.
    fn slot_matches(
        &self,
        facts: &inventory::SessionItemFacts,
        slot: &HudSlot,
        stack: Option<&protocol::NetworkItemStack>,
        icon: Option<IconRef>,
        durability: Option<f32>,
        selected: bool,
    ) -> bool {
        slot.count == stack.map_or(0, |stack| u32::from(stack.count))
            && slot.id_aux == stack.map(|stack| facts.id_aux(stack))
            && slot.selected == selected
            && slot.durability == stack.and(durability).map(f64::from)
            && slot.icon.and_then(|index| self.icons.get(index).copied()) == stack.and(icon)
    }

    /// Rebuilds hotbar cells and their shared dense icon table together, preserving all references.
    fn refresh_hotbar(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) {
        self.icons.clear();
        let facts = player.inventory.ledger().item_facts();
        let mut slot = |stack: Option<&protocol::NetworkItemStack>,
                        icon: Option<IconRef>,
                        durability: Option<f32>,
                        selected: bool| {
            let icon = stack.and(icon).map(|icon| {
                self.icons.push(icon);
                self.icons.len() - 1
            });
            HudSlot {
                icon,
                id_aux: stack.map(|stack| facts.id_aux(stack)),
                count: stack.map_or(0, |stack| u32::from(stack.count)),
                selected,
                durability: stack.and(durability).map(f64::from),
            }
        };
        self.model.hotbar = (0..frame.hotbar_stacks.len())
            .map(|index| {
                slot(
                    frame.hotbar_stacks[index].as_ref(),
                    frame.hotbar_icons[index],
                    frame.hotbar_durability[index],
                    player.selected_hotbar_slot() == Some(index as u8),
                )
            })
            .collect();
        self.model.offhand = runtime.local_offhand_stack(player).map(|stack| {
            slot(
                Some(stack),
                frame.offhand_icon,
                frame.offhand_durability,
                false,
            )
        });
    }

    /// Keeps text and creation clocks until a request or a visibility boundary changes them.
    fn refresh_text(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> bool {
        let now = frame.now_millis;
        let title = runtime.hud().title().filter(|text| text.visible_at(now));
        let subtitle = runtime.hud().subtitle().filter(|text| text.visible_at(now));
        let mut changed = refresh_title(&mut self.model.title, title, subtitle);
        changed |= refresh_controller_text(&mut self.model.title_text, runtime.hud().title_text());
        changed |=
            refresh_controller_text(&mut self.model.subtitle_text, runtime.hud().subtitle_text());
        changed |= refresh_timed(
            &mut self.model.actionbar,
            runtime
                .hud()
                .actionbar()
                .filter(|text| text.visible_at(now))
                .map(|text| (text.text.as_ref(), text.started_millis)),
        );
        changed |= refresh_timed(
            &mut self.model.tip,
            runtime
                .hud()
                .tip()
                .filter(|text| text.visible_at(now))
                .map(|text| (text.text.as_ref(), text.started_millis)),
        );
        let item_name = runtime
            .selected_item_changed_millis()
            .filter(|changed| now.saturating_sub(*changed) < ITEM_NAME_MILLIS)
            .zip(frame.selected_item_name.as_deref())
            .filter(|_| self.model.hotbar_visible && player.selected_hotbar_slot().is_some())
            .map(|(changed, name)| (bounded_visible_text(name), changed));
        changed |= refresh_timed(&mut self.model.item_name, item_name);
        changed
    }

    /// Resolves chat only when its bounded row identities, expiry, language or settings change.
    fn refresh_chat(
        &mut self,
        runtime: &UiRuntime,
        now: u64,
        settings: &SettingsOptions,
        reset: bool,
    ) -> bool {
        let hidden = settings.value(SettingId::HideChat) != 0;
        let mention_code = settings.mentions_color_code();
        let options = (
            settings.chat_lifetime(),
            settings.chat_color_code(),
            mention_code,
            hidden,
        );
        let horizon = seconds(now) - options.0 - 1.0;
        let rows = || {
            runtime
                .chat()
                .observed_rows()
                .skip(
                    runtime
                        .chat()
                        .messages()
                        .len()
                        .saturating_sub(MAX_CHAT_LINES),
                )
                .filter(|(_, line)| !hidden && seconds(line.received_millis) > horizon)
        };
        let mut changed = replace(
            &mut self.model.chat_color,
            ui_screens::settings_chat::default_color(settings),
        );
        if reset
            || self.chat_options != Some(options)
            || !rows().map(|(id, _)| id).eq(self.chat_rows.iter().copied())
        {
            self.chat_rows.clear();
            self.model.chat = rows()
                .map(|(id, line)| {
                    let born = line.received_millis.min(now);
                    self.chat_rows.push(id);
                    let text = resolve_chat_line_formatted(
                        line,
                        |key| runtime.translation(key),
                        Some(mention_code),
                    );
                    Timed {
                        text: bounded_visible_text(text.as_ref()).to_owned(),
                        born: seconds(born),
                    }
                })
                .collect();
            self.chat_options = Some(options);
            changed = true;
        }
        // A future timestamp advances only the row clock, keeping its resolved text.
        for ((_, line), model) in rows().zip(&mut self.model.chat) {
            changed |= replace(&mut model.born, seconds(line.received_millis.min(now)));
        }
        changed |= replace(
            &mut self.model.chat_visible,
            !hidden && !runtime.chat_focused() && !runtime.inventory_open(),
        );
        changed |= replace(&mut self.model.chat_lifetime, options.0);
        changed |= replace(
            &mut self.model.chat_background_opacity,
            f64::from(settings.value(SettingId::ChatBackgroundOpacity)) / 100.0,
        );
        changed |= replace(
            &mut self.model.chat_text,
            ui_screens::settings_chat::chat_text(settings),
        );
        changed
    }
}

/// Replaces a cheap semantic value and reports whether its bindings need refreshing.
fn replace<T: PartialEq>(value: &mut T, next: T) -> bool {
    if *value == next {
        false
    } else {
        *value = next;
        true
    }
}

/// Converts the HUD's millisecond clock to template seconds.
fn seconds(millis: u64) -> f64 {
    millis as f64 / 1_000.0
}

/// Retains full pack binding inputs after native title instances finish their fades.
fn refresh_controller_text(model: &mut Option<String>, input: &str) -> bool {
    let input = bounded_text(input, protocol::MAX_UI_TEXT_BYTES);
    if model.as_deref() == Some(input) {
        return false;
    }
    *model = Some(input.to_owned());
    true
}

/// Allocates bounded timed text only when its visible content or creation clock changes.
fn refresh_timed(model: &mut Option<Timed>, input: Option<(&str, u64)>) -> bool {
    let input = input.map(|(text, born)| {
        (
            bounded_text(text, protocol::MAX_UI_TEXT_BYTES),
            seconds(born),
        )
    });
    if model.as_ref().map(|text| (text.text.as_str(), text.born)) == input {
        return false;
    }
    *model = input.map(|(text, born)| Timed {
        text: text.to_owned(),
        born,
    });
    true
}

/// Preserves title request identity even when consecutive requests contain the same text.
fn refresh_title(
    model: &mut Option<HudTitle>,
    title: Option<&TimedText>,
    subtitle: Option<&TimedText>,
) -> bool {
    let Some(title) = title else {
        return model.take().is_some();
    };
    let text = bounded_text(&title.text, protocol::MAX_UI_TEXT_BYTES);
    let subtitle = subtitle.map_or("", |text| {
        bounded_text(&text.text, protocol::MAX_UI_TEXT_BYTES)
    });
    let fade_in = seconds(title.fade_in_millis);
    let fade_out = seconds(title.fade_out_millis);
    let total = title.expires_millis.saturating_sub(title.started_millis);
    let stay = seconds(total.saturating_sub(title.fade_in_millis + title.fade_out_millis));
    let born = seconds(title.started_millis);
    if model.as_ref().is_some_and(|model| {
        model.creation_id == title.fifo_sequence
            && model.title == text
            && model.subtitle == subtitle
            && model.fade_in == fade_in
            && model.stay == stay
            && model.fade_out == fade_out
            && model.born == born
    }) {
        return false;
    }
    *model = Some(HudTitle {
        creation_id: title.fifo_sequence,
        title: text.to_owned(),
        subtitle: subtitle.to_owned(),
        fade_in,
        stay,
        fade_out,
        background_alpha: 0.0,
        born,
    });
    true
}

#[cfg(test)]
mod tests;

/// Exercises production world-text visibility without constructing the rest of a frame.
#[cfg(test)]
pub(super) fn world_text_lines_for_frame(
    runtime: &UiRuntime,
    frame: &HudFrame,
) -> (Option<String>, Option<String>) {
    world_text_lines(runtime, world_text_input(runtime, frame))
}
