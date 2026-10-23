use super::super::*;
use assets::{EntityAssetSource, EntityRenderCandidate, EntityRenderSlot};

/// Builds a projectile controller with a sprite and an optional secondary glint sampler.
fn projectile_assets(identifier: &str, secondary: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled(identifier);
    if secondary {
        let mut sources = compiled.sources.into_vec();
        let glint = sources.len() as u32;
        sources.push(EntityAssetSource {
            path: format!("{}.png", assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER).into(),
            source_bytes: 1,
            source_sha256: [2; 32],
        });
        compiled.sources = sources.into();
        compiled.render.layers[0].slot_count = 2;
        compiled.render.slots = vec![
            EntityRenderSlot {
                first_candidate: 0,
                candidate_count: 1,
                default_candidate: None,
            },
            EntityRenderSlot {
                first_candidate: 1,
                candidate_count: 1,
                default_candidate: None,
            },
        ]
        .into();
        let mut candidates = compiled.render.candidates.into_vec();
        candidates.push(EntityRenderCandidate {
            condition: None,
            source: glint,
        });
        compiled.render.candidates = candidates.into();
    }
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn thrown_items_do_not_draw_secondary_glint_samplers() {
    for (identifier, secondary) in [
        ("minecraft:splash_potion", true),
        ("minecraft:lingering_potion", true),
        ("minecraft:xp_bottle", true),
        ("minecraft:ender_pearl", false),
        ("custom:projectile", true),
    ] {
        let assets = projectile_assets(identifier, secondary);
        let primary = assets.render_data().candidates[0].source;
        let mut actor = tests::actor_with_metadata(HashMap::new());
        actor.kind = protocol::ActorKind::Entity {
            identifier: identifier.into(),
        };
        let mut store = ActorAnimationStore::with_assets(assets);
        store.insert(1, 0, &actor);
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            true,
            |_| Default::default(),
        );
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.render.len(), 1, "{identifier}: one sprite draw");
        assert_eq!(
            rig.render[0].source, primary,
            "{identifier}: primary sprite"
        );
        for alpha in [0.0, 0.5, 1.0] {
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let sampled = store
                .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
                .unwrap();
            assert_eq!(sampled.render.len(), 1, "{identifier}: sampled sprite draw");
            assert_eq!(sampled.render[0].source, rig.render[0].source);
            assert_eq!(sampled.render[0].texture_slot, 0);
            assert!(sampled.render[0].multitexture.is_none());
        }
    }
}

#[test]
fn separate_projectile_controllers_keep_their_draw_layers() {
    let identifier = "minecraft:splash_potion";
    let mut compiled = crate::test_support::counting_random_compiled(identifier);
    let mut second = compiled.render.layers[0];
    second.first_slot = compiled.render.slots.len() as u32;
    compiled.render.layers = vec![compiled.render.layers[0], second].into();
    compiled.render.slots = vec![
        compiled.render.slots[0],
        EntityRenderSlot {
            first_candidate: compiled.render.candidates.len() as u32,
            candidate_count: 1,
            default_candidate: None,
        },
    ]
    .into();
    compiled.render.candidates =
        vec![compiled.render.candidates[0], compiled.render.candidates[0]].into();
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let primary = assets.render_data().candidates[0].source;
    let mut actor = tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: identifier.into(),
    };
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| Default::default(),
    );
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.render.len(), 2, "each controller retains its draw");
    assert!(rig.render.iter().all(|layer| layer.source == primary));
}
