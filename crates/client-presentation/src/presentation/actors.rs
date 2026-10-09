use std::{collections::HashMap, sync::Arc};

use assets::EntityRigFallback;
use client_world::{ActorRigSnapshot, ActorSnapshot, PlayerProfile};
use protocol::{ActorKind, PlayerSkin, SkinRgba8};
use render::{
    ActorArtworkLocation, ActorArtworkPages, ActorCullView, ActorRenderFrame, ActorRenderIdentity,
    ActorRenderScene, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    actor_bounds_are_visible, actor_rig_submission_is_visible, pack_overlay_rgba8,
};
use render_model::{
    ActorSkinPixels, EntityRigId, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    default_actor_skin_rgba8,
};

mod admission;
pub use admission::within_actor_candidate_cube;
pub(crate) use admission::{actor_within_render_distance, sample_candidate_scale};
mod tick_cache;
pub use tick_cache::PoseConversions;
use tick_cache::TickKey;
pub(crate) use tick_cache::convert_bones;

/// Damage tint blended over a hurt or dying actor.
const HURT_OVERLAY_RGBA: [f32; 4] = [1.0, 0.0, 0.0, client_world::HURT_OVERLAY_ALPHA];

#[derive(Clone, Debug)]
pub struct ActorRigPresentation {
    pub submission: ActorRigSubmission,
    pub skin_rgba8: Option<SkinRgba8>,
    pub artwork: Option<ActorArtworkLocation>,
    /// Authored model scale alone; the eye-anchored first-person hand ignores the metadata scale.
    pub authored_scale: f32,
    /// Body yaw used by the world transform, before axis scaling and death tilt.
    pub world_yaw_degrees: f32,
    /// Head yaw minus the rendered body yaw, in degrees.
    pub head_over_body: f32,
}

#[derive(Debug)]
pub struct ActorPresentationBatch {
    pub submissions: Vec<ActorRigSubmission>,
    /// One native-sized square RGBA8 skin per frame-local texture layer index.
    pub skin_layers: Vec<SkinRgba8>,
    pub artwork: HashMap<ActorRenderIdentity, ActorArtworkLocation>,
}

/// Publishes `batch`; its frame-local skin indices become the scene's stable skin slots.
pub fn update_actor_rig_scene(
    scene: &mut ActorRenderScene,
    partial_tick: f32,
    batch: ActorPresentationBatch,
) -> &ActorRenderFrame {
    // The app adapter has already applied the renderer's exact culling helper
    // to remotes before enforcing capacity. Passing no second cull view keeps
    // Phase 3's visible local reservation unconditional in both third-person
    // modes while the render-owned builder still validates every other field.
    scene.update_rigs_with_artwork(
        partial_tick,
        None,
        batch.submissions,
        &batch.skin_layers,
        &batch.artwork,
    )
}

/// Whether a rig can pass this frame's culling, judged before its presentation is built: non-player
/// actors must lie within vanilla's candidate cube, and no actor may be hidden by `occluded`
/// (given its culling box's low and high corners).
pub fn rig_may_be_visible(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    partial_tick: f32,
    view: Option<ActorCullView>,
    occluded: impl Fn([f32; 3], [f32; 3]) -> bool,
) -> bool {
    if !actor_within_render_distance(actor, partial_tick, view) {
        return false;
    }
    let (Some(view), Some(feet)) = (
        view,
        interpolated_position(actor, partial_tick.clamp(0.0, 1.0)),
    ) else {
        return true;
    };
    // Per-axis scale and the death tilt never lengthen the up axis past the largest axis scale.
    let largest_axis = rig
        .axis_scale
        .iter()
        .fold(1.0_f32, |largest, axis| largest.max(axis.abs()));
    let scale = rig.scale * actor.render_scale() * largest_axis;
    let bounds = rig.culling_bounds();
    if !actor_bounds_are_visible(feet, scale, bounds, Some(view)) {
        return false;
    }
    let (low, high) = bounds.at(feet, scale);
    !occluded(low, high)
}

