//! Local original emotes: native four-slot selection and unique equipped pieces.
use actor_animation::CustomEmote;
use launcher::menu::settings_options::EMOTE_SLOT_COUNT;
use ui::UiAction;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmotePlayback {
    pub emote: CustomEmote,
    pub start_millis: u64,
    /// Distinguishes repeated starts even when the render clock has not advanced.
    pub revision: u64,
}

impl EmotePlayback {
    pub fn elapsed(self, now_millis: u64) -> f64 {
        now_millis.saturating_sub(self.start_millis) as f64 / 1_000.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Closed,
    Wheel,
    Choose,
    Equip,
}

#[derive(Clone, Debug)]
pub struct EmoteState {
    slots: [Option<CustomEmote>; EMOTE_SLOT_COUNT],
    mode: Mode,
    selected: Option<usize>,
    playback: Option<EmotePlayback>,
    playback_revision: u64,
    catalog: Vec<CustomEmote>,
    catalog_page: usize,
    chosen: Option<CustomEmote>,
    preferences: Option<[Option<String>; EMOTE_SLOT_COUNT]>,
}

impl Default for EmoteState {
    fn default() -> Self {
        let mut slots = [None; EMOTE_SLOT_COUNT];
        for (slot, emote) in slots.iter_mut().zip(CustomEmote::ALL) {
            *slot = Some(*emote);
        }
        Self {
            slots,
            mode: Mode::Closed,
            selected: None,
            playback: None,
            playback_revision: 0,
            catalog: Vec::new(),
            catalog_page: 0,
            chosen: None,
            preferences: None,
        }
    }
}

impl EmoteState {
    pub fn open(&mut self) {
        self.mode = Mode::Wheel;
        self.selected = None;
    }

    pub fn close(&mut self) {
        self.mode = Mode::Closed;
    }
    pub fn is_open(&self) -> bool {
        self.mode != Mode::Closed
    }
    pub fn is_equipping(&self) -> bool {
        self.mode == Mode::Equip
    }
    pub fn slots(&self) -> &[Option<CustomEmote>; EMOTE_SLOT_COUNT] {
        &self.slots
    }
    pub fn selected_slot(&self) -> Option<usize> {
        self.selected
    }
    pub fn hover_slot(&mut self, slot: Option<usize>) {
        self.selected = slot.filter(|slot| *slot < self.slots.len());
    }
    /// Opens a paged list of supported clips the account owns.
    pub fn change_emotes(&mut self) {
        if self.is_choosing() {
            self.next_catalog_page();
            return;
        }
        self.catalog = CustomEmote::available();
        self.catalog_page = 0;
        self.chosen = None;
        self.selected = None;
        self.mode = Mode::Choose;
    }
    /// Reports whether the wheel currently lists catalog choices.
    pub fn is_choosing(&self) -> bool {
        self.mode == Mode::Choose
    }
    /// Returns the chosen clip while selecting its destination slot.
    pub fn equipping_emote(&self) -> Option<CustomEmote> {
        self.chosen
    }
    /// Returns the current catalog page and total page count for instructions.
    pub fn catalog_page(&self) -> Option<(usize, usize)> {
        self.is_choosing().then_some((
            self.catalog_page,
            self.catalog.len().div_ceil(EMOTE_SLOT_COUNT).max(1),
        ))
    }
    /// Cycles catalog pages without changing equipped slots or playback.
    pub fn next_catalog_page(&mut self) {
        if let Some((page, count)) = self.catalog_page() {
            self.catalog_page = (page + 1) % count;
            self.selected = None;
        }
    }
    /// Lists catalog choices during browsing, otherwise the four equipped slots.
    pub fn display_slots(&self) -> [Option<CustomEmote>; EMOTE_SLOT_COUNT] {
        if self.is_choosing() {
            std::array::from_fn(|slot| {
                self.catalog
                    .get(self.catalog_page * EMOTE_SLOT_COUNT + slot)
                    .copied()
            })
        } else {
            self.slots
        }
    }
    pub fn playback(&self) -> Option<EmotePlayback> {
        self.playback
    }
    pub fn stop(&mut self) {
        self.playback = None;
    }

