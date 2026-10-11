use super::*;
use crate::test_support::{SKIN_MODEL, skin_source};

#[test]
fn equal_actor_skin_sources_share_prepared_geometry() {
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    let mut actors = HashMap::new();
    let sources: Vec<_> = (0..32).map(|_| skin_source(SKIN_MODEL)).collect();
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
    let assets = crate::test_support::counting_random_assets();
    let mut first_store = ActorAnimationStore::with_assets(assets.clone());
    let first = prepared_model(&mut first_store, skin_source(SKIN_MODEL));
    first_store.clear();
    let after_reset = prepared_model(&mut first_store, skin_source(SKIN_MODEL));
    assert!(
        !Arc::ptr_eq(&first, &after_reset),
        "reset must retire prepared sources from the previous session"
    );
    let mut second_store = ActorAnimationStore::with_assets(assets);
    let independent = prepared_model(&mut second_store, skin_source(SKIN_MODEL));
    assert!(
        !Arc::ptr_eq(&after_reset, &independent),
        "catalog owners must not share a mutable cache lifetime"
    );
}

#[test]
fn changed_skin_source_replaces_the_shared_prepared_model() {
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    let first = prepared_model(&mut store, skin_source(SKIN_MODEL));
    let changed = SKIN_MODEL.replace("[8,12,4]", "[10,12,4]");
    let replacement = prepared_model(&mut store, skin_source(&changed));
    assert_ne!(first.digest, replacement.digest);
    assert!(!Arc::ptr_eq(&first, &replacement));
}

#[test]
fn an_equal_source_update_becomes_the_next_unchanged_pointer_fast_path() {
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    let first = prepared_model(&mut store, skin_source(SKIN_MODEL));
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let runtime_id = actor.runtime_id;
    let actors = HashMap::from([(runtime_id, actor)]);
    let lifetime = store.runtime_to_lifetime[&runtime_id];
    store.rigs.get_mut(&lifetime).unwrap().current[0].translation_scale[0] += 3.0;
    let before = store.rigs[&lifetime].current.clone();
    let replacement = skin_source(SKIN_MODEL);
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

/// Direct animation fixtures explicitly await their own worker preparation.
fn warm_source(store: &mut ActorAnimationStore, source: &Arc<SkinGeometrySource>) {
    store.request_skin_preparation(source);
    store.submit_skin_preparation();
    store.finish_skin_fixture_batch();
}

/// A ticked rig whose model `source` then installs between ticks; returns its runtime id.
fn rig_with_model_installed_between_ticks(
    store: &mut ActorAnimationStore,
    before_install: impl FnOnce(&mut ActorAnimationStore, u64),
) -> u64 {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let runtime_id = actor.runtime_id;
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(runtime_id, actor)]);
    store.advance_tick(&actors, None, None, false, true, |_| {
        ActorTickContext::default()
    });
    before_install(store, runtime_id);
    let source = skin_source(SKIN_MODEL);
    warm_source(store, &source);
    store.sync_skin_model(runtime_id, Some(&source));
    assert!(store.get(runtime_id).unwrap().skin_geometry.is_some());
    runtime_id
}

#[test]
fn model_installed_between_ticks_advances_the_rest_generation() {
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    let mut rest_generation = 0;
    let runtime_id = rig_with_model_installed_between_ticks(&mut store, |store, runtime_id| {
        rest_generation = store.get(runtime_id).unwrap().rest_reset_generation;
    });
    // The first-person arm cache keys on this; it must not reuse the old skeleton's arm.
    assert_ne!(
        store.get(runtime_id).unwrap().rest_reset_generation,
        rest_generation
    );
}

#[test]
fn model_installed_between_ticks_never_leaves_a_hud_pose_on_the_old_bones() {
    let mut store = ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
    let runtime_id = rig_with_model_installed_between_ticks(&mut store, |store, runtime_id| {
        let lifetime = store.runtime_to_lifetime[&runtime_id];
        let state = store.rigs.get_mut(&lifetime).unwrap();
        state.ui_pose = Some(state.current.clone());
    });
    let rig = store.get(runtime_id).unwrap();
    let hud = store.ui_pose(runtime_id).unwrap();
    assert_eq!(hud.len(), rig.bone_names.len());
    assert_eq!(hud, rig.current);
}