#[cfg(any(test, feature = "test-support"))]
pub fn entity_rig_presentation(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    artwork: &ActorArtworkPages,
    partial_tick: f32,
) -> Option<ActorRigPresentation> {
    entity_rig_presentation_cached(rig, actor, artwork, partial_tick, None)
}

/// [`entity_rig_presentation`] reusing each rig's tick through `poses`, so frames of one tick
/// only re-place it.
pub fn entity_rig_presentation_cached(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    artwork: &ActorArtworkPages,
    partial_tick: f32,
    poses: Option<&mut PoseConversions>,
) -> Option<ActorRigPresentation> {
    if !partial_tick.is_finite() {
        return None;
    }
    let location = matches!(actor.kind, ActorKind::Entity { .. })
        .then(|| artwork.route(EntityRigId(rig.rig.0)))
        .flatten();
    let rest_mode =
        location.is_some_and(|location| location.pose_mode() == assets::ActorPoseMode::RestPose);
    let selected = if rest_mode {
        ActorRigSnapshot {
            previous: rig.rest,
            current: rig.rest,
            completed_tick: rig.rest_completed_tick,
            reset_generation: rig.rest_reset_generation,
            ..*rig
        }
    } else {
        *rig
    };
    let build = |poses: Option<&mut PoseConversions>| {
        let bad_rest = rest_mode
            && (rig.rest.is_empty()
                || rig.rest.len() != rig.previous.len()
                || rig.rest.len() != rig.current.len()
                || !rig.rest.iter().all(|bone| {
                    RenderBoneTransform::from_model_space_scaled(
                        bone.rotation,
                        bone.translation_scale,
                        bone.axis_scale,
                    )
                    .is_some()
                }));
        let mut tick = tick_presentation(&selected, actor, None, bad_rest, poses)?;
        if matches!(actor.kind, ActorKind::Entity { .. })
            && let Some(location) = location
        {
            let submission = &mut tick.presentation.submission;
            submission.route = match rig.fallback {
                EntityRigFallback::Skip => ActorRigRoute::Compiled,
                EntityRigFallback::GeometryOnly => ActorRigRoute::StaticFallback,
                EntityRigFallback::Diagnostic => ActorRigRoute::NoDraw,
            };
            if rest_mode {
                submission.route = if bad_rest || rig.fallback == EntityRigFallback::Diagnostic {
                    ActorRigRoute::NoDraw
                } else {
                    ActorRigRoute::StaticFallback
                };
            }
            submission.texture_layer = location.layer();
            tick.presentation.artwork = Some(location);
        }
        Some(tick)
    };
    let tick = match poses {
        Some(poses) => {
            let key = TickKey::new(&selected, rig, actor, None, location);
            poses.tick_presentation(key, |poses| build(Some(poses)))
        }
        None => build(None),
    }?;
    place(tick, &selected, actor, partial_tick)
}

/// Converts a transient render-time pose without retaining its allocation address.
pub fn actor_rig_presentation(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    partial_tick: f32,
) -> Option<ActorRigPresentation> {
    if !partial_tick.is_finite() {
        return None;
    }
    let tick = tick_presentation(rig, actor, profile, false, None)?;
    place(tick, rig, actor, partial_tick)
}

/// [`actor_rig_presentation`] reusing each rig's tick through `poses`.
pub fn actor_rig_presentation_cached(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    partial_tick: f32,
    poses: &mut PoseConversions,
) -> Option<ActorRigPresentation> {
    if !partial_tick.is_finite() {
        return None;
    }
    let key = TickKey::new(rig, rig, actor, profile, None);
    let tick = poses.tick_presentation(key, |poses| {
        tick_presentation(rig, actor, profile, false, Some(poses))
    })?;
    place(tick, rig, actor, partial_tick)
}

/// A presentation's parts that hold for a whole tick, before its per-frame placement.
#[derive(Clone, Debug)]
struct TickPresentation {
    presentation: ActorRigPresentation,
}

