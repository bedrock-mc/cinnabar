mod commit;
mod hand;
pub use commit::{PreparedActorPublication, publish_actor_render_frame};
#[cfg(test)]
use hand::hand_camera_from_rig;
use hand::{HandSource, hand_motion_matrix, hand_progress, publish_hand_rig};

use std::sync::Arc;

use bevy::prelude::DetectChanges;
use bevy::{
    ecs::system::SystemParam,
    math::Mat4,
    prelude::{Local, Projection, Res, ResMut, Resource, Time, Vec3},
    time::Real,
};
use client_world::{LocalPlayerFeed, WorldStream};
use render::{
    ActorCullView, ActorMainWitness, ActorRenderScene, ActorRigFrameBuilder, ActorRigSubmission,
    HandItemAtlas, HandRigLight, HandRigScene, MAX_ACTOR_RENDER_DISTANCE_BLOCKS, RuntimeStage,
    RuntimeStageProfiler,
};

use crate::camera::{CameraSettingsAuthority, FlyCamera};
use crate::local_player::{LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalViewPose};
use crate::{
    actor_clock::{ActorFrameClock, authoritative_local_actor_eye, publish_local_actor_visibility},
    dropped_items::DroppedItemPublisher,
    presentation::actors::{
        ActorRigPresentation, local_actor_presentation_for_visibility,
        local_diagnostic_presentation, rig_world_from_actor, select_actor_presentations_for_view,
    },
    presentation::equipment::{
        EquipmentRuntime, FirstPersonArms, FirstPersonHand, FirstPersonItem, StagedSessionIcons,
        remote_input,
    },
};
use bevy::prelude::{Query, Transform, With};

/// The exact stream and immutable artwork snapshots borrowed for actor publication.
pub struct ActorWorld<'a> {
    pub stream: Option<&'a mut WorldStream>,
    pub entity_assets: Option<&'a assets::RuntimeEntityAssets>,
    pub pack_entities: Option<Arc<assets::SessionEntityPack>>,
    pub session_items: Option<Arc<crate::session_assets::SessionItems>>,
    pub prepared_actor_artwork:
        &'a mut Option<Arc<crate::prepared_actor_artwork::PreparedActorArtwork>>,
}

/// Gameplay and screen observations captured at the actor preparation boundary.
pub struct ActorFrameInput {
    pub local_feed: Option<LocalPlayerFeed>,
    pub predicted_eye: Option<[f32; 3]>,
    pub predicted_feet: Option<[f32; 3]>,
    pub local_equipment: crate::presentation::equipment::ActorEquipmentInput,
    pub swing_started: Option<i32>,
    pub renders_game: bool,
    pub hide_hand: bool,
}

/// Presentation-owned resources that determine the avatar's current view.
#[derive(SystemParam)]
pub struct ActorPresentationState<'w, 's> {
    avatar: Res<'w, LocalAvatarPresentation>,
    local_visibility: ResMut<'w, LocalAvatarVisibilityCarrier>,
    settings: Res<'w, CameraSettingsAuthority>,
    view: Res<'w, LocalViewPose>,
    camera: Query<'w, 's, (&'static Transform, &'static Projection), With<FlyCamera>>,
}

/// The frame fraction the actor rigs interpolate at, for overlays anchored to actors.
#[derive(Resource, Default)]
pub struct ActorFramePartialTick(pub f32);

/// The local player's own first-person rig, built as a single instance placed in camera space.
#[derive(Resource)]
pub struct HandRigBuilder(pub ActorRigFrameBuilder);

impl HandRigBuilder {
    pub fn from_runtime_assets(assets: &assets::RuntimeEntityAssets) -> anyhow::Result<Self> {
        ActorRigFrameBuilder::from_runtime_assets(assets)
            .map(Self)
            .map_err(|error| {
                anyhow::anyhow!("prepare validated first-person hand rig geometry: {error:?}")
            })
    }
}

