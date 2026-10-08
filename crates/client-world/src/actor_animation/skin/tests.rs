use super::*;

const PATCH: &str = r#"{"geometry":{"default":"geometry.skin_cache"}}"#;
pub(super) const MODEL: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[{
"description":{"identifier":"geometry.skin_cache","texture_width":64,"texture_height":64},
"bones":[{"name":"root","pivot":[0,0,0]},{"name":"body","parent":"root","pivot":[0,12,0],
"cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]}]}]}"#;

/// Builds independent source allocations, as separate player-list entries arrive from the wire.
pub(super) fn source(model: &str) -> Arc<SkinGeometrySource> {
    Arc::new(SkinGeometrySource {
        resource_patch: PATCH.to_string().into(),
        geometry_data: model.to_string().into(),
        animations: Arc::from([]),
    })
}

#[test]
fn equal_actor_skin_sources_share_prepared_geometry() {
    let mut store = ActorAnimationStore::with_assets(
        super::super::render_frame::tests::counting_random_assets(),
    );
    let mut actors = HashMap::new();
    let sources: Vec<_> = (0..32).map(|_| source(MODEL)).collect();
    for runtime_id in 1..=sources.len() as u64 {
        let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
        actor.runtime_id = runtime_id;
        actor.unique_id = runtime_id as i64;
        store.insert(1, 0, &actor);
        actors.insert(runtime_id, actor);
    }
    for source in &sources {
        warm_source(&mut store, source);
    }
    store.advance_tick(&actors, None, None, false, true, |actor| ActorTickContext {
        skin_geometry: Some(Arc::clone(&sources[actor.runtime_id as usize - 1])),
        ..ActorTickContext::default()
    });
    let first = Arc::clone(store.get(1).unwrap().skin_geometry.unwrap());
    for runtime_id in 2..=sources.len() as u64 {
        assert!(Arc::ptr_eq(
            &first,
            store.get(runtime_id).unwrap().skin_geometry.unwrap()
        ));
    }
    let before = store.get(2).unwrap().current.to_vec();
    let lifetime = store.runtime_to_lifetime[&1];
    store.rigs.get_mut(&lifetime).unwrap().current[0].translation_scale[0] += 3.0;
    assert_eq!(
        store.get(2).unwrap().current,
        before,
        "actors must retain independent pose state"
    );
}

/// Installs one actor and returns the immutable model retained by its snapshot.
fn prepared_model(
    store: &mut ActorAnimationStore,
    source: Arc<SkinGeometrySource>,
) -> Arc<assets::SkinGeometry> {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let runtime_id = actor.runtime_id;
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(runtime_id, actor)]);
    warm_source(store, &source);
    store.advance_tick(&actors, None, None, false, true, |_| ActorTickContext {
        skin_geometry: Some(Arc::clone(&source)),
        ..ActorTickContext::default()
    });
    Arc::clone(store.get(runtime_id).unwrap().skin_geometry.unwrap())
}

#[test]
fn skin_preparation_is_owned_by_the_catalog_and_session_lifetime() {
    let assets = super::super::render_frame::tests::counting_random_assets();
    let mut first_store = ActorAnimationStore::with_assets(assets.clone());
    let first = prepared_model(&mut first_store, source(MODEL));
    first_store.clear();
    let after_reset = prepared_model(&mut first_store, source(MODEL));
    assert!(
        !Arc::ptr_eq(&first, &after_reset),
        "reset must retire prepared sources from the previous session"
    );
    let mut second_store = ActorAnimationStore::with_assets(assets);
    let independent = prepared_model(&mut second_store, source(MODEL));
    assert!(
        !Arc::ptr_eq(&after_reset, &independent),
        "catalog owners must not share a mutable cache lifetime"
    );
}

#[test]
fn changed_skin_source_replaces_the_shared_prepared_model() {
    let mut store = ActorAnimationStore::with_assets(
        super::super::render_frame::tests::counting_random_assets(),
    );
    let first = prepared_model(&mut store, source(MODEL));
    let changed = MODEL.replace("[8,12,4]", "[10,12,4]");
    let replacement = prepared_model(&mut store, source(&changed));
    assert_ne!(first.digest, replacement.digest);
    assert!(!Arc::ptr_eq(&first, &replacement));
}

#[test]
fn an_equal_source_update_becomes_the_next_unchanged_pointer_fast_path() {
    let mut store = ActorAnimationStore::with_assets(
        super::super::render_frame::tests::counting_random_assets(),
    );
    let first = prepared_model(&mut store, source(MODEL));
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let runtime_id = actor.runtime_id;
    let actors = HashMap::from([(runtime_id, actor)]);
    let lifetime = store.runtime_to_lifetime[&runtime_id];
    store.rigs.get_mut(&lifetime).unwrap().current[0].translation_scale[0] += 3.0;
    let before = store.rigs[&lifetime].current.clone();
    let replacement = source(MODEL);
    warm_source(&mut store, &replacement);
    store.advance_tick(&actors, None, None, false, true, |_| ActorTickContext {
        skin_geometry: Some(replacement.clone()),
        ..ActorTickContext::default()
    });
    let lifetime = store.runtime_to_lifetime[&runtime_id];
    assert_eq!(
        store.rigs[&lifetime].current, before,
        "equal source replacement preserves pose history"
    );
    let model = store.rigs[&lifetime].skin.as_ref().unwrap();
    assert!(
        Arc::ptr_eq(model.source(), &replacement),
        "accept equal replacement once so later ticks do not compare its bytes again"
    );
    assert!(Arc::ptr_eq(
        &first,
        store.get(runtime_id).unwrap().skin_geometry.unwrap()
    ));
}

/// Lists and spawns one independently allocated, original custom-model appearance.
fn profile_store(byte: u8) -> crate::actor_store::ActorStore {
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(
        1,
        0,
        super::super::render_frame::tests::counting_random_assets_for("minecraft:player"),
    );
    store.apply(
        1,
        1,
        protocol::ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
            entries: Arc::from([protocol::PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "fixture".into(),
                verified: true,
                skin: profile_skin(byte),
            }]),
        }),
    );
    store.apply(
        1,
        2,
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
    store
}

/// Distinct pixels, capes and geometry reveal an incomplete appearance replacement.
fn profile_skin(byte: u8) -> protocol::PlayerSkin {
    let side = protocol::CLASSIC_SKIN_SIDE;
    protocol::PlayerSkin::Standard(protocol::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: vec![byte; side * side * 4].into(),
        cape: Some(protocol::CapeImage {
            width: side as u32,
            height: side as u32 / 2,
            rgba8: vec![byte; side * side * 2].into(),
        }),
        geometry: Some(source(
            &MODEL.replace("[8,12,4]", &format!("[{},12,4]", 8 + byte)),
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

/// Direct animation fixtures explicitly await their own worker preparation.
fn warm_source(store: &mut ActorAnimationStore, source: &Arc<SkinGeometrySource>) {
    store.request_skin_preparation(source);
    store.submit_skin_preparation();
    store.finish_skin_fixture_batch();
}