/// Validates the tick's pose and builds everything but the frame placement.
fn tick_presentation(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    rejected_pose: bool,
    poses: Option<&mut PoseConversions>,
) -> Option<TickPresentation> {
    if rig.actor.runtime_id != actor.runtime_id
        || rig.actor.spawn_revision != actor.spawn_revision
        || rig.actor.session_id == 0
        || rig.actor.runtime_id == 0
        || rig.actor.spawn_revision == 0
        || rig.completed_tick == 0
        || rig.reset_generation == 0
        || (!rejected_pose && (rig.previous.is_empty() || rig.previous.len() != rig.current.len()))
    {
        return None;
    }

    // A rejected submission retains exact ownership for observable NoDraw counts,
    // but contains no substitute pose and can never reach a GPU draw.
    let (previous_bones, current_bones) = if rejected_pose {
        (Arc::from([]), Arc::from([]))
    } else if let Some(poses) = poses {
        poses.convert(rig)?
    } else {
        (convert_bones(rig.previous)?, convert_bones(rig.current)?)
    };
    let (route, skin_rgba8) = player_route_and_skin(actor, profile, rig.fallback);
    Some(TickPresentation {
        presentation: ActorRigPresentation {
            submission: ActorRigSubmission {
                material: Default::default(),
                culling_bounds: rig.culling_bounds(),
                input: ActorRigRenderInput {
                    identity: ActorRenderIdentity {
                        session_id: rig.actor.session_id,
                        dimension: rig.actor.dimension,
                        runtime_id: rig.actor.runtime_id,
                        spawn_revision: rig.actor.spawn_revision,
                        // Movement fields belong to the frame; `place` fills them.
                        ingress_sequence: 0,
                        source_tick: None,
                        movement_revision: 0,
                        pose_generation: rig.completed_tick,
                        layer: render::ACTOR_LAYER_BODY,
                    },
                    rig: EntityRigId(rig.rig.0),
                    previous_bones,
                    current_bones,
                    completed_tick: rig.completed_tick,
                    reset_generation: rig.reset_generation,
                },
                world_from_actor: [[0.0; 4]; 3],
                texture_layer: u32::MAX,
                route,
                tint: 0,
                uv_anim: render::IDENTITY_UV_ANIM,
                light: 0,
                overlay_rgba8: 0,
            },
            skin_rgba8,
            artwork: None,
            authored_scale: rig.scale,
            world_yaw_degrees: 0.0,
            head_over_body: 0.0,
        },
    })
}

/// Places a tick's presentation at the frame's interpolated feet, facing, scale and overlay.
fn place(
    tick: TickPresentation,
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    partial_tick: f32,
) -> Option<ActorRigPresentation> {
    let alpha = partial_tick.clamp(0.0, 1.0);
    let (world_from_actor, yaw) = sampled_rig_placement(
        rig,
        actor,
        alpha,
        [
            rig.scale,
            rig.axis_scale[0],
            rig.axis_scale[1],
            rig.axis_scale[2],
        ],
    )?;
    let mut presentation = tick.presentation;
    let submission = &mut presentation.submission;
    let identity = &mut submission.input.identity;
    identity.ingress_sequence = actor.spawn_revision.max(actor.movement_revision);
    identity.source_tick = actor.source_tick;
    identity.movement_revision = actor.movement_revision;
    if !identity.is_exact() {
        return None;
    }
    submission.world_from_actor = world_from_actor;
    submission.overlay_rgba8 = if actor.hurt_overlay_active() {
        pack_overlay_rgba8(HURT_OVERLAY_RGBA)
    } else {
        0
    };
    presentation.world_yaw_degrees = yaw;
    presentation.head_over_body =
        wrap_degrees(lerp_degrees(actor.previous_pose.head_yaw, actor.head_yaw, alpha) - yaw);
    Some(presentation)
}

/// Places authored frame scales with the actor's metadata scale, feet, facing and tilts.
pub(crate) fn sampled_rig_placement(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    alpha: f32,
    authored: [f32; 4],
) -> Option<([[f32; 4]; 3], f32)> {
    let alpha = alpha.clamp(0.0, 1.0);
    let position = interpolated_position(actor, alpha)?;
    let yaw = if is_billboard(actor) || actor.target_rotation_is_absolute() {
        0.0
    } else {
        lerp_degrees(rig.previous_body_yaw, rig.body_yaw, alpha)
    };
    let scale = authored[0] * actor.render_scale();
    if !yaw.is_finite()
        || !scale.is_finite()
        || scale <= 0.0
        || authored.iter().any(|x| !x.is_finite())
    {
        return None;
    }
    let rows = glide_tilted(
        death_tilted(
            scaled_axes(
                rig_world_from_actor(position, yaw, scale),
                [authored[1], authored[2], authored[3]],
            ),
            actor.death_rotation_progress(alpha),
        ),
        glide_rotation(actor, alpha),
    );
    Some((rows, yaw))
}