/// Vertical FOV of the first-person pass; underwater and death-camera narrowing are not modelled.
pub const HAND_FOV_DEGREES: f32 = 70.0;

/// Rebuilds session artwork and item routes, or restores startup artwork after disconnect.
fn apply_session_pack(
    scene: &mut ActorRenderScene,
    mut pages: render::ActorArtworkPages,
    pack: Option<&crate::session_assets::SessionEntityPack>,
    session_icons: Option<StagedSessionIcons>,
    geometry_ready: &mut SessionGeometryReady,
    equipment: Option<&mut EquipmentRuntime>,
    profiler: Option<&RuntimeStageProfiler>,
) -> (
    Option<render::ActorArtworkPages>,
    Option<StagedSessionIcons>,
    Vec<Option<render::ActorArtworkLocation>>,
) {
    let equipment_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorEquipmentSetup));
    let mut layer = None;
    let mut geometries = Vec::new();
    if let Some(pack) = pack
        && let Some(catalog) = &pack.equipment
    {
        let (extended, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
        pages = extended;
        if !geometry_ready.equipment {
            geometries = EquipmentRuntime::pack_geometries(&pack.assets, catalog);
        }
        layer = Some((Arc::clone(&pack.assets), Arc::clone(catalog), locations));
    }
    if let Some(equipment) = equipment {
        equipment.set_pack_layer(layer);
    }
    let mut icon_locations = Vec::new();
    if let Some(icons) = &session_icons {
        let (extended, locations) = pages.with_equipment_rasters(icons.rasters());
        pages = extended;
        icon_locations = locations;
    }
    drop(equipment_timer);
    if !geometry_ready.entities || !geometry_ready.equipment {
        apply_session_geometry(scene, pack, geometries, geometry_ready, profiler);
    }
    scene.configure_artwork(pages.clone());
    let effective = (pack.is_some() || session_icons.is_some()).then_some(pages);
    (effective, session_icons, icon_locations)
}

/// Each accepted namespace can survive item/artwork refreshes independently.
#[derive(Default)]
struct SessionGeometryReady {
    entities: bool,
    equipment: bool,
}

/// Retries rejected namespaces while retaining geometry that already published successfully.
fn apply_session_geometry(
    scene: &mut ActorRenderScene,
    pack: Option<&crate::session_assets::SessionEntityPack>,
    geometries: Vec<render::ActorRigGeometry>,
    ready: &mut SessionGeometryReady,
    profiler: Option<&RuntimeStageProfiler>,
) {
    let geometry_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorGeometrySetup));
    let assets = pack.map(|pack| &*pack.assets);
    let (entities, equipment) = match (ready.entities, ready.equipment) {
        (false, false) => scene.replace_session_pack_geometries(assets, geometries),
        (false, true) => (scene.replace_pack_entities(assets), Ok(())),
        (true, false) => (Ok(()), scene.replace_pack_equipment(geometries)),
        (true, true) => return,
    };
    ready.entities = entities.is_ok();
    ready.equipment = equipment.is_ok();
    if let Err(error) = entities {
        bevy::log::warn!(?error, "server pack entity geometry was not applied");
    }
    if let Err(error) = equipment {
        bevy::log::warn!(?error, "server pack equipment geometry was not applied");
    }
    drop(geometry_timer);
}

