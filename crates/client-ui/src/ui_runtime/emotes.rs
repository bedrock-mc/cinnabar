//! Local original emotes: native four-slot selection and unique equipped pieces.
use client_world::CustomEmote;
use launcher::menu::settings_options::EMOTE_SLOT_COUNT;
use ui::UiAction;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmotePlayback {
    pub emote: CustomEmote,
    pub start_millis: u64,
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
    Equip,
}

#[derive(Clone, Debug)]
pub struct EmoteState {
    slots: [Option<CustomEmote>; EMOTE_SLOT_COUNT],
    mode: Mode,
    selected: Option<usize>,
    playback: Option<EmotePlayback>,
    preferences: Option<[Option<String>; EMOTE_SLOT_COUNT]>,
}

impl Default for EmoteState {
    fn default() -> Self {
        let mut slots = [None; EMOTE_SLOT_COUNT];
        slots[0] = CustomEmote::ALL.first().copied();
        Self {
            slots,
            mode: Mode::Closed,
            selected: None,
            playback: None,
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
    pub fn change_emotes(&mut self) {
        self.mode = Mode::Equip;
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
        if self.is_equipping() {
            let emote = *CustomEmote::ALL.first()?;
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
        let emote = self.slots[slot]?;
        self.playback = Some(EmotePlayback {
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
            UiAction::Cancel if self.is_equipping() => self.mode = Mode::Wheel,
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
        let Some(saved) = saved else {
            return;
        };
        self.slots = saved
            .clone()
            .map(|value| value.as_deref().and_then(CustomEmote::from_id));
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