pub fn local_diagnostic_presentation(
    actor_session_id: u64,
    dimension: i32,
    runtime_id: u64,
    pose_generation: u64,
    position: [f32; 3],
    yaw_degrees: f32,
    pitch_degrees: f32,
) -> Option<ActorRigPresentation> {
    if actor_session_id == 0
        || runtime_id == 0
        || pose_generation == 0
        || position.iter().any(|value| !value.is_finite())
        || !yaw_degrees.is_finite()
        || !pitch_degrees.is_finite()
    {
        return None;
    }
    let head_rotation = quaternion_from_euler_degrees([pitch_degrees, 0.0, 0.0]);
    let pivots = [
        [0.0, 1.75, 0.0],
        [0.0, 1.5, 0.0],
        [-0.3125, 1.375, 0.0],
        [-0.11875, 0.75, 0.0],
        [0.3125, 1.375, 0.0],
        [0.11875, 0.75, 0.0],
    ];
    let mut bones = pivots.map(|pivot| RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    });
    bones[0].rotation = head_rotation;
    Some(ActorRigPresentation {
        submission: ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: actor_session_id,
                    dimension,
                    runtime_id,
                    spawn_revision: actor_session_id,
                    ingress_sequence: pose_generation,
                    source_tick: None,
                    movement_revision: pose_generation,
                    pose_generation,
                    layer: render::ACTOR_LAYER_BODY,
                },
                rig: EntityRigId(u32::MAX),
                previous_bones: Arc::from(bones),
                current_bones: Arc::from(bones),
                completed_tick: pose_generation,
                reset_generation: actor_session_id,
            },
            // Same facing convention as the driven rig so the pre-rig fallback and the rig agree.
            world_from_actor: rig_world_from_actor(position, yaw_degrees, 1.0),
            texture_layer: u32::MAX,
            route: ActorRigRoute::Diagnostic,
            tint: 0,
            uv_anim: render::IDENTITY_UV_ANIM,
            light: 0,
            overlay_rgba8: 0,
        },
        skin_rgba8: Some(default_actor_skin_rgba8()),
        artwork: None,
        authored_scale: 1.0,
        world_yaw_degrees: yaw_degrees,
        head_over_body: 0.0,
    })
}

