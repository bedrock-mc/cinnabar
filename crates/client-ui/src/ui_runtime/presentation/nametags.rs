//! Native name-tag extraction and font-pixel records for the retained GPU atlas.
//! See docs/reference/nametag-rendering.md for the vanilla rules.

use std::sync::Arc;

use bevy::{camera::Camera, math::Vec3, prelude::GlobalTransform};
use client_world::ActorSnapshot;
use protocol::{ActorKind, ActorMetadataValue};

use view_presentation::nametags::{NametagAnchor, DEFAULT_RENDER_DISTANCE, HEAD_CLEARANCE, DEFAULT_HEIGHT, SNEAKING_HEIGHT};

#[cfg(test)]
pub(super) mod tests;

/// Presentation resource bound, not a native visibility rule.
pub(super) const MAX_PRESENTED_NAMETAGS: usize = 128;
const METADATA_HEIGHT: u32 = 54;
const METADATA_ALWAYS_SHOW_NAMETAG: u32 = 81;
const METADATA_SCORE: u32 = 84;
const METADATA_RENDER_DISTANCE: u32 = 140;
const ACTOR_FLAG_SNEAKING: u32 = 1;
const ACTOR_FLAG_INVISIBLE: u32 = 5;
const ACTOR_FLAG_SHOW_NAME: u32 = 14;
const ACTOR_FLAG_ALWAYS_SHOW_NAME: u32 = 15;
const SCORE_DISTANCE_SQUARED: f32 = 100.0;

fn actor_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    matches!(actor.metadata.get(&0),
        Some(ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags))
            if flags & (1_u64 << bit) != 0)
}

fn tag_height(actor: &ActorSnapshot) -> f32 {
    let height = match actor.metadata.get(&METADATA_HEIGHT) {
        Some(ActorMetadataValue::Float(height)) if height.is_finite() && *height > 0.0 => *height,
        _ if actor_flag(actor, ACTOR_FLAG_SNEAKING) => SNEAKING_HEIGHT * actor.render_scale(),
        _ => DEFAULT_HEIGHT * actor.render_scale(),
    };
    height + HEAD_CLEARANCE
}

fn tag_world_position(actor: &ActorSnapshot, partial_tick: f32) -> Option<Vec3> {
    Some(
        Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?)
            + Vec3::Y * tag_height(actor),
    )
}

fn render_distance(actor: &ActorSnapshot) -> f32 {
    match actor.metadata.get(&METADATA_RENDER_DISTANCE) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() && *value >= 0.0 => *value,
        _ => DEFAULT_RENDER_DISTANCE,
    }
}

fn visible(actor: &ActorSnapshot, picked_actor: Option<u64>) -> bool {
    if actor_flag(actor, ACTOR_FLAG_INVISIBLE) {
        return false;
    }
    let always = matches!(actor.kind, ActorKind::Player { .. })
        || actor_flag(actor, ACTOR_FLAG_ALWAYS_SHOW_NAME)
        || matches!(actor.metadata.get(&METADATA_ALWAYS_SHOW_NAMETAG),
            Some(ActorMetadataValue::Byte(value)) if *value != 0);
    always || (actor_flag(actor, ACTOR_FLAG_SHOW_NAME) && picked_actor == Some(actor.runtime_id))
}

fn tag_text(
    actor: &ActorSnapshot,
    name: Arc<str>,
    distance_squared: f32,
    scoreboards: &ui::ScoreboardStore,
) -> Arc<str> {
    if distance_squared >= SCORE_DISTANCE_SQUARED {
        return name;
    }
    // Empty synced score text is authoritative; retained objectives are only a fallback.
    match actor.metadata.get(&METADATA_SCORE) {
        Some(ActorMetadataValue::String(score)) if !score.is_empty() => {
            format!("{name}\n{score}").into()
        }
        Some(ActorMetadataValue::String(_)) => name,
        _ => scoreboards
            .below_name_for_owner(&ui::ScoreOwner::Player(actor.unique_id))
            .or_else(|| scoreboards.below_name_for_owner(&ui::ScoreOwner::Entity(actor.unique_id)))
            .map_or_else(
                || Arc::clone(&name),
                |(score, objective)| format!("{name}\n{score} {objective}").into(),
            ),
    }
}

pub fn extract_nametag(
    actor: &ActorSnapshot,
    eye: Vec3,
    picked_actor: Option<u64>,
    name: Arc<str>,
    scoreboards: &ui::ScoreboardStore,
    partial_tick: f32,
) -> Option<NametagAnchor> {
    if name.is_empty() || !visible(actor, picked_actor) {
        return None;
    }
    let feet = Vec3::from_array(actor.interpolated_position(partial_tick.clamp(0.0, 1.0))?);
    let distance_squared = eye.distance_squared(feet);
    let maximum = render_distance(actor);
    if !distance_squared.is_finite() || distance_squared > maximum * maximum {
        return None;
    }
    let position = tag_world_position(actor, partial_tick)?;
    if !position.is_finite() || eye == position || !(eye - position).length_squared().is_finite() {
        return None;
    }
    let text = tag_text(actor, name, distance_squared, scoreboards);
    view_presentation::nametags::nametag_anchor(
        actor.runtime_id, &text, feet, position, eye, actor_flag(actor, ACTOR_FLAG_SNEAKING), maximum,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn project_nametags(
    scoreboards: &ui::ScoreboardStore,
    stream: &chunk_pipeline::WorldStream,
    _camera: &Camera,
    camera_transform: &GlobalTransform,
    _logical_size: [f32; 2],
    picked_actor: Option<u64>,
    partial_tick: f32,
    show_players: bool,
) -> Vec<NametagAnchor> {
    let eye = camera_transform.translation();
    let mut anchors: Vec<_> = stream
        .authority()
        .remote_actors()
        .filter(|actor| show_players || !matches!(actor.kind, ActorKind::Player { .. }))
        .filter_map(|actor| {
            extract_nametag(
                actor,
                eye,
                picked_actor,
                stream.authority().actor_name_tag(actor.unique_id)?,
                scoreboards,
                partial_tick,
            )
        })
        .collect();
    anchors.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.runtime_id.cmp(&b.runtime_id))
    });
    anchors.truncate(MAX_PRESENTED_NAMETAGS);
    anchors
}
