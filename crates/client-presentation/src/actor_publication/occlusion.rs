//! Bounds and preparation counters for terrain-culled actor bodies.

use crate::presentation::actors::ActorPresentationBatch;
use chunk_pipeline::WorldStream;

/// Work counts produced while admitting remote actor bodies.
#[derive(Default)]
pub(super) struct PreparationWork {
    pub(super) candidates: u32,
    pub(super) hidden_by_occlusion: u32,
}

impl PreparationWork {
    /// Records the admitted bodies and returns their count before extra layers are attached.
    pub(super) fn record_selection(
        &self,
        batch: &ActorPresentationBatch,
        local_runtime_id: u64,
    ) -> usize {
        record_work(
            self.candidates,
            self.hidden_by_occlusion,
            batch,
            local_runtime_id,
        );
        batch.submissions.len()
    }
}

/// Includes rotated body and shadow bounds. Attachments without a combined bound fail open
/// through non-finite bounds; name tags keep their independent publication path.
pub(super) fn body_and_shadow_bounds(
    stream: &WorldStream,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    partial_tick: f32,
    low: [f32; 3],
    high: [f32; 3],
) -> ([f32; 3], [f32; 3]) {
    let runtime_id = rig.actor.runtime_id;
    let authored = match rig.skin_geometry {
        Some(geometry) => geometry.visible_bounds,
        None => rig
            .geometry_source()
            .and_then(|(assets, geometry)| assets.geometry_visible_bounds(geometry)),
    };
    let equipment = crate::presentation::equipment::remote_input(stream, runtime_id);
    let cape = stream
        .authority()
        .actor_player_profile(runtime_id)
        .is_some_and(
            |p| matches!(&p.skin, protocol::PlayerSkin::Standard(skin) if skin.cape.is_some()),
        );
    // Attachments and extra skin/controller geometry have no combined bound here.
    if authored.is_none()
        || !rig.scale.is_finite()
        || rig.scale <= 0.0
        || low.iter().chain(&high).any(|v| !v.is_finite())
        || rig.axis_scale.iter().any(|s| !s.is_finite())
        || rig.render.len() > 1
        || !rig.skin_layers.is_empty()
        || cape
        || equipment.main.is_some()
        || equipment.off.is_some()
        || equipment.body.is_some()
        || equipment.armor.iter().any(Option::is_some)
    {
        return ([f32::NAN; 3], [f32::NAN; 3]);
    }
    let Some(feet) = stream
        .authority()
        .actor(runtime_id)
        .and_then(|actor| actor.interpolated_position(partial_tick.clamp(0.0, 1.0)))
    else {
        return ([f32::NAN; 3], [f32::NAN; 3]);
    };
    // A sphere about the feet contains the authored box under yaw and death tilt.
    let reach: [f32; 3] =
        std::array::from_fn(|a| (low[a] - feet[a]).abs().max((high[a] - feet[a]).abs()));
    let radius = reach.iter().map(|v| v * v).sum::<f32>().sqrt();
    let low = feet.map(|v| v - radius);
    let high = feet.map(|v| v + radius);
    stream
        .authority()
        .actor_shadow_caster(runtime_id, partial_tick)
        .map_or((low, high), |caster| {
            let shadow = render_model::EntityShadow {
                feet: caster.feet,
                radius: caster.radius,
            };
            let (shadow_low, shadow_high) = shadow.bounds();
            (
                std::array::from_fn(|a| low[a].min(shadow_low[a])),
                std::array::from_fn(|a| high[a].max(shadow_high[a])),
            )
        })
}

/// Reports remote candidates, rejected occlusion proofs and submitted bodies after selection.
pub(super) fn record_work(
    candidates: u32,
    hidden_by_occlusion: u32,
    batch: &ActorPresentationBatch,
    local_runtime_id: u64,
) {
    bevy::log::trace!(
        candidates,
        hidden_by_occlusion,
        submitted = batch
            .submissions
            .iter()
            .filter(|s| s.input.identity.runtime_id != local_runtime_id)
            .count(),
        "actor preparation work"
    );
}
