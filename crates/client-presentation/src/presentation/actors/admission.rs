/// Admits non-player actors inside vanilla's camera-centred candidate cube, including its edge.
pub fn within_actor_candidate_cube(position: [f32; 3], camera: [f32; 3]) -> bool {
    !(0..3)
        .any(|axis| (position[axis] - camera[axis]).abs() > render::ACTOR_CANDIDATE_RADIUS_BLOCKS)
}

/// Rejects distance-ineligible actors independently of their authored frame scale.
pub(crate) fn actor_within_render_distance(
    actor: &client_world::ActorSnapshot,
    partial_tick: f32,
    view: Option<render::ActorCullView>,
) -> bool {
    let (Some(view), Some(feet)) = (
        view,
        super::interpolated_position(actor, partial_tick.clamp(0.0, 1.0)),
    ) else {
        return true;
    };
    (!matches!(actor.kind, protocol::ActorKind::Entity { .. })
        || within_actor_candidate_cube(feet, view.camera_position.to_array()))
        && view.contains_distance(feet)
}

/// Rejects distance-ineligible rigs before invoking their authored scale sampler.
pub(crate) fn sample_candidate_scale<'a>(
    rig: client_world::ActorRigSnapshot<'a>,
    actor: &client_world::ActorSnapshot,
    partial_tick: f32,
    view: Option<render::ActorCullView>,
    sample: impl FnOnce(client_world::ActorRigSnapshot<'a>) -> client_world::ActorRigSnapshot<'a>,
) -> Option<client_world::ActorRigSnapshot<'a>> {
    actor_within_render_distance(actor, partial_tick, view).then(|| sample(rig))
}