pub fn local_actor_presentation_for_visibility(
    local_runtime_id: u64,
    visibility_runtime_id: u64,
    canonical: Option<ActorRigPresentation>,
    diagnostic: Option<ActorRigPresentation>,
    yaw_degrees: f32,
) -> Option<ActorRigPresentation> {
    if local_runtime_id == 0 || visibility_runtime_id != local_runtime_id {
        return None;
    }
    let diagnostic = diagnostic.filter(|presentation| {
        presentation.submission.input.identity.runtime_id == local_runtime_id
    })?;
    match canonical {
        Some(mut canonical)
            if canonical.submission.input.identity.runtime_id == local_runtime_id =>
        {
            // The body lags the view yaw as the rig's head does, so the head faces the view.
            let feet = diagnostic.submission.world_from_actor.map(|row| row[3]);
            let yaw = yaw_degrees - canonical.head_over_body;
            let (sine, cosine) = (yaw - canonical.world_yaw_degrees).to_radians().sin_cos();
            let old = canonical.submission.world_from_actor;
            let rows = &mut canonical.submission.world_from_actor;
            for axis in 0..3 {
                rows[0][axis] = cosine * old[0][axis] - sine * old[2][axis];
                rows[2][axis] = sine * old[0][axis] + cosine * old[2][axis];
                rows[axis][3] = feet[axis];
            }
            canonical.world_yaw_degrees = yaw;
            Some(canonical)
        }
        Some(_) => None,
        None => Some(diagnostic),
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn select_actor_presentations(
    local_runtime_id: u64,
    local_visible: bool,
    local: Option<ActorRigPresentation>,
    remotes: impl IntoIterator<Item = ActorRigPresentation>,
) -> ActorPresentationBatch {
    select_actor_presentations_for_view(local_runtime_id, local_visible, local, remotes, None)
}

pub fn select_actor_presentations_for_view(
    local_runtime_id: u64,
    local_visible: bool,
    local: Option<ActorRigPresentation>,
    remotes: impl IntoIterator<Item = ActorRigPresentation>,
    view: Option<ActorCullView>,
) -> ActorPresentationBatch {
    // The newest identity of each remote actor wins; equal identities keep the first.
    let mut latest: Vec<ActorRigPresentation> = remotes
        .into_iter()
        .filter(|remote| {
            let runtime_id = remote.submission.input.identity.runtime_id;
            runtime_id != 0 && runtime_id != local_runtime_id
        })
        .collect();
    latest.sort_by(|a, b| {
        let (a, b) = (a.submission.input.identity, b.submission.input.identity);
        a.runtime_id.cmp(&b.runtime_id).then(b.cmp(&a))
    });
    latest.dedup_by_key(|remote| remote.submission.input.identity.runtime_id);

    let local = local_visible
        .then_some(local)
        .flatten()
        .filter(|local| local.submission.input.identity.runtime_id == local_runtime_id);
    let mut selected = Vec::with_capacity(latest.len() + usize::from(local.is_some()));
    if let Some(local) = local {
        selected.push(local);
    }
    for remote in latest {
        if remote.submission.route == ActorRigRoute::NoDraw {
            selected.push(remote);
            continue;
        }
        if !actor_rig_submission_is_visible(&remote.submission, view) {
            continue;
        }
        selected.push(remote);
    }

    let mut artwork = HashMap::with_capacity(selected.len());
    let mut skin_families = Vec::<SkinRgba8>::new();
    let mut skin_layer_of = HashMap::<SkinRgba8, usize>::new();
    let mut submissions = Vec::with_capacity(selected.len());
    for mut presentation in selected {
        if presentation.submission.route == ActorRigRoute::NoDraw {
            presentation.submission.texture_layer = u32::MAX;
            submissions.push(presentation.submission);
            continue;
        }
        if let Some(location) = presentation.artwork {
            artwork.insert(presentation.submission.input.identity, location);
            submissions.push(presentation.submission);
            continue;
        }
        let Some(skin) = presentation.skin_rgba8 else {
            presentation.submission.route = ActorRigRoute::NoDraw;
            presentation.submission.texture_layer = u32::MAX;
            submissions.push(presentation.submission);
            continue;
        };
        let layer = match skin_layer_of.entry(skin) {
            std::collections::hash_map::Entry::Occupied(layer) => *layer.get(),
            std::collections::hash_map::Entry::Vacant(layer)
                if skin_families.len() < MAX_RENDERED_PLAYERS =>
            {
                let index = skin_families.len();
                skin_families.push(layer.key().clone());
                *layer.insert(index)
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                presentation.submission.route = ActorRigRoute::NoDraw;
                presentation.submission.texture_layer = u32::MAX;
                submissions.push(presentation.submission);
                continue;
            }
        };
        presentation.submission.texture_layer =
            u32::try_from(layer).expect("actor skin family count is bounded");
        submissions.push(presentation.submission);
    }
    ActorPresentationBatch {
        submissions,
        skin_layers: skin_families,
        artwork,
    }
}

/// Appends the layers `layers_for` builds on each body already in the batch, reading the bodies
/// in place rather than from a copy.
pub fn attach_layers<Context>(
    batch: &mut ActorPresentationBatch,
    context: &mut Context,
    mut layers_for: impl for<'a> FnMut(
        &'a mut Context,
        &ActorRigSubmission,
    ) -> &'a mut [crate::presentation::equipment::EquipmentPresentation],
) {
    for index in 0..batch.submissions.len() {
        for layer in layers_for(context, &batch.submissions[index]) {
            batch
                .artwork
                .insert(layer.submission.input.identity, layer.location);
            batch.submissions.push(layer.submission.clone());
        }
    }
}

/// Lights each body at the reference body-height point; a body without solved light yet
/// keeps drawing unlit rather than black.
pub fn light_bodies(batch: &mut ActorPresentationBatch, stream: &chunk_pipeline::WorldStream) {
    for submission in &mut batch.submissions {
        let feet = submission.world_from_actor.map(|row| row[3]);
        let position = stream
            .authority()
            .actor(submission.input.identity.runtime_id)
            .map_or(feet, |actor| actor.brightness_sample_position(feet));
        if let Some((block, sky)) = stream.solved_light_at(position) {
            submission.light = render::pack_actor_light(block, sky);
        }
    }
}

/// Places a rig-frame model, which faces -Z with its right side at +X, so it faces the
/// Minecraft `yaw_degrees` direction at `position`, scaled about the feet.
pub fn rig_world_from_actor(position: [f32; 3], yaw_degrees: f32, scale: f32) -> [[f32; 4]; 3] {
    let (sine, cosine) = yaw_degrees.to_radians().sin_cos();
    [
        [-cosine * scale, 0.0, sine * scale, position[0]],
        [0.0, scale, 0.0, position[1]],
        [-sine * scale, 0.0, -cosine * scale, position[2]],
    ]
}

/// Scales the model's own axes (`scaleX`, `scaleY`, `scaleZ`) about its feet.
fn scaled_axes(mut rows: [[f32; 4]; 3], axis_scale: [f32; 3]) -> [[f32; 4]; 3] {
    for row in &mut rows {
        for (value, scale) in row.iter_mut().zip(axis_scale) {
            *value *= scale;
        }
    }
    rows
}

/// Tips the rig sideways about its feet as death progresses; the ease-out curve needs measurement.
pub fn death_tilted(mut rows: [[f32; 4]; 3], progress: Option<f32>) -> [[f32; 4]; 3] {
    let Some(progress) = progress else {
        return rows;
    };
    let angle = progress.clamp(0.0, 1.0).sqrt() * std::f32::consts::FRAC_PI_2;
    let (sine, cosine) = angle.sin_cos();
    for row in &mut rows {
        let (x, y) = (row[0], row[1]);
        row[0] = x * cosine + y * sine;
        row[1] = -x * sine + y * cosine;
    }
    rows
}

/// Vanilla's single-precision degree/radian factors for the glide tilt.
const DEGREES_TO_RADIANS: f32 = 0.017_453_292;
const RADIANS_TO_DEGREES: f32 = 57.295_776;
/// Cross products below this leave the gliding body unturned.
const GLIDE_TURN_DEAD_ZONE: f32 = 0.0625;

/// A gliding actor's `[pitch, yaw]` body tilt in degrees: pitch eases in over its first ten
/// gliding ticks, and yaw turns it toward its horizontal motion.
#[must_use]
pub fn glide_rotation(actor: &ActorSnapshot, alpha: f32) -> Option<[f32; 2]> {
    if !actor.is_gliding() {
        return None;
    }
    let ticks = actor.status.fall_fly_ticks as f32 + alpha;
    let ease = ticks * ticks / 100.0;
    let ease = if ease > 1.0 { 1.0 } else { ease.max(0.0) };
    Some([(-90.0 - actor.pitch) * ease, glide_turn(actor, alpha)])
}

/// Signed angle from the interpolated view to the horizontal motion, without normalising the
/// view's own horizontal length, as vanilla measures it.
fn glide_turn(actor: &ActorSnapshot, alpha: f32) -> f32 {
    let lerp = |previous: f32, current: f32| {
        let wrapped = (current - previous + 180.0) % 360.0;
        let wrapped = if wrapped < 0.0 {
            wrapped + 360.0
        } else {
            wrapped
        };
        (wrapped - 180.0) * alpha + previous
    };
    let yaw = lerp(actor.previous_pose.yaw, actor.yaw);
    let pitch = lerp(actor.previous_pose.pitch, actor.pitch);
    let yaw_angle = f64::from(-std::f32::consts::PI - yaw * DEGREES_TO_RADIANS);
    let horizontal = -(sim::minecraft_cos(f64::from(-(pitch * DEGREES_TO_RADIANS))) as f32);
    let view_z = sim::minecraft_cos(yaw_angle) as f32 * horizontal;
    let view_x = horizontal * sim::minecraft_sin(yaw_angle) as f32;
    let [delta_x, _, delta_z] = actor.native_velocity();
    let motion = delta_z * delta_z + delta_x * delta_x;
    if view_z * view_z + view_x * view_x <= 0.0 || motion <= 0.0 {
        return 0.0;
    }
    let cross = view_z * delta_x - view_x * delta_z;
    let side = if cross.abs() < GLIDE_TURN_DEAD_ZONE {
        0.0
    } else {
        cross.signum()
    };
    let turn = ((view_z * delta_z + view_x * delta_x) / motion.sqrt()).acos() * side;
    // Rounding can push the cosine just past one; that frame keeps the body unturned.
    if turn.is_finite() {
        turn * RADIANS_TO_DEGREES
    } else {
        0.0
    }
}

/// Pitches the rig about its feet, then turns it about its own up axis.
pub fn glide_tilted(mut rows: [[f32; 4]; 3], rotation: Option<[f32; 2]>) -> [[f32; 4]; 3] {
    let Some([pitch, yaw]) = rotation else {
        return rows;
    };
    let (sine, cosine) = (pitch * DEGREES_TO_RADIANS).sin_cos();
    for row in &mut rows {
        let (y, z) = (row[1], row[2]);
        row[1] = y * cosine + z * sine;
        row[2] = -y * sine + z * cosine;
    }
    let (sine, cosine) = (yaw * DEGREES_TO_RADIANS).sin_cos();
    for row in &mut rows {
        let (x, z) = (row[0], row[2]);
        row[0] = x * cosine - z * sine;
        row[2] = x * sine + z * cosine;
    }
    rows
}

fn interpolated_position(actor: &ActorSnapshot, partial_tick: f32) -> Option<[f32; 3]> {
    actor.interpolated_position(partial_tick)
}

pub(crate) fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

pub(crate) fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// Projectile bones carry absolute rotation; billboard bones carry the camera's rotation.
fn is_billboard(actor: &ActorSnapshot) -> bool {
    matches!(&actor.kind, ActorKind::Entity { identifier } if matches!(identifier.as_ref(),
        "minecraft:xp_bottle" | "minecraft:ender_pearl" | "minecraft:xp_orb"
        | "minecraft:dragon_fireball" | "minecraft:fireball" | "minecraft:snowball"
        | "minecraft:small_fireball" | "minecraft:splash_potion" | "minecraft:egg"
        | "minecraft:eye_of_ender_signal" | "minecraft:lingering_potion"))
}

fn quaternion_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

fn player_route_and_skin(
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    fallback: EntityRigFallback,
) -> (ActorRigRoute, Option<SkinRgba8>) {
    let ActorKind::Player { .. } = &actor.kind else {
        return (ActorRigRoute::NoDraw, None);
    };
    let route = match fallback {
        EntityRigFallback::Skip => ActorRigRoute::Compiled,
        EntityRigFallback::GeometryOnly => ActorRigRoute::StaticFallback,
        EntityRigFallback::Diagnostic => ActorRigRoute::Diagnostic,
    };
    let skin = profile
        .filter(|profile| profile.unique_id == actor.unique_id)
        .and_then(|profile| match &profile.skin {
            PlayerSkin::Standard(skin) => {
                render_model::prepare_actor_skin_cached(&ActorSkinPixels {
                    width: skin.width,
                    height: skin.height,
                    rgba8: skin.rgba8.clone(),
                })
            }
            PlayerSkin::Unavailable(_) => None,
        })
        .unwrap_or_else(default_actor_skin_rgba8);
    (route, Some(skin))
}

#[cfg(test)]
mod glide_tests;

#[cfg(test)]
mod selection_tests;

#[cfg(test)]
mod sampled_scale_tests;

#[cfg(test)]
mod death_tests {
    use super::*;

    const IDENTITY: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 6.0],
        [0.0, 0.0, 1.0, 7.0],
    ];

    #[test]
    fn alive_is_untouched_and_finished_death_lies_on_its_side() {
        assert_eq!(death_tilted(IDENTITY, None), IDENTITY);
        let lying = death_tilted(IDENTITY, Some(1.0));
        // The local up axis now points along world +/-X while the feet pivot stays fixed.
        assert!(lying[0][1].abs() > 0.999 && lying[1][1].abs() < 1e-6);
        assert_eq!([lying[0][3], lying[1][3], lying[2][3]], [5.0, 6.0, 7.0]);
    }
    #[test]
    fn review_render_local_placement_keeps_axis_scaling_and_death_tilt() {
        let old_feet = [1.0, 2.0, 3.0];
        let new_feet = [4.0, 64.0, 2.0];
        let axes = [2.0, 3.0, 4.0];
        let mut canonical = local_diagnostic_presentation(7, 0, 7, 5, old_feet, 30.0, 0.0).unwrap();
        canonical.head_over_body = 10.0;
        canonical.submission.world_from_actor = death_tilted(
            scaled_axes(canonical.submission.world_from_actor, axes),
            Some(0.4),
        );
        let diagnostic = local_diagnostic_presentation(7, 0, 7, 5, new_feet, 90.0, 0.0).unwrap();
        let local =
            local_actor_presentation_for_visibility(7, 7, Some(canonical), Some(diagnostic), 90.0)
                .unwrap();
        let expected = death_tilted(
            scaled_axes(rig_world_from_actor(new_feet, 80.0, 1.0), axes),
            Some(0.4),
        );
        for (actual, expected) in local
            .submission
            .world_from_actor
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
        }
    }
}

