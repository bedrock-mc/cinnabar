//! Isolated camera-item routing tests; no optional installed carriers or native art.

use super::*;
use bevy::math::Quat;
use client_world::ItemAnimationState;
use render::ActorRenderIdentity;
use sha2::{Digest, Sha256};

#[test]
fn unchanged_authored_pose_sampling_allocates_nothing_and_keeps_matrix_identity() {
    let (mut runtime, body, _) = block_fixture();
    let transform = body.input.current_bones[0];
    let translated = RenderBoneTransform {
        translation_scale: [0.5, 0.0, 0.0, 1.0],
        ..transform
    };
    for endpoints in [[transform, transform], [transform, translated]] {
        let pose = runtime
            .poses
            .sample_pair(&body, LAYER_MAIN_HAND, 1, |endpoint, _| {
                Some(endpoints[endpoint])
            })
            .unwrap();
        let allocated = crate::test_allocations::count();
        let repeated = runtime
            .poses
            .sample_pair(&body, LAYER_MAIN_HAND, 1, |endpoint, _| {
                Some(endpoints[endpoint])
            })
            .unwrap();
        assert_eq!(crate::test_allocations::count() - allocated, 0);
        assert!(Arc::ptr_eq(&pose[0], &repeated[0]));
        assert!(Arc::ptr_eq(&pose[1], &repeated[1]));
        assert_eq!(
            Arc::ptr_eq(&pose[0], &pose[1]),
            endpoints[0] == endpoints[1]
        );
    }
}