#[derive(SystemParam)]
pub struct ActorFramePublication<'w, 's> {
    time: Res<'w, Time<Real>>,
    scene: ResMut<'w, ActorRenderScene>,
    prepared: ResMut<'w, PreparedActorPublication>,
    published_session: Local<'s, Option<u64>>,
    published_pack: Local<'s, Option<Arc<crate::session_assets::SessionEntityPack>>>,
    /// Partial rejection must still be retried on the next resource refresh.
    pack_geometry_ready: Local<'s, SessionGeometryReady>,
    published_items: Local<'s, Option<Arc<crate::session_assets::SessionItems>>>,
    actor_clock: Local<'s, ActorFrameClock>,
    presentation: ActorPresentationState<'w, 's>,
    artwork: Res<'w, render::ActorArtworkPages>,
    /// The startup artwork plus the session's server-pack pages; `None` in a vanilla session.
    session_artwork: Local<'s, Option<render::ActorArtworkPages>>,
    cape_state: Local<'s, crate::presentation::cape::CapeState>,
    skin_rigs: Local<'s, crate::presentation::skin_rig::SkinRigCache>,
    skin_layers: Local<'s, crate::presentation::skin_layers::SkinLayerCache>,
    poses: Local<'s, crate::presentation::actors::PoseConversions>,
    layer_poses: Local<'s, crate::presentation::entity_layers::LayerPoseCache>,
    hand_builder: ResMut<'w, HandRigBuilder>,
    hand_scene: ResMut<'w, HandRigScene>,
    hand_revision: Local<'s, u64>,
    hand_motion: Option<Res<'w, crate::camera::FirstPersonHandMotion>>,
    equipment: Option<ResMut<'w, EquipmentRuntime>>,
    dropped_items: DroppedItemPublisher<'w, 's>,
    profiler: Option<Res<'w, render::RuntimeStageProfiler>>,
    partial_tick: ResMut<'w, ActorFramePartialTick>,
}