#[cfg(test)]
mod skin_dedupe_tests {
    use super::*;

    fn remote(runtime_id: u64, skin: SkinRgba8) -> ActorRigPresentation {
        let mut presentation =
            local_diagnostic_presentation(7, 0, runtime_id, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
                .expect("finite carrier converts");
        presentation.skin_rgba8 = Some(skin);
        presentation
    }

    /// Equal texels in distinct allocations share one layer; different texels never do.
    #[test]
    fn skins_share_a_layer_only_when_their_texels_match() {
        let texels = |value: u8| vec![value; 4096];
        let batch = select_actor_presentations(
            99,
            false,
            None,
            [1, 2, 1, 2, 3]
                .into_iter()
                .enumerate()
                .map(|(index, value)| remote(index as u64 + 1, texels(value).into())),
        );
        let layers = batch
            .submissions
            .iter()
            .map(|submission| submission.texture_layer)
            .collect::<Vec<_>>();
        assert_eq!(layers, [0, 1, 0, 1, 2]);
        assert_eq!(batch.skin_layers.len(), 3);
        for (layer, value) in batch.skin_layers.iter().zip([1, 2, 3]) {
            assert_eq!(&**layer, texels(value).as_slice());
        }
    }
}

#[cfg(test)]
mod layer_pass_tests {
    use super::*;

    /// Layer builders read the batch's own bodies: a copied body would hold a second reference
    /// to its bone allocations.
    #[test]
    fn attach_layers_never_copies_a_body() {
        let mut batch = select_actor_presentations(
            99,
            false,
            None,
            (1..=3).map(|runtime_id| {
                local_diagnostic_presentation(7, 0, runtime_id, 5, [0.0, 64.0, 0.0], 0.0, 0.0)
                    .expect("finite carrier converts")
            }),
        );
        let mut visited = Vec::new();
        attach_layers(&mut batch, &mut visited, |visited, body| {
            visited.push((
                body.input.identity.runtime_id,
                Arc::strong_count(&body.input.previous_bones),
                Arc::strong_count(&body.input.current_bones),
            ));
            &mut []
        });
        assert_eq!(visited, [(1, 1, 1), (2, 1, 1), (3, 1, 1)]);
    }
}

#[cfg(test)]
#[path = "actors/native_skin_tests.rs"]
mod native_skin_tests;