/// The cube sheet is injected after atlas construction: these tests cover placement and
/// routing, not block-carrier admission (which has separate asset tests).
fn block_fixture() -> (EquipmentRuntime, ActorRigSubmission, WornItem) {
    let entities = RuntimeEntityAssets::from_compiled(assets::CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 8,
        sources: vec![assets::EntityAssetSource {
            path: "entity/fixture.entity.json".into(),
            source_bytes: 2,
            source_sha256: Sha256::digest(b"{}").into(),
        }]
        .into(),
        symbols: vec![assets::EntityAssetSymbol {
            kind: assets::EntityAssetKind::Entity,
            identifier: "test:fixture".into(),
            source_index: 0,
            dependencies: Box::new([]),
        }]
        .into(),
        geometries: Box::new([]),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    })
    .unwrap();
    let icons =
        RuntimeIconCatalog::decode(&assets::encode_icon_catalog([1; 32], &[], &[]).unwrap())
            .unwrap();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::new(entities),
        None,
        Arc::new(icons),
        None,
        None,
        ActorArtworkPages::default(),
    );
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let sheet = IconSprite {
        width,
        height,
        rgba8: vec![255; usize::from(width) * usize::from(height) * 4].into(),
    };
    let atlas = SpriteAtlas::pack(&[sheet]);
    let (_, locations) = pages.with_equipment_rasters(&atlas.layers);
    runtime.placements = atlas.placements;
    runtime.atlas_locations = locations;
    let visual = 7;
    runtime.block_sheets.insert(visual, 0);
    let rig = EntityRigId(0x7000_0000);
    runtime.register_skin_rig(rig, vec!["rightItem".into()]);
    let bone = RenderBoneTransform {
        rotation: Quat::from_rotation_x(0.3).to_array(),
        translation_scale: [0.2, 0.9, 0.3, 0.9],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let body = ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 1,
                pose_generation: 1,
                layer: ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: Arc::from([bone]),
            current_bones: Arc::from([bone]),
            completed_tick: 1,
            reset_generation: 1,
        },
        world_from_actor: [
            [0.9, 0.0, 0.0, 10.0],
            [0.0, 0.9, 0.0, 20.0],
            [0.0, 0.0, 0.9, 30.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let item = WornItem {
        identifier: Arc::from("test:opaque_cube"),
        metadata: 0,
        damage: None,
        kind: HeldKind::Block(visual),
        dye_rgb: None,
        enchanted: false,
    };
    (runtime, body, item)
}

fn third_person(
    runtime: &mut EquipmentRuntime,
    body: &ActorRigSubmission,
    item: &WornItem,
) -> EquipmentPresentation {
    let layers = runtime.layers_for(
        body,
        &ActorEquipmentInput {
            main: Some(item.clone()),
            ..Default::default()
        },
        None,
    );
    assert_eq!(layers.len(), 1);
    layers[0].clone()
}

#[test]
fn carried_block_sheet_is_admitted_without_untinted_world_materials() {
    let (base, body, item) = block_fixture();
    let face = IconSprite {
        width: assets::BLOCK_ITEM_FACE_SIDE,
        height: assets::BLOCK_ITEM_FACE_SIDE,
        rgba8: vec![255; usize::from(assets::BLOCK_ITEM_FACE_SIDE).pow(2) * 4].into(),
    };
    let sheet = assets::compose_block_item_sheet(&std::array::from_fn(|_| face.clone())).unwrap();
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            base.assets.source_manifest_sha256(),
            &[sheet],
            &[],
            &[assets::IconBlockSheet {
                visual: assets::BlockVisualId(7),
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut runtime, _, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::new(icons),
        None,
        None,
        ActorArtworkPages::default(),
    );
    assert_eq!(runtime.block_sheets.get(&7), Some(&0));
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    let first = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .expect("authored carried sheet must produce a held cube");
    assert!(first.camera_space);
    assert_eq!(runtime.take_pending_geometries().len(), 1);
}

#[test]
fn carried_block_sheet_rejects_a_stale_manifest_or_out_of_range_visual() {
    let (base, _, _) = block_fixture();
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let sheet = IconSprite {
        width,
        height,
        rgba8: vec![255; usize::from(width) * usize::from(height) * 4].into(),
    };
    for (manifest, visual) in [([2; 32], 7), ([1; 32], base.assets.block_visual_count())] {
        let icons = RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog_with_block_sheets(
                manifest,
                std::slice::from_ref(&sheet),
                &[],
                &[assets::IconBlockSheet {
                    visual: assets::BlockVisualId(visual),
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap();
        let (runtime, _, _) = EquipmentRuntime::build(
            Arc::clone(&base.assets),
            None,
            Arc::new(icons),
            None,
            None,
            ActorArtworkPages::default(),
        );
        assert!(runtime.block_sheets.is_empty());
    }
}

#[test]
fn carried_cube_alpha_mode_follows_world_materials_for_both_hands() {
    use assets::{
        BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
        TextureArray, TextureMip, TextureRef, VisualKind, VisualSupport,
    };
    use render::HandItemAlphaMode;

    let (base, body, mut item) = block_fixture();
    let flags = [
        0,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MATERIAL_FLAG_ALPHA_BLEND,
    ];
    let overlay = BlockOverlay {
        visuals: (0..flags.len())
            .map(|material| BlockVisual {
                faces: [material as u32; 6],
                flags: BlockFlags::CUBE_GEOMETRY,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: assets::NO_MODEL_TEMPLATE,
                animation: assets::NO_ANIMATION,
                variant: 0,
            })
            .collect(),
        light_properties: vec![LightProperties::OPAQUE_DARK; flags.len()],
        materials: flags
            .into_iter()
            .map(|flags| Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags,
                ..Material::unvaried()
            })
            .collect(),
        texture: Some(TextureArray {
            layers: 1,
            mips: [16, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![127; (size * size * 4) as usize].into(),
                })
                .collect(),
        }),
        ..Default::default()
    };
    let world = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap();
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let sheet = IconSprite {
        width,
        height,
        rgba8: vec![127; usize::from(width) * usize::from(height) * 4].into(),
    };
    let mappings = (1..=flags.len() as u32)
        .map(|visual| assets::IconBlockSheet {
            visual: assets::BlockVisualId(visual),
            sprite: 0,
        })
        .collect::<Vec<_>>();
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            base.assets.source_manifest_sha256(),
            &[sheet],
            &[],
            &mappings,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut runtime, _, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::new(icons),
        Some(Arc::new(world)),
        None,
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    let mut rigs = Vec::new();
    for (visual, expected) in (1..).zip([
        HandItemAlphaMode::Opaque,
        HandItemAlphaMode::Cutout,
        HandItemAlphaMode::Blend,
    ]) {
        item.kind = HeldKind::Block(visual);
        let main = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(main.alpha_mode, expected);
        assert_eq!(off.alpha_mode, expected);
        rigs.push(main.presentation.submission.input.rig);
        assert_eq!(main.presentation.location, off.presentation.location);
        assert_eq!(
            main.presentation.submission.input.rig,
            off.presentation.submission.input.rig
        );
    }
    assert_eq!(runtime.take_pending_geometries().len(), rigs.len());
    for (visual, rig) in (1..).zip(rigs) {
        item.kind = HeldKind::Block(visual);
        let repeated = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        assert_eq!(repeated.presentation.submission.input.rig, rig);
    }
    assert!(runtime.take_pending_geometries().is_empty());
}

#[test]
fn session_block_sheets_preserve_material_alpha_modes_for_both_hands() {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    use render::HandItemAlphaMode;

    let (mut runtime, body, mut item) = block_fixture();
    item.identifier = Arc::from("test:custom_cube");
    for (flags, expected) in [
        (0, HandItemAlphaMode::Opaque),
        (
            assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            HandItemAlphaMode::Cutout,
        ),
        (assets::MATERIAL_FLAG_ALPHA_BLEND, HandItemAlphaMode::Blend),
    ] {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
        let items = crate::session_assets::SessionItems {
            components: Arc::new(Default::default()),
            icons: Some(Arc::new(SessionIcons {
                block_sheets: vec![SessionIcon {
                    identifier: Arc::clone(&item.identifier),
                    metadata: 0,
                    width,
                    height,
                    rgba8: vec![127; (width * height * 4) as usize].into(),
                }],
                block_material_flags: BTreeMap::from([(Arc::clone(&item.identifier), flags)]),
                ..Default::default()
            })),
        };
        let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
        let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(staged.rasters());
        runtime.set_session_items(Some(&items), Some(staged), locations);
        for kind in [HeldKind::Other, HeldKind::Sprite] {
            item.kind = kind;
            let main = runtime
                .first_person_item(&body, &item, ItemAnimationState::default())
                .unwrap();
            let off = runtime.first_person_offhand(&body, &item).unwrap();
            assert_eq!(main.alpha_mode, expected);
            assert_eq!(off.alpha_mode, expected);
            assert_eq!(
                main.presentation.submission.input.rig,
                off.presentation.submission.input.rig
            );
        }
    }
}

#[test]
fn first_person_block_uses_camera_space_and_reuses_the_cube_atlas_and_rig() {
    let (mut runtime, body, item) = block_fixture();
    let first = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    assert!(
        first.camera_space,
        "ordinary blocks must not ride rightItem"
    );
    let expected =
        crate::presentation::equipment::first_person::block_pose(ItemAnimationState::default())
            .unwrap();
    assert_eq!(
        &*first.presentation.submission.input.previous_bones,
        &[expected]
    );
    assert_eq!(
        &*first.presentation.submission.input.current_bones,
        &[expected]
    );
    let third = third_person(&mut runtime, &body, &item);
    assert_eq!(first.presentation.location, third.location);
    assert_eq!(
        first.presentation.submission.input.rig,
        third.submission.input.rig
    );
    assert_ne!(
        first.presentation.submission.input.current_bones,
        third.submission.input.current_bones
    );
    assert_eq!(runtime.take_pending_geometries().len(), 1);
}

#[test]
fn first_person_block_pose_ignores_avatar_bones_and_model_scale() {
    let (mut runtime, body, item) = block_fixture();
    let animation = ItemAnimationState {
        attack_time: 0.25,
        arm_height: 0.6,
    };
    let first = runtime.first_person_item(&body, &item, animation).unwrap();
    let expected = crate::presentation::equipment::first_person::block_pose(animation).unwrap();
    assert_eq!(
        &*first.presentation.submission.input.previous_bones,
        &[expected]
    );
    assert_eq!(
        &*first.presentation.submission.input.current_bones,
        &[expected]
    );
    let mut changed = body.clone();
    let changed_bone = RenderBoneTransform {
        rotation: Quat::from_rotation_z(1.7).to_array(),
        translation_scale: [20.0, -12.0, 8.0, 3.0],
        axis_scale: [2.0, 2.0, 2.0, 1.0],
    };
    changed.input.previous_bones = Arc::from([changed_bone]);
    changed.input.current_bones = Arc::from([changed_bone]);
    changed.world_from_actor = [
        [3.0, 0.0, 0.0, -40.0],
        [0.0, 3.0, 0.0, 50.0],
        [0.0, 0.0, 3.0, 60.0],
    ];
    let second = runtime
        .first_person_item(&changed, &item, animation)
        .unwrap();
    assert_eq!(
        first.presentation.submission.input.previous_bones,
        second.presentation.submission.input.previous_bones
    );
    assert_eq!(
        first.presentation.submission.input.current_bones,
        second.presentation.submission.input.current_bones
    );
    assert_eq!(first.presentation.location, second.presentation.location);
    let third = third_person(&mut runtime, &body, &item);
    let third_changed = third_person(&mut runtime, &changed, &item);
    assert_ne!(
        third.submission.input.current_bones,
        third_changed.submission.input.current_bones
    );
    assert_ne!(
        third.submission.world_from_actor,
        third_changed.submission.world_from_actor
    );
}

#[test]
fn third_person_block_retains_the_existing_grip_on_the_avatar_bone() {
    let (mut runtime, body, item) = block_fixture();
    let third = third_person(&mut runtime, &body, &item);
    let expected = attach_to_bone(body.input.current_bones[0], held_block_display()).unwrap();
    assert_eq!(&*third.submission.input.previous_bones, &[expected]);
    assert_eq!(&*third.submission.input.current_bones, &[expected]);
    assert_eq!(third.submission.world_from_actor, body.world_from_actor);
}

#[test]
fn review_render_pack_replacement_reclaims_mesh_slots_without_reusing_vanilla_ids() {
    let (mut runtime, _, _) = block_fixture();
    runtime.next_mesh = MAX_ITEM_MESHES as u32;
    for index in 0..MAX_ITEM_MESHES as u32 - 1 {
        runtime
            .meshes
            .insert(MeshKey::Block(index), Some(item_mesh_rig_id(index)));
    }
    let retired = item_mesh_rig_id(MAX_ITEM_MESHES as u32 - 1);
    let placement = runtime.placements[0].unwrap();
    for _ in 0..8 {
        runtime.attachable_meshes.insert((true, 0, 0), retired);
        runtime.set_pack_layer(None);
        assert_eq!(
            runtime.build_mesh(MeshKey::Block(7), 0, placement),
            Some(retired)
        );
        assert_eq!(
            runtime.meshes[&MeshKey::Block(0)],
            Some(item_mesh_rig_id(0))
        );
    }
}

#[test]
fn review_render_pack_replacement_invalidates_only_pack_armor_maps() {
    let (mut runtime, _, _) = block_fixture();
    runtime
        .armor_maps
        .insert((1, "vanilla".into()), Arc::from([Some(0)]));
    runtime.armor_maps.insert(
        (
            1,
            format!("{}geometry.armor", pack::ARMOR_CACHE_PREFIX).into(),
        ),
        Arc::from([Some(1)]),
    );
    runtime.set_pack_layer(None);
    assert_eq!(runtime.armor_maps.len(), 1);
    assert!(runtime.armor_maps.contains_key(&(1, "vanilla".into())));
}

#[test]
fn compiled_attack_facts_normalize_and_session_overrides_reset_to_the_catalog() {
    let (fixture, _, _) = block_fixture();
    let compiled = assets::CompiledItemAttackTiming {
        identifier: "fixture:spear".into(),
        swing_duration_seconds: assets::ItemDisplayScalar::new(0.75),
        attack_cooldown: Some(assets::CompiledItemAttackCooldown {
            category: "fixture:jab".into(),
            duration_seconds: assets::ItemDisplayScalar::new(0.5).unwrap(),
        }),
        piercing_weapon: true,
        is_spear: true,
        kinetic_weapon: Some(assets::CompiledKineticWeaponTiming {
            delay_ticks: 4,
            dismount_ticks: 30,
            knockback_ticks: 60,
            damage_ticks: 90,
        }),
    };
    let bytes = assets::encode_equipment_catalog_with_attack_timings(
        [1; 32],
        [2; 32],
        &[],
        &[],
        &[],
        &[compiled],
    )
    .unwrap();
    let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
    let (mut runtime, _, _) = EquipmentRuntime::build(
        fixture.assets,
        Some(Arc::new(catalog)),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let base = runtime.item_attack_timings();
    let timing = base.get("fixture:spear").unwrap();
    assert_eq!(
        timing.swing_duration_ticks,
        Some(sim::TICKS_PER_SECOND * 3 / 4)
    );
    let cooldown = timing.attack_cooldown.as_ref().unwrap();
    assert_eq!(cooldown.category.as_ref(), "fixture:jab");
    assert_eq!(cooldown.ticks, sim::TICKS_PER_SECOND / 2);
    assert!(timing.piercing_weapon);
    assert!(timing.is_spear);
    assert_eq!(timing.kinetic_weapon.unwrap().damage_ticks, 90);
    let override_timing = protocol::ItemAttackTiming {
        swing_duration_ticks: Some(sim::TICKS_PER_SECOND),
        ..Default::default()
    };
    let items = crate::session_assets::SessionItems {
        components: Arc::new(
            [
                (
                    Arc::from("fixture:spear"),
                    protocol::ItemComponents {
                        attack: Some(override_timing.clone()),
                        ..Default::default()
                    },
                ),
                (
                    Arc::from("fixture:custom"),
                    protocol::ItemComponents {
                        attack: Some(override_timing.clone()),
                        ..Default::default()
                    },
                ),
            ]
            .into_iter()
            .collect(),
        ),
        icons: None,
    };
    runtime.set_session_items(Some(&items), None, Vec::new());
    assert_eq!(
        runtime.item_attack_timings().get("fixture:spear"),
        Some(&override_timing)
    );
    assert_eq!(
        runtime.item_attack_timings().get("fixture:custom"),
        Some(&override_timing)
    );
    runtime.set_session_items(None, None, Vec::new());
    assert_eq!(runtime.item_attack_timings(), base);
}
