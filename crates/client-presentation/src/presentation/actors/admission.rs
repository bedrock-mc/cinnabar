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

/// Admits a placed remote before skin registration using conservative frame-selected boxes.
/// Runtime skins cover their body and sampled persona boxes; catalog bodies cover base fallback.
/// Unknown selected bounds fail open. The shared frame retains the sampled controller result.
pub(crate) fn admit_frame_bounds(
    presentation: &mut super::ActorRigPresentation,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    view: Option<render_model::ActorCullView>,
    frame: Option<&mut actor_animation::ActorRenderFrame<'_>>,
) -> bool {
    use render_model::actor_rig_submission_is_visible;

    if view.is_none() {
        return true;
    }
    let feet = presentation.submission.world_from_actor.map(|row| row[3]);
    if view.is_some_and(|view| !view.contains_distance(feet)) {
        return false;
    }
    let mut visible = actor_rig_submission_is_visible(&presentation.submission, view);
    let mut bounds = presentation.submission.culling_bounds;
    for layer in rig.skin_layers {
        let Some(selected) = layer.geometry.visible_bounds else {
            presentation.submission.culling_bounds = bounds;
            return true;
        };
        presentation.submission.culling_bounds = selected;
        visible |= actor_rig_submission_is_visible(&presentation.submission, view);
        bounds = union(bounds, selected);
    }
    presentation.submission.culling_bounds = bounds;
    // The layer resolver ignores catalog geometry choices for a runtime player skin.
    if presentation.artwork.is_none() || !rig_may_replace_geometry(rig, frame.as_deref()) {
        return visible;
    }
    let Some((assets, _)) = rig.geometry_source() else {
        return true;
    };
    let layers = match frame {
        Some(frame) => match frame.layers_with_skin(rig.actor.runtime_id) {
            Some(layers) => layers.render,
            None => return true,
        },
        None => std::borrow::Cow::Borrowed(rig.render),
    };
    for layer in layers.iter() {
        let Some(geometry) = layer.geometry else {
            continue;
        };
        let Some(selected) = assets.geometry_visible_bounds(geometry as usize) else {
            presentation.submission.culling_bounds = bounds;
            return true;
        };
        presentation.submission.culling_bounds = selected;
        visible |= actor_rig_submission_is_visible(&presentation.submission, view);
        bounds = union(bounds, selected);
    }
    presentation.submission.culling_bounds = bounds;
    visible
}

/// Covers both admitted model boxes without overflow when their finite offsets are far apart.
fn union(
    a: assets::SkinGeometryBounds,
    b: assets::SkinGeometryBounds,
) -> assets::SkinGeometryBounds {
    let low: [f32; 3] = std::array::from_fn(|axis| {
        (a.center[axis] - a.half_extents[axis]).min(b.center[axis] - b.half_extents[axis])
    });
    let high: [f32; 3] = std::array::from_fn(|axis| {
        (a.center[axis] + a.half_extents[axis]).max(b.center[axis] + b.half_extents[axis])
    });
    assets::SkinGeometryBounds {
        center: std::array::from_fn(|axis| low[axis] * 0.5 + high[axis] * 0.5),
        half_extents: std::array::from_fn(|axis| high[axis] * 0.5 - low[axis] * 0.5),
    }
}
