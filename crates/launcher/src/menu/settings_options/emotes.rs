//! Equipped emote IDs are preferences; the client catalog validates availability.
use super::SettingsOptions;

const MAX_EMOTE_ID_BYTES: usize = 128;
/// Vanilla's emote wheel equips top/right/bottom/left slots.
pub const EMOTE_SLOT_COUNT: usize = 4;

impl SettingsOptions {
    pub fn emote_slots(&self) -> Option<&[Option<String>; EMOTE_SLOT_COUNT]> {
        self.emote_slots.as_ref()
    }

    pub fn set_emote_slots(&mut self, slots: [Option<String>; EMOTE_SLOT_COUNT]) -> bool {
        let slots = slots.map(|id| {
            id.filter(|id| {
                !id.is_empty()
                    && id.len() <= MAX_EMOTE_ID_BYTES
                    && !id.chars().any(char::is_control)
            })
        });
        if self.emote_slots.as_ref() == Some(&slots) {
            return false;
        }
        self.emote_slots = Some(slots);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_and_explicit_empty_slots_are_distinct_and_preferences_round_trip() {
        let mut settings = SettingsOptions::default();
        assert!(settings.emote_slots().is_none());
        assert!(settings.set_emote_slots([None, None, None, None]));
        let bytes = serde_json::to_vec(&settings).unwrap();
        let loaded = SettingsOptions::decode(&bytes).unwrap();
        assert_eq!(loaded.emote_slots(), Some(&[None, None, None, None]));
        assert!(!settings.set_emote_slots([None, None, None, None]));
    }

    #[test]
    fn stored_emote_ids_are_bounded_without_requiring_a_launcher_catalog() {
        let mut settings = SettingsOptions::default();
        settings.set_emote_slots([
            Some("custom:fixture".into()),
            Some("x".repeat(MAX_EMOTE_ID_BYTES + 1)),
            Some("bad\nvalue".into()),
            None,
        ]);
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(
            loaded.emote_slots().unwrap(),
            &[Some("custom:fixture".into()), None, None, None]
        );
    }
}
