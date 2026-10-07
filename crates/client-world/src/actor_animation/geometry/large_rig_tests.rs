use super::*;

fn compiled_display(bone_count: usize) -> assets::CompiledEntityAssets {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/display.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:test".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 2;
    let template = compiled.geometries[0].bones[0].clone();
    compiled.geometries[0].bones = (0..bone_count)
        .map(|index| EntityGeometryBone {
            name: format!("part{index}").into(),
            parent: (index != 0).then(|| "part0".into()),
            pivot: Some(
                [0.0, index as f32, 0.0]
                    .map(|value| assets::EntityGeometryScalar::new(value).unwrap()),
            ),
            ..template.clone()
        })
        .collect();
    compiled.animation_channels[0].bone = (bone_count - 1) as u32;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.animation_keyframes[0].value =
        [2.0, 0.0, 0.0].map(|value| assets::EntityGeometryScalar::new(value).unwrap());
    compiled
}

#[test]
fn composite_display_rigs_keep_every_admitted_bone_and_animate_the_last_part() {
    for bone_count in [
        assets::MAX_SKIN_GEOMETRY_BONES + 1,
        190,
        assets::MAX_ENTITY_GEOMETRY_BONES,
    ] {
        let assets =
            Arc::new(RuntimeEntityAssets::from_compiled(compiled_display(bone_count)).unwrap());
        let actor = super::super::tests::actor_with_metadata(HashMap::new());
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.set_pack(Some((assets, vec![0])));
        store.insert(1, 0, &actor);
        let rig = store.get(actor.runtime_id).expect("admitted display rig");
        assert_eq!(rig.current.len(), bone_count);
        assert!(rig.rig.0 >= assets::PACK_RIG_ID_BASE);
        let rest = rig.rest[bone_count - 1];
        for _ in 0..3 {
            store.advance_tick(
                &HashMap::from([(actor.runtime_id, actor.clone())]),
                None,
                None,
                true,
                false,
                |_| ActorTickContext::default(),
            );
            let rig = store.get(actor.runtime_id).unwrap();
            assert_eq!(rig.previous.len(), bone_count);
            assert_eq!(rig.current.len(), bone_count);
            assert_eq!(rig.render.len(), 1);
            assert_eq!(rig.current[bone_count - 1].translation_scale[0], -2.0);
            assert_eq!(
                rig.current[bone_count - 1].translation_scale[1],
                rest.translation_scale[1]
            );
            assert_eq!(store.stats().frozen_actors, 0);
            assert_eq!(store.stats().unrigged_spawns, 0);
        }
    }
}

#[test]
fn runtime_skeleton_rejects_parts_beyond_the_entity_geometry_bound() {
    let compiled = compiled_display(assets::MAX_ENTITY_GEOMETRY_BONES + 1);
    assert!(skeleton(&compiled.geometries[0].bones).is_none());
}
