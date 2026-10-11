use std::sync::Arc;

use protocol::ActorKind;
use render_api::SkinGeometrySource;

use crate::actor_store::{ActorStore, MAX_APPEARANCES_PUBLISHED_PER_FRAME};
use actor_animation::test_support::{self, SKIN_MODEL, skin_source};

/// Lists and spawns one independently allocated, original custom-model appearance.
fn profile_store(byte: u8) -> ActorStore {
    let mut store = player_store();
    add_player(&mut store, 1, profile_skin(byte));
    store
}

/// A store whose standard catalog models have finished preparing.
fn player_store() -> ActorStore {
    let mut store = ActorStore::new_with_entity_assets(
        1,
        0,
        test_support::counting_random_assets_for("minecraft:player"),
    );
    store.finish_appearance_fixture_batch();
    store
}

/// Lists then spawns player `id` as one wire batch would, with uuid, unique and runtime ids from `id`.
fn add_player(store: &mut ActorStore, id: u8, skin: protocol::PlayerSkin) {
    let sequence = u64::from(id) * 2;
    store.apply(
        1,
        sequence - 1,
        protocol::ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
            entries: Arc::from([protocol::PlayerListEntry::Add {
                uuid: [id; 16],
                unique_id: i64::from(id),
                username: "fixture".into(),
                verified: true,
                skin,
            }]),
        }),
    );
    store.apply(
        1,
        sequence,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: i64::from(id),
            runtime_id: u64::from(id),
            kind: ActorKind::Player {
                uuid: [id; 16],
                username: "fixture".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
}

/// Distinct pixels, capes and geometry reveal an incomplete appearance replacement.
fn profile_skin(byte: u8) -> protocol::PlayerSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: vec![byte; side * side * 4].into(),
        cape: Some(render_api::CapeImage {
            width: side as u32,
            height: side as u32 / 2,
            rgba8: vec![byte; side * side * 2].into(),
        }),
        geometry: Some(skin_source(
            &SKIN_MODEL.replace("[8,12,4]", &format!("[{},12,4]", 8 + byte)),
        )),
    })
}

#[test]
fn queued_appearance_does_not_publish_new_pixels_before_model_readiness() {
    let mut store = profile_store(1);
    store.advance_interpolation_ticks(1);
    assert!(
        store.player_profile(1).is_none(),
        "a cold appearance must await completed immutable work"
    );
    assert!(store.actor_rig(1).is_none());
}

#[test]
fn delayed_first_skin_uses_an_animated_pose_without_repeating_authored_effects() {
    for pending_ticks in [0, 1] {
        let mut store = player_store();
        let mut skin = profile_skin(1);
        if let protocol::PlayerSkin::Standard(skin) = &mut skin {
            skin.geometry = Some(skin_source(
                &SKIN_MODEL.replace("\"body\"", "\"rightitem\""),
            ));
        }
        add_player(&mut store, 1, skin);
        store.advance_interpolation_frame(pending_ticks);
        assert!(store.actor_rig(1).is_none());
        store.finish_appearance_fixture_batch();
        store.advance_interpolation_frame(0);
        let rig = store.actor_rig(1).unwrap();
        assert!(rig.skin_geometry.is_some());
        assert!(rig.pose_evaluated);
        assert_ne!(
            rig.current, rig.rest,
            "first prepared skin must be animated"
        );
        assert_eq!(rig.previous, rig.current);
        assert_eq!(rig.completed_tick, u64::from(pending_ticks));
        assert_eq!(rig.render[0].color[0], 1.0, "authored effect runs once");
        let pose = rig.current.to_vec();
        let layers = rig.render.to_vec();
        store.advance_interpolation_frame(0);
        assert_eq!(store.actor_rig(1).unwrap().current, pose);
        assert_eq!(store.actor_rig(1).unwrap().render, layers);
        store.advance_interpolation_frame(1);
        assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], 2.0);
    }
}

