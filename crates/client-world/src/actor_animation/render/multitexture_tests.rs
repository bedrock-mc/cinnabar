use super::super::*;

#[test]
#[ignore = "requires a pinned entity carrier; set CINNABAR_ENTITY_CARRIER"]
fn pinned_trader_llama_draws_body_and_carpet_as_one_three_sampler_layer() {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    for baby in [false, true] {
        for variant in 0..4 {
            let mut actor = tests::actor_with_metadata(HashMap::from([
                (2, ActorMetadataValue::Int(variant)),
                (43, ActorMetadataValue::Int(1)),
                (
                    0,
                    ActorMetadataValue::Flags(if baby { 1 << query::FLAG_BABY } else { 0 }),
                ),
            ]));
            actor.kind = ActorKind::Entity {
                identifier: "minecraft:trader_llama".into(),
            };
            let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
            store.insert(1, 0, &actor);
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
                .expect("the native llama rig is admitted");
            assert_eq!(
                snapshot.render.len(),
                1,
                "textures are samplers, not three coplanar draws"
            );
            let layer = &snapshot.render[0];
            let extra = layer
                .multitexture
                .expect("body, decor and empty sampler travel together");
            for source in [layer.source, extra[0], extra[1]] {
                assert!(assets::native_actor_texture_uses_multitexture(
                    &assets.sources()[source as usize]
                ));
            }
            let body = &assets.sources()[layer.source as usize].path;
            assert_eq!(body.contains("_baby"), baby);
            let decor = &assets.sources()[extra[0] as usize].path;
            assert!(decor.contains(if baby {
                "trader_llama_baby"
            } else {
                "trader_llama_decor"
            }));
        }
    }
}
