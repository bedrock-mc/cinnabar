//! Optional pack fog parsing shares carrier validation and transition data.

use assets::{FogDistance, FogDistanceMode, FogMedium, FogProfile, FogTransition};
use serde_json::Value;

/// Parses finite supported fog distances, dropping unfamiliar media independently.
pub(super) fn fog_profile(root: &Value) -> Option<FogProfile> {
    let settings = &root["minecraft:fog_settings"];
    let identifier = settings["description"]["identifier"].as_str()?;
    if identifier.is_empty() || identifier.len() > assets::MAX_ENVIRONMENT_IDENTIFIER_BYTES {
        return None;
    }
    let mut distances = Vec::new();
    for (medium, source) in settings["distance"].as_object()? {
        let Some(medium) = FogMedium::from_source_name(medium) else {
            continue;
        };
        let Some((mode, start_bits, end_bits, rgb8)) = distance_fields(source) else {
            continue;
        };
        let transition = if source["transition_fog"].is_null() {
            None
        } else if let Some(transition) = transition(&source["transition_fog"]) {
            Some(transition)
        } else {
            continue;
        };
        distances.push(FogDistance {
            medium,
            mode,
            start_bits,
            end_bits,
            rgb8,
            transition,
        });
    }
    distances.sort_by_key(|distance| distance.medium);
    (!distances.is_empty()).then(|| FogProfile {
        identifier: identifier.into(),
        distances: distances.into_boxed_slice(),
    })
}

/// Uses the same finite, ordered distance bounds as the pinned atmosphere carrier.
fn distance_fields(source: &Value) -> Option<(FogDistanceMode, u32, u32, u32)> {
    let mode = FogDistanceMode::from_source_name(source["render_distance_type"].as_str()?)?;
    let start = source["fog_start"].as_f64()? as f32;
    let end = source["fog_end"].as_f64()? as f32;
    if !start.is_finite() || !end.is_finite() || end < start {
        return None;
    }
    Some((
        mode,
        start.to_bits(),
        end.to_bits(),
        parse_rgb(&source["fog_color"])?,
    ))
}

/// Retains pack transition fields and delegates their bounds to the shared fog validator.
fn transition(source: &Value) -> Option<FogTransition> {
    let initial = &source["init_fog"];
    if !initial["transition_fog"].is_null() {
        return None;
    }
    let (mode, start_bits, end_bits, rgb8) = distance_fields(initial)?;
    let bits = |key: &str| source[key].as_f64().map(|value| (value as f32).to_bits());
    let transition = FogTransition {
        mode,
        start_bits,
        end_bits,
        rgb8,
        min_percent_bits: bits("min_percent")?,
        mid_seconds_bits: bits("mid_seconds")?,
        mid_percent_bits: bits("mid_percent")?,
        max_seconds_bits: bits("max_seconds")?,
    };
    transition.is_valid().then_some(transition)
}

/// Accepts Bedrock's RGB hex strings and numeric RGB triples.
pub(super) fn parse_rgb(value: &Value) -> Option<u32> {
    if let Some(text) = value.as_str() {
        let text = text.strip_prefix('#').unwrap_or(text);
        return (text.len() == 6)
            .then(|| u32::from_str_radix(text, 16).ok())
            .flatten();
    }
    let channels = value.as_array()?;
    if channels.len() != 3 {
        return None;
    }
    let mut rgb = 0;
    for channel in channels {
        let value = channel.as_f64()?;
        if !(0.0..=1.0).contains(&value) {
            return None;
        }
        rgb = (rgb << 8) | (value * 255.0).round() as u32;
    }
    Some(rgb)
}