#[test]
fn queued_replacement_retains_the_complete_previous_appearance() {
    let mut store = profile_store(1);
    store.advance_interpolation_ticks(1);
    store.finish_appearance_fixture_batch();
    store.advance_interpolation_ticks(1);
    let previous = store.player_profile(1).unwrap().clone();
    let previous_model = Arc::clone(store.actor_rig(1).unwrap().skin_geometry.unwrap());
    assert!(store.actor_rig(1).unwrap().skin_mesh.is_some());
    let next = profile_skin(2);
    store.apply_skin_update([1; 16], next.clone());
    assert_eq!(store.player_profile(1), Some(&previous));
    store.advance_interpolation_ticks(1);
    assert_eq!(
        store.player_profile(1),
        Some(&previous),
        "all appearance channels stay together while queued"
    );
    assert!(Arc::ptr_eq(
        &previous_model,
        store.actor_rig(1).unwrap().skin_geometry.unwrap()
    ));
    store.finish_appearance_fixture_batch();
    store.advance_interpolation_frame(0);
    assert_eq!(
        store.player_profile(1),
        Some(&previous),
        "pixels wait for the matching animation tick"
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(store.player_profile(1).unwrap().skin, next);
    assert_ne!(
        store.actor_rig(1).unwrap().skin_geometry.unwrap().digest,
        previous_model.digest
    );
}

#[test]
fn cleared_session_cannot_publish_an_old_appearance_completion() {
    let mut store = profile_store(1);
    store.advance_interpolation_ticks(1);
    store.begin_session(2, 0);
    store.advance_interpolation_ticks(1);
    assert!(store.player_profile(1).is_none());
    assert_eq!(store.actor_rigs().count(), 0);
}

/// A skin naming a catalog model, as classic skins do, has nothing to wait for.
fn standard_skin() -> protocol::PlayerSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: vec![9; side * side * 4].into(),
        cape: None,
        geometry: Some(Arc::new(SkinGeometrySource {
            resource_patch: r#"{"geometry":{"default":"geometry.item"}}"#.into(),
            geometry_data: "".into(),
            animations: Arc::from([]),
        })),
    })
}

/// Whether player `id` is drawable with its catalog model installed.
fn drawn_with_model(store: &ActorStore, id: u64) -> bool {
    store
        .actor_rigs()
        .any(|rig| rig.actor.runtime_id == id && rig.skin_geometry.is_some())
}

#[test]
fn cold_catalog_model_prepares_off_the_frame_thread_then_resolves_inline() {
    let mut store = player_store();
    let prepared = test_support::skin_preparations_on_this_thread();
    add_player(&mut store, 1, standard_skin());
    store.advance_interpolation_frame(0);
    assert_eq!(
        test_support::skin_preparations_on_this_thread(),
        prepared,
        "no model work on the frame thread"
    );
    assert!(!drawn_with_model(&store, 1));
    store.finish_appearance_fixture_batch();
    store.advance_interpolation_frame(0);
    assert!(drawn_with_model(&store, 1));
    add_player(&mut store, 2, standard_skin());
    store.advance_interpolation_frame(0);
    assert!(
        drawn_with_model(&store, 2),
        "a warmed catalog model publishes in the frame its player is added"
    );
    assert_eq!(test_support::skin_preparations_on_this_thread(), prepared);
}

#[test]
fn standard_humanoid_skin_is_drawable_in_the_frame_it_is_added() {
    let mut store = player_store();
    let prepared = test_support::skin_preparations_on_this_thread();
    let side = render_api::CLASSIC_SKIN_SIDE;
    let skin = protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: vec![9; side * side * 4].into(),
        cape: None,
        geometry: Some(Arc::new(SkinGeometrySource {
            resource_patch: r#"{"geometry" : {"default" : "geometry.humanoid.customSlim"}}"#.into(),
            geometry_data: "".into(),
            animations: Arc::from([]),
        })),
    });
    add_player(&mut store, 1, skin);
    store.advance_interpolation_frame(0);
    assert!(store.player_profile(1).is_some());
    assert!(store.actor_rigs().any(|rig| rig.actor.runtime_id == 1));
    assert_eq!(test_support::skin_preparations_on_this_thread(), prepared);
}

