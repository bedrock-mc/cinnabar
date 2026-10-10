use super::*;
use bevy::math::{Mat4, Vec3};
use render::{ActorArtworkPages, EquipmentRaster};
use render_model::STANDARD_SKIN_BYTES;

fn presentation(runtime_id: u64, position: [f32; 3]) -> ActorRigPresentation {
    local_diagnostic_presentation(7, 0, runtime_id, 5, position, 0.0, 0.0).unwrap()
}

fn artwork() -> ActorArtworkLocation {
    ActorArtworkPages::default()
        .with_equipment_rasters(&[EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        }])
        .1[0]
        .unwrap()
}

fn pack_actor(runtime_id: u64, position: [f32; 3]) -> ActorRigPresentation {
    let mut actor = presentation(runtime_id, position);
    actor.artwork = Some(artwork());
    actor.skin_rgba8 = None;
    actor.submission.route = ActorRigRoute::Compiled;
    actor
}

fn view(direction: Vec3) -> ActorCullView {
    let camera = Vec3::new(0.0, 65.0, 0.0);
    ActorCullView {
        camera_position: camera,
        clip_from_world: Mat4::perspective_infinite_reverse_rh(
            90_f32.to_radians(),
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * Mat4::look_to_rh(camera, direction, Vec3::Y),
        max_distance: 100.0,
    }
}

#[test]
fn camera_turn_admitting_map_actors_does_not_evict_a_visible_late_actor() {
    let entity_id = MAX_RENDERED_PLAYERS as u64 + 2;
    let remotes = || {
        (1..=MAX_RENDERED_PLAYERS as u64)
            .map(|id| pack_actor(id, [0.0, 64.0, -5.0]))
            .chain([presentation(entity_id - 1, [4.0, 64.0, -4.0])])
            .chain([pack_actor(entity_id, [4.0, 64.0, -4.0])])
    };
    let narrow = select_actor_presentations_for_view(
        0,
        false,
        None,
        remotes(),
        Some(view(Vec3::new(1.0, 0.0, -0.3))),
    );
    assert_eq!(narrow.submissions.len(), 2);
    let expanded = select_actor_presentations_for_view(
        0,
        false,
        None,
        remotes(),
        Some(view(Vec3::new(0.3, 0.0, -1.0))),
    );
    assert!(expanded.submissions.iter().any(|draw| {
        draw.input.identity.runtime_id == entity_id && draw.route != ActorRigRoute::NoDraw
    }));
    assert_eq!(expanded.submissions.len(), MAX_RENDERED_PLAYERS + 2);
    assert_eq!(expanded.skin_layers.len(), 1);
}

#[test]
fn shared_player_skins_do_not_consume_one_residency_slot_per_actor() {
    let count = MAX_RENDERED_PLAYERS + 1;
    let batch = select_actor_presentations(
        0,
        false,
        None,
        (1..=count).map(|id| presentation(id as u64, [0.0, 64.0, -5.0])),
    );
    assert_eq!(batch.submissions.len(), count);
    assert_eq!(batch.skin_layers.len(), 1);
}

#[test]
fn skin_residency_overflow_keeps_pack_artwork_and_already_admitted_skins() {
    let skin = |value: usize| -> SkinRgba8 {
        let mut texels = vec![255; STANDARD_SKIN_BYTES];
        texels[..size_of::<usize>()].copy_from_slice(&value.to_le_bytes());
        texels.into()
    };
    let base = (1..=MAX_RENDERED_PLAYERS + 1).map(|id| {
        let mut actor = presentation(id as u64, [0.0, 64.0, -5.0]);
        actor.skin_rgba8 = Some(skin(id));
        actor
    });
    let reused_id = MAX_RENDERED_PLAYERS as u64 + 2;
    let mut reused = presentation(reused_id, [0.0, 64.0, -5.0]);
    reused.skin_rgba8 = Some(skin(1));
    let pack_id = reused_id + 1;
    let batch = select_actor_presentations(
        0,
        false,
        None,
        base.chain([reused, pack_actor(pack_id, [0.0, 64.0, -5.0])]),
    );
    assert_eq!(batch.skin_layers.len(), MAX_RENDERED_PLAYERS);
    let route = |id| {
        batch
            .submissions
            .iter()
            .find(|draw| draw.input.identity.runtime_id == id)
            .unwrap()
            .route
    };
    assert_eq!(
        route(MAX_RENDERED_PLAYERS as u64 + 1),
        ActorRigRoute::NoDraw
    );
    assert_ne!(route(reused_id), ActorRigRoute::NoDraw);
    assert_eq!(route(pack_id), ActorRigRoute::Compiled);
}

#[test]
fn nondrawing_actors_do_not_reserve_skin_residency() {
    let hidden = (1..=MAX_RENDERED_PLAYERS).map(|id| {
        let mut actor = presentation(id as u64, [0.0, 64.0, -5.0]);
        actor.submission.route = ActorRigRoute::NoDraw;
        actor.skin_rgba8 = Some(vec![id as u8; STANDARD_SKIN_BYTES].into());
        actor
    });
    let visible_id = MAX_RENDERED_PLAYERS as u64 + 1;
    let batch = select_actor_presentations(
        0,
        false,
        None,
        hidden.chain([presentation(visible_id, [0.0, 64.0, -5.0])]),
    );
    assert_eq!(batch.skin_layers.len(), 1);
    assert_ne!(
        batch.submissions.last().unwrap().route,
        ActorRigRoute::NoDraw
    );
}

#[test]
fn offscreen_shadow_casters_cannot_evict_visible_actors_during_scene_build() {
    let main = view(Vec3::NEG_Z);
    let radius = main.max_distance;
    let shadow = ActorCullView {
        clip_from_world: Mat4::orthographic_rh(-radius, radius, -radius, radius, -radius, radius),
        ..main
    };
    let visible_id = render::MAX_ACTOR_RENDER_INSTANCES as u64 + 1;
    let remotes = (1..visible_id)
        .map(|id| presentation(id, [0.0, 64.0, 5.0]))
        .chain([presentation(visible_id, [0.0, 64.0, -5.0])]);
    let batch = select_actor_presentations_for_shadow_view(
        0,
        false,
        None,
        remotes,
        Some(main),
        Some(shadow),
    );
    let mut scene = ActorRenderScene::default();
    let frame = update_actor_rig_scene(&mut scene, 0.5, batch);
    assert!(
        frame
            .rig
            .manifest
            .iter()
            .any(|entry| entry.identity.runtime_id == visible_id),
        "offscreen casters must not consume a visible actor's instance capacity"
    );
}
