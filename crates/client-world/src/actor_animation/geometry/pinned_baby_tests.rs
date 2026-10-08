//! Runtime selection and animation gates of the pinned native baby polar bear.
use super::super::*;

#[test]
#[ignore = "requires CINNABAR_ENTITY_CARRIER pointing to the pinned compiled entity carrier"]
fn pinned_baby_polar_bear_selects_its_own_model_and_never_plays_the_adult_body_clip() {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let mut actor = tests::actor_with_metadata(HashMap::from([(
        0,
        ActorMetadataValue::Flags(1 << query::FLAG_BABY),
    )]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:polar_bear".into(),
    };
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    for _ in 0..3 {
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            true,
            |_| ActorTickContext::default(),
        );
        let snapshot = store
            .get(actor.runtime_id)
            .expect("native baby rig is admitted");
        let geometry = assets.rig_geometries()[snapshot.rig.0 as usize].geometry as usize;
        assert_eq!(
            assets.geometries()[geometry].identifier.as_ref(),
            "geometry.polar_bear.baby"
        );
        assert_eq!(
            snapshot.scale, 2.0,
            "native entity description's baby scale"
        );
        let body = snapshot
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "body")
            .unwrap();
        assert_eq!(
            snapshot.current[body], snapshot.rest[body],
            "adult move clip cannot transform the baby body"
        );
        assert_eq!(
            snapshot.current[body].translation_scale[..3],
            [0.0, 6.5, 4.0]
        );
        assert_eq!(snapshot.render.len(), 1);
        let layer = &snapshot.render[0];
        assert!(
            layer.geometry.is_none(),
            "render and pose use the same selected geometry"
        );
        assert!(
            assets.sources()[layer.source as usize]
                .path
                .contains("polar_bear_baby")
        );
    }
}