/// A ready join within the burst limit publishes every player in its arrival frame.
#[test]
fn a_normal_ready_join_publishes_every_player_in_the_arrival_frame() {
    let mut store = player_store();
    let prepared = test_support::skin_preparations_on_this_thread();
    let count = MAX_APPEARANCES_PUBLISHED_PER_FRAME;
    for id in 1..=count as u8 {
        add_player(&mut store, id, ready_standard_skin(id));
    }
    store.advance_interpolation_frame(0);
    assert_eq!(store.actor_rigs().count(), count);
    for id in 1..=count as u64 {
        assert!(store.player_profile(id).is_some());
    }
    assert_eq!(test_support::skin_preparations_on_this_thread(), prepared);
}

/// Creates a standard appearance that needs no model preparation.
fn ready_standard_skin(color: u8) -> protocol::PlayerSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    protocol::PlayerSkin::Standard(render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: vec![color; side * side * 4].into(),
        cape: None,
        geometry: None,
    })
}

/// The local appearance publishes first even when a ready crowd exceeds the frame limit.
#[test]
fn a_ready_crowd_never_defers_the_local_appearance() {
    let mut store = player_store();
    let limit = MAX_APPEARANCES_PUBLISHED_PER_FRAME;
    let count = limit + 4;
    for id in 1..=count as u8 {
        add_player(&mut store, id, ready_standard_skin(id));
    }
    store.exclude_remote_state_for(count as u64);
    store.advance_interpolation_frame(0);
    assert!(store.actor_rig(count as u64).is_some());
    assert!(store.player_profile(count as u64).is_some());
    assert_eq!(store.actor_rigs().count(), limit);
    store.advance_interpolation_frame(0);
    assert_eq!(store.actor_rigs().count(), count);
}

/// A crowd whose batches finish together is admitted without waiting for earlier batches, then
/// becomes drawable a bounded number of players per frame rather than all in one frame.
#[test]
fn custom_model_crowd_publishes_a_bounded_number_per_frame() {
    const PER_FRAME: usize = MAX_APPEARANCES_PUBLISHED_PER_FRAME;
    let batches = test_support::MAX_SKIN_BATCHES_IN_FLIGHT;
    let crowd = test_support::MAX_SKIN_PREPARATIONS_PER_PASS * batches;
    let mut store = player_store();
    for id in 1..=crowd as u8 {
        add_player(&mut store, id, profile_skin(id));
    }
    let mut drawn = 0;
    for frame in 1..=batches + crowd.div_ceil(PER_FRAME) {
        // Admit every batch without waiting; workers can finish during admission.
        if frame == batches + 1 {
            store.finish_appearance_fixture_batch();
        }
        store.advance_interpolation_frame(0);
        let now = store.actor_rigs().count();
        assert!(
            now - drawn <= PER_FRAME,
            "{} players in frame {frame}",
            now - drawn
        );
        drawn = now;
    }
    assert_eq!(drawn, crowd);
}

#[test]
fn model_installed_between_ticks_advances_the_drawn_rig_generation() {
    let mut store = player_store();
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Player {
                uuid: [1; 16],
                username: "fixture".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    let drawn = store
        .actor_rig(1)
        .expect("a player without a profile draws");
    assert!(drawn.skin_geometry.is_none());
    let generation = drawn.reset_generation;
    store.apply(
        1,
        2,
        protocol::ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
            entries: Arc::from([protocol::PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "fixture".into(),
                verified: true,
                skin: standard_skin(),
            }]),
        }),
    );
    store.advance_interpolation_frame(0);
    store.finish_appearance_fixture_batch();
    store.advance_interpolation_frame(0);
    let rig = store.actor_rig(1).unwrap();
    assert!(rig.skin_geometry.is_some());
    // Cached pose conversions key on this; reused pose buffers must not keep the old skeleton.
    assert_ne!(rig.reset_generation, generation);
}