/// Captures this frame's actor inputs before outbound interactions can change them.
pub fn prepare_actor_render_frame(
    mut client_world: ActorWorld<'_>,
    input: ActorFrameInput,
    sample_world: impl FnOnce(&mut WorldStream),
    hand_use: impl Fn(
        &WorldStream,
        f32,
    ) -> (
        Option<u32>,
        Option<client_world::AttachableAnimationInput<'static>>,
    ),
    hides_box: impl Fn(&WorldStream, [f32; 3], [f32; 3]) -> bool,
    params: ActorFramePublication,
) {
    let ActorFramePublication {
        time,
        mut scene,
        mut prepared,
        mut published_session,
        mut published_pack,
        mut pack_geometry_ready,
        mut published_items,
        mut actor_clock,
        presentation,
        artwork,
        mut session_artwork,
        mut cape_state,
        mut skin_rigs,
        mut skin_layers,
        mut poses,
        mut layer_poses,
        mut hand_builder,
        mut hand_scene,
        mut hand_revision,
        hand_motion,
        mut equipment,
        mut dropped_items,
        profiler,
        mut partial_tick,
    } = params;
    let ActorPresentationState {
        avatar,
        mut local_visibility,
        settings,
        view,
        camera,
    } = presentation;
    let session_id = client_world
        .stream
        .as_ref()
        .map(|stream| stream.actor_session_id());
    let new_session = *published_session != session_id;
    if new_session {
        if session_id.is_none() {
            *client_world.prepared_actor_artwork = None;
        }
        scene.reset();
        actor_clock.reset();
        *skin_layers = Default::default();
        *published_session = session_id;
        if let Some(stream) = client_world.stream.as_mut() {
            stream.set_actor_seat_defaults(crate::seat_defaults::seat_defaults());
        }
    }
    let pack = session_id.and_then(|_| client_world.pack_entities.clone());
    let items = session_id.and_then(|_| client_world.session_items.clone());
    let same = |left: Option<*const ()>, right: Option<*const ()>| left == right;
    let pack_changed = !same(
        published_pack.as_ref().map(|pack| Arc::as_ptr(pack).cast()),
        pack.as_ref().map(|pack| Arc::as_ptr(pack).cast()),
    );
    let items_changed = !same(
        published_items
            .as_ref()
            .map(|items| Arc::as_ptr(items).cast()),
        items.as_ref().map(|items| Arc::as_ptr(items).cast()),
    );
    if new_session || pack_changed || items_changed || artwork.is_changed() {
        let _setup = profiler
            .as_deref()
            .map(|profiler| profiler.time(render::RuntimeStage::ActorSessionSetup));
        *published_pack = pack.clone();
        *published_items = items.clone();
        let staged = StagedSessionIcons::stage(items.as_deref());
        if new_session || pack_changed {
            *pack_geometry_ready = SessionGeometryReady::default();
        }
        let artwork_timer = profiler
            .as_deref()
            .map(|profiler| profiler.time(RuntimeStage::ActorArtworkSetup));
        let pages = crate::prepared_actor_artwork::session_pages(
            &artwork,
            pack.as_ref(),
            client_world.prepared_actor_artwork.as_deref(),
        );
        drop(artwork_timer);
        // Always republished: presentation selects from these pages, the scene validates them.
        let (effective, staged, locations) = apply_session_pack(
            &mut scene,
            pages,
            pack.as_deref(),
            staged,
            &mut pack_geometry_ready,
            equipment.as_deref_mut(),
            profiler.as_deref(),
        );
        *session_artwork = effective;
        if let Some(equipment) = equipment.as_deref_mut() {
            equipment.set_session_items(items.as_deref(), staged, locations);
        }
    }
    let artwork = session_artwork.as_ref().unwrap_or(&artwork);
    let step = actor_clock.advance(time.delta());
    partial_tick.0 = step.partial_tick;
    skin_rigs.begin_frame();
    poses.begin_frame();
    if let Some(equipment) = equipment.as_deref_mut() {
        equipment.begin_frame();
    }
    layer_poses.begin_frame();
    let first_person = settings.perspective() == semantic_input::PerspectiveMode::FirstPerson;
    let mut local_feed = input.local_feed;
    if let Some(feed) = local_feed.as_mut() {
        feed.first_person = first_person;
        feed.main_hand = input
            .local_equipment
            .main
            .as_ref()
            .map(|item| item.identifier.clone());
        feed.off_hand = input
            .local_equipment
            .off
            .as_ref()
            .map(|item| item.identifier.clone());
    }
    if let Some(stream) = client_world.stream.as_mut() {
        if let Some(equipment) = equipment.as_deref() {
            stream.set_item_use_durations(equipment.item_use_durations());
        }
        // Feed the client-authored local pose before the tick advance and rig read so the
        // local body/hand are driven by the shared rig, not the static fallback.
        if let Some(feed) = &local_feed {
            stream.sync_local_player_pose(feed);
        }
        // Attacks, mining and use swing the local arm at once; the server echoes no swing.
        if let Some(ticks) = input.swing_started {
            stream.start_local_player_swing(ticks);
        }
        let (yaw, pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
        stream.set_actor_camera_rotation([
            -pitch.to_degrees(),
            (180.0 - yaw.to_degrees()).rem_euclid(360.0),
        ]);
        if let Ok((transform, _)) = camera.single() {
            stream.set_actor_camera_position(transform.translation.to_array());
        }
        stream.set_actor_animation_view(
            camera
                .single()
                .ok()
                .and_then(|(transform, projection)| animation_view(transform, projection)),
        );
        let _animation = profiler
            .as_deref()
            .map(|profiler| profiler.time(render::RuntimeStage::ActorAnimation));
        // Fluid and bed state is tick state; a frame without a tick would resample the same.
        if step.ticks > 0 {
            sample_world(stream);
        }
        stream.advance_actor_interpolation_frame(step.ticks);
    }
    let authoritative_subject_eye = authoritative_local_actor_eye(
        input.predicted_eye,
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.resolved_server_position().position),
    );
    let authoritative_subject_feet = input
        .predicted_feet
        .map(bevy::prelude::Vec3::from_array)
        .or_else(|| {
            client_world.stream.as_ref().map(|stream| {
                let mut feet =
                    bevy::prelude::Vec3::from_array(stream.resolved_server_position().position);
                feet.y -= protocol::PLAYER_NETWORK_OFFSET;
                feet
            })
        });
    publish_local_actor_visibility(
        &avatar,
        settings.perspective(),
        authoritative_subject_eye,
        authoritative_subject_feet,
        view.rotation(),
        &mut local_visibility,
    );
    // Vanilla projects the hand with its own fixed FOV, ignoring the FOV option and modifiers.
    let hand_camera_fov = camera
        .single()
        .ok()
        .filter(|(_, projection)| matches!(projection, Projection::Perspective(_)))
        .map(|_| HAND_FOV_DEGREES.to_radians());
    let preparation = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::ActorPreparation));
    let cull_view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    // Registered together below: each registration rebuilds and re-uploads the whole catalog.
    let mut new_geometries = Vec::new();
    let (local_runtime_id, actor_session_id, dimension, remotes, canonical_local, unrigged_actors) =
        client_world
            .stream
            .as_ref()
            .map(|stream| {
                let local_runtime_id = stream.local_player_runtime_id();
                let mut remotes = Vec::with_capacity(stream.actor_count());
                let mut canonical_local = None;
                let mut rigged = 0;
                for rig in stream.actor_rigs() {
                    rigged += 1;
                    let Some(actor) = stream.actor(rig.actor.runtime_id) else {
                        continue;
                    };
                    // Culled before any per-actor work; the local rig also drives the hand.
                    if rig.actor.runtime_id != local_runtime_id
                        && !crate::presentation::actors::rig_may_be_visible(
                            &rig,
                            actor,
                            step.partial_tick,
                            cull_view,
                            |low, high| hides_box(stream, low, high),
                        )
                    {
                        continue;
                    }
                    let profile = stream.actor_player_profile(rig.actor.runtime_id);
                    let presentation = if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
                        crate::presentation::actors::actor_rig_presentation_cached(
                            &rig,
                            actor,
                            profile,
                            step.partial_tick,
                            &mut poses,
                        )
                        .map(|mut presentation| {
                            if let Some(geometry) = rig.skin_geometry {
                                // The pose drives the skin's own bones, so only its model fits.
                                match skin_rigs.rig(geometry, |built| {
                                    if let Some(equipment) = equipment.as_deref_mut() {
                                        equipment.register_skin_rig(
                                            built.id,
                                            geometry
                                                .bones
                                                .iter()
                                                .map(|bone| bone.name.clone())
                                                .collect(),
                                        );
                                    }
                                    new_geometries.push(built);
                                }) {
                                    Some(id) => presentation.submission.input.rig = id,
                                    None => {
                                        presentation.submission.route =
                                            render::ActorRigRoute::NoDraw;
                                    }
                                }
                            }
                            presentation
                        })
                    } else {
                        crate::presentation::actors::entity_rig_presentation_cached(
                            &rig,
                            actor,
                            artwork,
                            step.partial_tick,
                            Some(&mut poses),
                        )
                    };
                    let Some(presentation) = presentation else {
                        continue;
                    };
                    if rig.actor.runtime_id == local_runtime_id {
                        canonical_local = Some(presentation);
                    } else {
                        remotes.push(presentation);
                    }
                }
                (
                    local_runtime_id,
                    stream.actor_session_id(),
                    stream.current_dimension(),
                    remotes,
                    canonical_local,
                    stream.actor_count().saturating_sub(rigged),
                )
            })
            .unwrap_or((0, 0, 0, Vec::new(), None, 0));
    // First person draws the player's own rig near the camera: the visible arms with every other
    // bone hidden, and a drawable held item in its own first-person frame. Anything not covered
    // (an undrawable item) leaves the CPU viewmodel in charge.
    let hand_source: Option<HandSource> = if first_person && input.renders_game {
        canonical_local.clone().and_then(|presentation| {
            let stream = client_world.stream.as_ref()?;
            let equipment = equipment.as_deref_mut()?;
            let equipment_input = local_equipment(stream, local_runtime_id, &input.local_equipment);
            let (consume_ticks, item_animation) = hand_use(stream, step.partial_tick);
            let hand = stream.actor_rig(local_runtime_id).map_or(
                FirstPersonHand {
                    swing: 0.0,
                    equip: 1.0,
                    consume: None,
                },
                |rig| hand_progress(rig.hand, consume_ticks, step.partial_tick),
            );
            let items = std::array::from_fn(|index| {
                let item = [equipment_input.main.as_ref(), equipment_input.off.as_ref()][index]?;
                let rig = stream.actor_rig(local_runtime_id)?;
                let modern = item_animation.and_then(|mut render_input| {
                    render_input.frame_alpha = step.partial_tick;
                    let render_input =
                        equipment_input.attachable_input(render_input.for_hand(index == 1));
                    equipment.first_person_attachable(
                        &presentation.submission,
                        item,
                        stream.actor(local_runtime_id)?,
                        &rig,
                        render_input,
                    )
                });
                let layer = modern.or_else(|| {
                    if index == 0 {
                        equipment.first_person_item(&presentation.submission, item, hand)
                    } else {
                        equipment.first_person_offhand(&presentation.submission, item)
                    }
                })?;
                let page = usize::from(layer.presentation.location.page()).checked_sub(1)?;
                let page = artwork.pages().get(page)?;
                let (width, height) = page.dimensions();
                let atlas = HandItemAtlas {
                    width,
                    height,
                    layers: page.layers(),
                    rgba8: page.shared_pixels(),
                };
                Some((layer, atlas))
            });
            // Provisional: vanilla draws every held item; an undrawable one shows the bare arm.
            let arms = FirstPersonArms::for_hands(
                equipment_input
                    .main
                    .as_ref()
                    .map(|item| item.identifier.as_ref()),
                equipment_input
                    .off
                    .as_ref()
                    .map(|item| item.identifier.as_ref()),
            )
            .with_undrawn_main(items[0].is_some());
            let body = equipment.mask_first_person(&presentation.submission, arms);
            (body.is_some() || items.iter().any(Option::is_some)).then_some(HandSource {
                presentation,
                body,
                items,
                motion: hand_motion
                    .as_deref()
                    .map_or(Mat4::IDENTITY, hand_motion_matrix),
            })
        })
    } else {
        None
    };
    let visibility_snapshot = local_visibility.snapshot().copied();
    let (local_visible, local) = visibility_snapshot.map_or((false, None), |visibility| {
        if visibility.runtime_id() != local_runtime_id {
            return (false, None);
        }
        // Native camera and body transforms sample the same actor origin at render alpha
        // (26.30 VanillaOffsetSystem::getCameraPosition / Actor::getActorToWorldTransform).
        // Keep the rig's body yaw and animation, but use the physics render sample rather
        // than interpolating the local position again on the remote actor clock.
        let local = canonical_local
            .map(|mut local| {
                place_local_actor_at_render_feet(&mut local, visibility.feet());
                local
            })
            .or_else(|| {
                let (yaw, pitch, _) = visibility.rotation().to_euler(bevy::math::EulerRot::YXZ);
                let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
                let pitch_degrees = -pitch.to_degrees();
                let position = visibility.feet();
                let diagnostic = local_diagnostic_presentation(
                    actor_session_id,
                    dimension,
                    visibility.runtime_id(),
                    visibility.pose_generation(),
                    position.to_array(),
                    yaw_degrees,
                    pitch_degrees,
                );
                local_actor_presentation_for_visibility(
                    local_runtime_id,
                    visibility.runtime_id(),
                    None,
                    diagnostic,
                    yaw_degrees,
                )
            });
        (visibility.visible(), local)
    });
    // The visibility override rebuilds the local transform, so re-apply the death tip-over.
    let local_death = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.actor(local_runtime_id))
        .and_then(|actor| actor.status.death_progress(step.partial_tick));
    let local = local.map(|mut local| {
        local.submission.world_from_actor = crate::presentation::actors::death_tilted(
            local.submission.world_from_actor,
            local_death,
        );
        local
    });
    let camera_position = cull_view
        .as_ref()
        .map(|view| view.camera_position.to_array());
    let mut batch = select_actor_presentations_for_view(
        local_runtime_id,
        local_visible,
        local,
        remotes,
        cull_view,
    );
    if let Some(stream) = client_world.stream.as_ref() {
        crate::presentation::actors::light_bodies(&mut batch, stream);
    }
    if let (Some(stream), Some(cape)) = (
        client_world.stream.as_ref(),
        cape_state.rig(client_world.entity_assets),
    ) {
        if !scene.contains_geometry(cape.id) {
            let _ = scene.insert_geometry(cape.geometry.clone());
        }
        crate::presentation::cape::apply_capes(
            &mut batch,
            cape,
            |runtime_id| stream.actor_rig(runtime_id),
            |runtime_id| stream.actor_player_profile(runtime_id),
        );
    }
    let selected_count = batch.submissions.len();
    if let (Some(equipment), Some(stream)) =
        (equipment.as_deref_mut(), client_world.stream.as_ref())
    {
        // Equipment rides each selected body's pose, so culled bodies never build layers.
        let bodies = batch.submissions.clone();
        for body in &bodies {
            let runtime_id = body.input.identity.runtime_id;
            let input = if runtime_id == local_runtime_id {
                local_equipment(stream, runtime_id, &input.local_equipment)
            } else {
                remote_input(stream, runtime_id)
            };
            for layer in equipment.layers_for(body, &input) {
                batch
                    .artwork
                    .insert(layer.submission.input.identity, layer.location);
                batch.submissions.push(layer.submission);
            }
        }
    }
    // After equipment, which rides the rig's own model even when a controller draws another.
    if let Some(stream) = client_world.stream.as_ref() {
        crate::presentation::entity_layers::apply_render_layers_cached(
            &mut batch,
            |runtime_id| stream.actor_rig(runtime_id),
            artwork,
            &mut layer_poses,
        );
    }
    if let Some(stream) = client_world.stream.as_ref()
        && let Some(pages) = skin_layers.apply(
            &mut batch,
            artwork,
            |runtime_id| stream.actor_rig(runtime_id),
            &mut skin_rigs,
            |geometry| new_geometries.push(geometry),
        )
    {
        scene.configure_artwork(pages);
    }
    // Hiding skin layers keeps armor and held items visible.
    if let Some(stream) = client_world.stream.as_ref() {
        for submission in &mut batch.submissions {
            let identity = submission.input.identity;
            if (identity.layer == render::ACTOR_LAYER_BODY
                || identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE
                || crate::presentation::skin_layers::is_skin_layer(identity.layer)
                || identity.layer >= crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE)
                && stream
                    .actor(identity.runtime_id)
                    .is_some_and(|actor| actor.is_invisible())
            {
                submission.route = render::ActorRigRoute::NoDraw;
            }
        }
    }
    if let Some(equipment) = equipment.as_deref_mut() {
        new_geometries.extend(equipment.take_pending_geometries());
    }
    drop(preparation);
    {
        let _rig_build = profiler
            .as_deref()
            .map(|profiler| profiler.time(render::RuntimeStage::ActorRigBuild));
        register_geometries(&mut hand_builder.0, &mut scene, new_geometries);
    }
    prepared.0 = Some(commit::PendingActorPublication {
        batch,
        partial_tick: step.partial_tick,
        witness: ActorMainWitness {
            local_snapshot: visibility_snapshot.is_some(),
            local_visible,
            expected_runtime_id: local_runtime_id,
            visibility_runtime_id: visibility_snapshot.map_or(0, |snapshot| snapshot.runtime_id()),
            selected_count,
            local_route: None,
            frame_instances: 0,
            frame_manifest: 0,
            skin_bytes: 0,
            rejects: Default::default(),
            unrigged_actors,
        },
    });
    let hand_light = client_world.stream.as_ref().map_or(
        HandRigLight {
            block_level: 0,
            sky_level: 0,
            daylight: 1.0,
            pad: 0,
        },
        |stream| {
            let (block, sky) = authoritative_subject_eye
                .map_or((0, 0), |eye| stream.light_level_at(eye.to_array()));
            HandRigLight {
                block_level: u32::from(block),
                sky_level: u32::from(sky),
                // Reserved legacy field; the hand samples the shared world lightmap.
                daylight: 1.0,
                pad: 0,
            }
        },
    );
    dropped_items.publish(
        client_world.stream.as_deref(),
        camera_position.map(|position| {
            let (yaw, _, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
            (position, (180.0 - yaw.to_degrees()).rem_euclid(360.0))
        }),
        step.partial_tick,
    );
    publish_hand_rig(
        &mut hand_builder.0,
        &mut hand_scene,
        &mut hand_revision,
        hand_source.filter(|_| !input.hide_hand),
        hand_camera_fov,
        hand_light,
        step.partial_tick,
    );
}