    pub fn activate_slot(&mut self, slot: usize, now_millis: u64) -> Option<CustomEmote> {
        if !self.is_open() || slot >= self.slots.len() {
            return None;
        }
        self.selected = Some(slot);
        if self.is_choosing() {
            self.chosen =
                self.display_slots()[slot].filter(|emote| CustomEmote::available().contains(emote));
            if self.chosen.is_some() {
                self.mode = Mode::Equip;
                self.selected = None;
            }
            return None;
        }
        if self.is_equipping() {
            let emote = self
                .equipping_emote()
                .filter(|emote| CustomEmote::available().contains(emote))?;
            if self.slots[slot] != Some(emote) {
                if let Some(previous) = self.slots.iter().position(|value| *value == Some(emote)) {
                    self.slots.swap(previous, slot);
                } else {
                    self.slots[slot] = Some(emote);
                }
                self.preferences = Some(
                    self.slots
                        .map(|value| value.map(|emote| emote.id().to_owned())),
                );
            }
            self.mode = Mode::Wheel;
            return None;
        }
        let emote = self.slots[slot].filter(|emote| CustomEmote::available().contains(emote))?;
        self.playback_revision = self.playback_revision.wrapping_add(1);
        self.playback = Some(EmotePlayback {
            revision: self.playback_revision,
            emote,
            start_millis: now_millis,
        });
        self.close();
        Some(emote)
    }

    pub fn handle_action(&mut self, action: UiAction, now_millis: u64) -> Option<CustomEmote> {
        if !self.is_open() {
            return None;
        }
        match action {
            UiAction::Cancel if self.is_equipping() => self.mode = Mode::Choose,
            UiAction::Cancel if self.is_choosing() => self.mode = Mode::Wheel,
            UiAction::Cancel => self.close(),
            UiAction::Accept => return self.activate_slot(self.selected.unwrap_or(0), now_millis),
            UiAction::Navigate([x, y]) if x != 0 || y != 0 => {
                self.selected = Some(if y < 0 {
                    0
                } else if x > 0 {
                    1
                } else if y > 0 {
                    2
                } else {
                    3
                });
            }
            _ => {}
        }
        None
    }

    pub fn apply_preferences(&mut self, saved: Option<&[Option<String>; EMOTE_SLOT_COUNT]>) {
        let available = CustomEmote::available();
        if self
            .playback
            .is_some_and(|playback| !available.contains(&playback.emote))
        {
            self.stop();
        }
        if self.is_choosing() || self.is_equipping() {
            self.catalog = available.clone();
            self.catalog_page = self.catalog_page.min(
                self.catalog
                    .len()
                    .div_ceil(EMOTE_SLOT_COUNT)
                    .saturating_sub(1),
            );
            if self
                .chosen
                .is_some_and(|chosen| !available.contains(&chosen))
            {
                self.chosen = None;
                self.mode = Mode::Choose;
            }
        }
        let Some(saved) = saved else {
            return;
        };
        self.slots = saved.clone().map(|value| {
            value
                .as_deref()
                .and_then(CustomEmote::from_id)
                .filter(|emote| available.contains(emote))
        });
        for slot in 0..self.slots.len() {
            if self.slots[slot].is_some() && self.slots[..slot].contains(&self.slots[slot]) {
                self.slots[slot] = None;
            }
        }
    }

    pub fn take_preferences(&mut self) -> Option<[Option<String>; EMOTE_SLOT_COUNT]> {
        self.preferences.take()
    }

    pub fn reset(&mut self) {
        self.close();
        self.stop();
        self.selected = None;
    }
}

impl super::UiRuntime {
    pub fn emotes(&self) -> &EmoteState {
        &self.emotes
    }
    pub fn emotes_mut(&mut self) -> &mut EmoteState {
        &mut self.emotes
    }
}

#[cfg(test)]
mod tests;
