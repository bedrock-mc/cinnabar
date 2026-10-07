//! The sound component: a shorthand sound for every interacted button
//! event, plus `sounds[]` entries filtered by button and replay interval.

use serde_json::Value;

use crate::tree::ResolvedControl;

/// One sound with its volume and pitch (both default 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub name: String,
    pub volume: f32,
    pub pitch: f32,
}

/// A `sounds[]` entry: `event_type: "button_event"` is the only accepted type.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundEntry {
    pub sound: Sound,
    pub button_name: Option<String>,
    pub min_seconds_between_plays: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundMeta {
    pub shorthand: Option<Sound>,
    pub entries: Vec<SoundEntry>,
}

impl SoundMeta {
    /// Buttons and toggles always carry the component; other types only with `sounds`.
    pub(crate) fn read(control: &ResolvedControl, always: bool) -> Option<Self> {
        let sounds = control.properties.get("sounds").filter(|v| !v.is_null());
        if !always && sounds.is_none() {
            return None;
        }
        let entries = match sounds {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_object)
                .filter(|item| {
                    item.get("event_type").and_then(Value::as_str) == Some("button_event")
                })
                .filter_map(|item| {
                    Some(SoundEntry {
                        sound: sound(|key| item.get(key))?,
                        button_name: item
                            .get("button_name")
                            .and_then(Value::as_str)
                            .filter(|name| !name.is_empty())
                            .map(str::to_owned),
                        min_seconds_between_plays: item
                            .get("min_seconds_between_plays")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        Some(SoundMeta {
            shorthand: sound(|key| control.properties.get(key)),
            entries,
        })
    }
}

fn sound<'a>(get: impl Fn(&str) -> Option<&'a Value>) -> Option<Sound> {
    let name = get("sound_name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())?;
    let number = |key: &str| get(key).and_then(Value::as_f64).unwrap_or(1.0) as f32;
    Some(Sound {
        name: name.to_owned(),
        volume: number("sound_volume"),
        pitch: number("sound_pitch"),
    })
}