/// Reuses inventory facts while reading pose flags after this frame's local pose synchronization.
fn local_equipment(
    stream: &WorldStream,
    runtime_id: u64,
    inventory: &crate::presentation::equipment::ActorEquipmentInput,
) -> crate::presentation::equipment::ActorEquipmentInput {
    let actor = stream.actor(runtime_id);
    crate::presentation::equipment::ActorEquipmentInput {
        sneaking: actor.is_some_and(|actor| actor.is_sneaking()),
        sleeping: actor.is_some_and(|actor| actor.is_sleeping()),
        ..inventory.clone()
    }
}

/// Rigs this far outside the view on every side still animate, so only a turn faster than this
/// in one tick shows a rig its held pose for that tick.
const ANIMATION_GUARD_DEGREES: f32 = 30.0;

/// The camera's frustum widened by the guard band, with the render distances.
fn animation_view(
    transform: &bevy::prelude::Transform,
    projection: &Projection,
) -> Option<client_world::ActorAnimationView> {
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    let (guard, limit) = (ANIMATION_GUARD_DEGREES.to_radians(), 85f32.to_radians());
    let half_vertical = perspective.fov * 0.5;
    let half_horizontal = (half_vertical.tan() * perspective.aspect_ratio).atan();
    let (half_vertical, half_horizontal) = (
        (half_vertical + guard).min(limit),
        (half_horizontal + guard).min(limit),
    );
    let clip = Mat4::perspective_infinite_reverse_rh(
        half_vertical * 2.0,
        half_horizontal.tan() / half_vertical.tan(),
        perspective.near,
    ) * transform.to_matrix().inverse();
    let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip.row(row));
    let planes = [w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane.to_array());
    planes
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then(|| client_world::ActorAnimationView {
            planes,
            camera: transform.translation.to_array(),
            player_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
            entity_radius: render::ACTOR_CANDIDATE_RADIUS_BLOCKS,
        })
}

/// Registers new skin models and item meshes in both rig catalogs with one rebuild each; if a
/// batch is refused, each is tried alone so a rejected mesh only leaves that model undrawn.
fn register_geometries(
    hand: &mut ActorRigFrameBuilder,
    scene: &mut ActorRenderScene,
    geometries: Vec<render::ActorRigGeometry>,
) {
    if geometries.is_empty() {
        return;
    }
    if hand.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries.iter().cloned() {
            let _ = hand.insert_geometry(geometry);
        }
    }
    if scene.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries {
            let _ = scene.insert_geometry(geometry);
        }
    }
}

/// Positions the local rig at the same render sample as its camera subject.
fn place_local_actor_at_render_feet(presentation: &mut ActorRigPresentation, feet: Vec3) {
    for (row, coordinate) in presentation
        .submission
        .world_from_actor
        .iter_mut()
        .zip(feet.to_array())
    {
        row[3] = coordinate;
    }
}

#[cfg(test)]
mod tests;
