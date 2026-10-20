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

#[test]
fn legacy_attack_targets_preserve_custom_skin_and_persona_geometry() {
    let assets = crate::test_support::counting_random_assets();
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    store.insert(1, 0, &actor);
    let source = Arc::new(SkinGeometrySource {
        resource_patch: r#"{"geometry":{"default":"geometry.skin_cache","animated_face":"geometry.layer"}}"#.into(),
        geometry_data: r#"{"format_version":"1.12.0","minecraft:geometry":[
            {"description":{"identifier":"geometry.skin_cache","texture_width":64,"texture_height":64},"bones":[
                {"name":"body","pivot":[0,10,0],"rotation":[0,30,0]},
                {"name":"head","parent":"body","pivot":[0,30,0]},
                {"name":"rightarm","parent":"body","pivot":[-7,26,2],"rotation":[10,20,30]}]},
            {"description":{"identifier":"geometry.layer","texture_width":16,"texture_height":16},"bones":[
                {"name":"head","parent":"body","pivot":[0,32,2]},
                {"name":"rightarm","parent":"body","pivot":[-9,25,3],"rotation":[-15,30,10]},
                {"name":"body","pivot":[2,8,0],"rotation":[0,-20,0]}]}]}"#.into(),
        animations: Arc::from([render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face, width: 16, height: 16,
            rgba8: Arc::from([255; 16 * 16 * 4]), frames: 1, blinking: false,
        }]),
    });
    warm_source(&mut store, &source);
    let lifetime = store.runtime_to_lifetime[&actor.runtime_id];
    let state = store.rigs.get_mut(&lifetime).unwrap();
    state.bone_names = ["body", "head", "rightarm"].map(Into::into).into();
    state.bones = vec![
        RuntimeBone {
            pivot: [0.0, 24.0, 0.0],
            ..Default::default()
        },
        RuntimeBone {
            parent: Some(0),
            pivot: [0.0, 24.0, 0.0],
            ..Default::default()
        },
        RuntimeBone {
            parent: Some(0),
            pivot: [5.0, 22.0, 0.0],
            ..Default::default()
        },
    ];
    assert!(!sync_skin(state, Some(&source), &store.skin_preparation));
    let mut local = vec![LocalDelta::default(); state.bones.len()];
    crate::legacy_swing::apply(&state.bone_names, &mut local, 0.5, 0.0, 1.0).unwrap();
    let mut body = vec![BoneTransform::default(); state.posed_bones().len()];
    state.compose_into(&local, &mut body).unwrap();
    let prepared = &state.skin_skeleton().unwrap().prepared;
    let baseline = compose_pose(&prepared.bones, &[]).unwrap();
    assert_custom_attack(&body, &baseline, &prepared.names);
    let layout = VariableLayout::new(&assets);
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let evaluator = Evaluator {
        assets: &assets,
        pack: false,
        layout: &layout,
        program: None,
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        presentation_alpha: None,
        query_history: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &state.bones,
        bone_names: &state.bone_names,
    };
    let layers = skin_layers::evaluate(state, &evaluator, &layout.fresh(1), (&local, None), None);
    assert_eq!(layers.len(), 1);
    let layer = &prepared.layers[0];
    let baseline = compose_pose(&layer.bones, &[]).unwrap();
    assert_custom_attack(&layers[0].current, &baseline, &layer.names);
    assert_eq!(layers[0].previous, layers[0].current);
}

/// Checks attack deltas relative to each destination's own pivots and authored orientation.
fn assert_custom_attack(pose: &[BoneTransform], baseline: &[BoneTransform], names: &[Box<str>]) {
    let index = |name: &str| {
        names
            .iter()
            .position(|candidate| candidate.as_ref() == name)
            .unwrap()
    };
    let head = index("head");
    assert_eq!(pose[head], baseline[head]);
    let body = index("body");
    assert_eq!(
        pose[body].translation_scale,
        baseline[body].translation_scale
    );
    assert_ne!(pose[body].rotation, baseline[body].rotation);
    let arm = index("rightarm");
    let shift = [-0.092644215, 0.0, -0.95757717];
    for ((actual, baseline), shift) in pose[arm].translation_scale[..3]
        .iter()
        .zip(&baseline[arm].translation_scale[..3])
        .zip(shift)
    {
        assert!((actual - baseline - shift).abs() < 3e-6);
    }
    assert_ne!(pose[arm].rotation, baseline[arm].rotation);
    assert_ne!(
        pose[arm].rotation,
        crate::pose::quat_from_euler([
            0.7592212_f32.to_degrees(),
            0.57832694_f32.to_degrees(),
            0.0
        ])
    );
}

#[test]
fn appearance_binding_indices_follow_skin_install_replacement_and_removal() {
    let assets = crate::test_support::counting_random_assets();
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    store.insert(1, 0, &actor);
    let lifetime = store.runtime_to_lifetime[&actor.runtime_id];
    let initial = Arc::new(render_model::ActorAttachmentBindings::from_cape(
        &assets.geometries()[0].bones,
        &store.rigs[&lifetime].bone_names,
    ));
    store.rigs.get_mut(&lifetime).unwrap().attachment_bindings = Arc::clone(&initial);
    let changed = SKIN_MODEL.replace("\"body\"", "\"rightitem\"");
    for model in [Some(SKIN_MODEL), Some(changed.as_str()), None] {
        let source = model.map(skin_source);
        if let Some(source) = &source {
            warm_source(&mut store, source);
        }
        store.sync_skin_model(actor.runtime_id, source.as_ref());
        let rig = store.get(actor.runtime_id).unwrap();
        let expected = initial.rebind(rig.bone_names);
        assert_eq!(rig.attachments.unwrap().cape(), expected.cape());
    }
}
