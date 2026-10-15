/// Admits non-player actors inside vanilla's camera-centred candidate cube, including its edge.
pub fn within_actor_candidate_cube(position: [f32; 3], camera: [f32; 3]) -> bool {
    !(0..3).any(|axis| {
        (position[axis] - camera[axis]).abs() > render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS
    })
}

/// Rejects distant actors and player bodies containing the camera before frame model sampling.
pub(crate) fn actor_is_render_candidate(
    actor: &client_world::ActorSnapshot,
    partial_tick: f32,
    view: Option<render_model::ActorCullView>,
) -> bool {
    let (Some(view), Some(feet)) = (
        view,
        super::interpolated_position(actor, partial_tick.clamp(0.0, 1.0)),
    ) else {
        return true;
    };
    let camera = view.camera_position.to_array();
    if matches!(actor.kind, protocol::ActorKind::Player { .. })
        && view.camera_position.is_finite()
        && actor.bounding_box_at(feet).is_some_and(|(low, high)| {
            (0..3).all(|axis| {
                low[axis] < high[axis] && camera[axis] >= low[axis] && camera[axis] <= high[axis]
            })
        })
    {
        // Reject the body before skin and equipment layers can cover the entire view.
        return false;
    }
    (!matches!(actor.kind, protocol::ActorKind::Entity { .. })
        || within_actor_candidate_cube(feet, camera))
        && view.contains_distance(feet)
}

/// Rejects ineligible remote bodies before invoking their authored scale sampler.
pub(crate) fn sample_candidate_scale<'a>(
    rig: actor_animation::ActorRigSnapshot<'a>,
    actor: &client_world::ActorSnapshot,
    partial_tick: f32,
    view: Option<render_model::ActorCullView>,
    sample: impl FnOnce(actor_animation::ActorRigSnapshot<'a>) -> actor_animation::ActorRigSnapshot<'a>,
) -> Option<actor_animation::ActorRigSnapshot<'a>> {
    actor_is_render_candidate(actor, partial_tick, view).then(|| sample(rig))
}

/// Reads cached geometry choices without evaluating controllers or spending their frame budget.
pub(crate) fn rig_may_replace_geometry(
    rig: &actor_animation::ActorRigSnapshot<'_>,
    frame: Option<&actor_animation::ActorRenderFrame<'_>>,
) -> bool {
    rig.render.iter().any(|layer| layer.geometry.is_some())
        || frame.is_some_and(|frame| frame.may_replace_body_geometry(rig.actor.runtime_id))
}
