//! Worn category and cape composition follow the same admitted owner definition.
use super::*;
use crate::presentation::{actors::ActorPresentationBatch, equipment};
use assets::{RuntimeEquipmentCatalog, RuntimeIconCatalog};
use equipment::{ActorEquipmentInput, EquipmentAnimation};
use render_model::{ActorArtworkPages, EntityRigId};

#[path = "overlay_tests.rs"]
mod overlays;

#[path = "selection_composition_tests.rs"]
mod selection_composition;

const CHEST: &str = "minecraft:iron_chestplate";
const ARMOR_GEOMETRY: &str = "geometry.fixture.armour";

/// Adds ordinary armour and optionally mapped wings to the original body/cape fixture.
fn variant_files(wings_first: bool, condition: Option<&str>) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = fixture_files(false);
    for (name, identifier, geometry, restriction) in [
        ("armour", CHEST, ARMOR_GEOMETRY, None),
        (
            "wings",
            if wings_first {
                "fixture:wings"
            } else {
                "zfixture:wings"
            },
            assets::ELYTRA_GEOMETRY_IDENTIFIER,
            condition,
        ),
    ] {
        if name == "wings" && restriction.is_none() {
            continue;
        }
        let mut description = serde_json::json!({
            "identifier":identifier,"materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture"},"geometry":{"default":geometry},
            "render_controllers":["controller.render.fixture"]
        });
        if let Some(condition) = restriction {
            description["item"] = serde_json::json!({(CHEST):condition});
        }
        files.push((
            format!("attachables/{name}.json").into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version":"1.10.0","minecraft:attachable":{"description":description}
            }))
            .unwrap(),
        ));
    }
    let mut geometry = serde_json::json!({
        "format_version":"1.12.0","minecraft:geometry":[
            {"description":{"identifier":ARMOR_GEOMETRY,"texture_width":64,"texture_height":32},
             "bones":[{"name":"body","pivot":[0,24,0],
                 "cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]}]},
            {"description":{"identifier":assets::ELYTRA_GEOMETRY_IDENTIFIER,"texture_width":64,"texture_height":32},
             "bones":[{"name":"body","pivot":[0,24,0]},
                 {"name":"wing","parent":"body","pivot":[0,24,0],
                  "cubes":[{"origin":[0,0,0],"size":[10,20,2],"uv":[22,0]}]}]}
        ]
    });
    if condition.is_none() {
        geometry["minecraft:geometry"].as_array_mut().unwrap().pop();
    }
    files.push((
        "models/entity/equipment.json".into(),
        serde_json::to_vec(&geometry).unwrap(),
    ));
    files
}

/// Compiles each authored category and its owner condition without runtime fixture edits.
fn compile_fixture(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> (Arc<RuntimeEntityAssets>, Arc<RuntimeEquipmentCatalog>) {
    let pack = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    let catalog = RuntimeEquipmentCatalog::from_parts(
        pack.identity,
        pack.equipment_bindings,
        pack.equipment_textures,
    )
    .unwrap();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(pack.entities).unwrap()),
        Arc::new(catalog),
    )
}

/// Builds startup equipment and its actual artwork pages for the publication route.
fn build_runtime(
    entities: &Arc<RuntimeEntityAssets>,
    catalog: Option<Arc<RuntimeEquipmentCatalog>>,
) -> (EquipmentRuntime, ActorArtworkPages) {
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&assets::encode_icon_catalog([1; 32], &[], &[]).unwrap())
            .unwrap(),
    );
    let (runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(entities),
        catalog,
        icons,
        None,
        None,
        Default::default(),
    );
    (runtime, pages)
}

/// Installs a replaceable equipment source with pages belonging to that exact catalog.
fn install(
    runtime: &mut EquipmentRuntime,
    pages: &mut ActorArtworkPages,
    entities: Arc<RuntimeEntityAssets>,
    catalog: Arc<RuntimeEquipmentCatalog>,
) {
    let (next, locations) =
        std::mem::take(pages).with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    *pages = next;
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
}

/// Publishes the chest stack through replicated equipment admission and prepares its real cape.
fn equipped_stream(entities: Arc<RuntimeEntityAssets>, state: &mut ActorFrameState) -> WorldStream {
    let mut stream = stream(Arc::clone(&entities));
    let entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == CHEST)
        .unwrap()
        .clone();
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([entry.clone()])
    }));
    stream
        .submit(
            3,
            WorldEvent::ArmorEquipment(Box::new(protocol::ArmorEquipmentEvent {
                actor_runtime_id: 2,
                chestplate: protocol::NetworkItemStack {
                    network_id: entry.network_id,
                    count: 1,
                    ..protocol::NetworkItemStack::empty()
                },
                helmet: Default::default(),
                leggings: Default::default(),
                boots: Default::default(),
                body: Default::default(),
            })),
        )
        .unwrap();
    assert_eq!(
        equipment::remote_input(&stream, 2).armor[1]
            .as_ref()
            .unwrap()
            .identifier
            .as_ref(),
        CHEST
    );
    let cape = match &stream.authority().actor_player_profile(2).unwrap().skin {
        protocol::PlayerSkin::Standard(skin) => skin.cape.clone().unwrap(),
        _ => unreachable!(),
    };
    let (_, preparation) = state.cape_state.rig(Some(&entities)).unwrap();
    preparation.request(&cape);
    preparation.submit();
    preparation.finish_fixture_batch();
    stream
}

/// Runs actual local or remote layer assembly, including cape suppression and texture replacement.
fn publish(
    stream: &WorldStream,
    entities: &RuntimeEntityAssets,
    runtime: &mut EquipmentRuntime,
    pages: &ActorArtworkPages,
    state: &mut ActorFrameState,
    input: &ActorEquipmentInput,
    local: bool,
) -> ActorPresentationBatch {
    let authority = stream.authority();
    let rig = authority.actor_rig(2).unwrap();
    let body = crate::presentation::actors::actor_rig_presentation(
        &rig,
        authority.actor(2).unwrap(),
        None,
        1.0,
    )
    .unwrap();
    let mut batch = if local {
        crate::presentation::actors::select_actor_presentations(2, true, Some(body), [])
    } else {
        crate::presentation::actors::select_actor_presentations(1, false, None, [body])
    };
    let mut frame = authority.actor_render_frame(1.0);
    state.layer_poses.begin_frame();
    layers::attach(
        &mut batch,
        Some(stream),
        Some(&mut frame),
        layers::LayerPoses {
            local_runtime_id: if local { 2 } else { 1 },
            local_equipment: input,
            java_posed: &[],
            combat_visuals: render_api::DEFAULT_COMBAT_VISUALS,
            local_emote_pose: None,
            remote_emote_poses: &Default::default(),
            native_body_sampled: false,
            local_coverage: false,
            remote_skin_layers: &Default::default(),
            native_pose: &state.java_hand.native_pose,
        },
        layers::LayerCaches {
            equipment: Some(runtime),
            layer_poses: &mut state.layer_poses,
            cape_state: &mut state.cape_state,
            skin_layers: &mut state.skin_layers,
            skin_rigs: &mut state.skin_rigs,
        },
        pages,
        Some(entities),
        &mut ActorRenderScene::default(),
        (1.0, 0.05, None),
        &mut Vec::new(),
    );
    batch
}

/// Checks the emitted rig and pose as well as the corresponding cape draw and artwork route.
fn assert_category(batch: &ActorPresentationBatch, expected: Option<EntityRigId>, wings: bool) {
    let worn = batch
        .submissions
        .iter()
        .find(|draw| draw.input.identity.layer == equipment::ELYTRA_LAYER);
    assert_eq!(worn.map(|draw| draw.input.rig), expected);
    if let Some(worn) = worn {
        assert_eq!(worn.input.current_bones.len(), if wings { 2 } else { 1 });
        assert_eq!(batch.artwork.contains_key(&worn.input.identity), !wings);
    }
    assert_eq!(
        batch
            .submissions
            .iter()
            .any(|draw| draw.input.identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE),
        !wings
    );
}

#[test]
fn owner_selected_worn_category_matches_cape_composition_in_both_orders_and_sources() {
    for wings_first in [true, false] {
        for from_pack in [false, true] {
            for local in [false, true] {
                let condition = "q.owner_identifier == 'minecraft:player' && (q.is_local_player == q.is_sneaking)";
                let (entities, catalog) =
                    compile_fixture(variant_files(wings_first, Some(condition)));
                let (mut runtime, mut pages) =
                    build_runtime(&entities, (!from_pack).then(|| Arc::clone(&catalog)));
                if from_pack {
                    install(&mut runtime, &mut pages, Arc::clone(&entities), catalog);
                }
                let mut state = ActorFrameState::default();
                let mut stream = equipped_stream(Arc::clone(&entities), &mut state);
                for (step, sneaking) in [false, true, false].into_iter().enumerate() {
                    stream
                        .submit(
                            4 + step as u64,
                            WorldEvent::Actor(ActorEvent::Metadata(
                                protocol::ActorMetadataUpdateEvent {
                                    runtime_id: 2,
                                    dimension: 0,
                                    tick: step as u64,
                                    metadata: Arc::from([protocol::ActorMetadata {
                                        key: protocol_ids::ActorDataKey::Flags.wire_value(),
                                        value: protocol::ActorMetadataValue::Flags(if sneaking {
                                            1 << protocol_ids::ActorFlag::Sneaking.wire_value()
                                        } else {
                                            0
                                        }),
                                    }]),
                                    properties: Arc::from([]),
                                },
                            )),
                        )
                        .unwrap();
                    stream.advance_actor_interpolation_ticks(1);
                    let input = ActorEquipmentInput {
                        is_local_player: true,
                        ..equipment::remote_input(&stream, 2)
                    };
                    let wings = local == sneaking;
                    let index = render_model::find_geometry_index(
                        &entities,
                        if wings {
                            assets::ELYTRA_GEOMETRY_IDENTIFIER
                        } else {
                            ARMOR_GEOMETRY
                        },
                    )
                    .unwrap();
                    let expected = if from_pack {
                        render_model::pack_equipment_rig_id(index)
                    } else {
                        render_model::equipment_rig_id(index)
                    };
                    for _ in 0..2 {
                        let batch = publish(
                            &stream,
                            &entities,
                            &mut runtime,
                            &pages,
                            &mut state,
                            &input,
                            local,
                        );
                        assert_category(&batch, Some(expected), wings);
                    }
                }
            }
        }
    }
}

#[test]
fn source_replacement_rejection_and_missing_geometry_restore_ordinary_armour_and_cape() {
    let (base, base_catalog) = compile_fixture(variant_files(true, None));
    let (mut runtime, mut pages) = build_runtime(&base, Some(base_catalog));
    let mut state = ActorFrameState::default();
    let mut stream = equipped_stream(Arc::clone(&base), &mut state);
    stream.advance_actor_interpolation_ticks(1);
    let input = ActorEquipmentInput {
        is_local_player: true,
        ..equipment::remote_input(&stream, 2)
    };
    for mode in [0, 1, 2, 3, 0] {
        let expected = if mode == 3 {
            runtime.set_pack_layer(None);
            render_model::equipment_rig_id(
                render_model::find_geometry_index(&base, ARMOR_GEOMETRY).unwrap(),
            )
        } else {
            let mut files = variant_files(mode == 0, Some(if mode == 1 { "0" } else { "1" }));
            if mode != 0 {
                files.retain(|(path, _)| path.as_ref() != "attachables/armour.json");
            }
            if mode == 2 {
                let (_, bytes) = files
                    .iter_mut()
                    .find(|(path, _)| path.as_ref() == "models/entity/equipment.json")
                    .unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                json["minecraft:geometry"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|geometry| {
                        geometry["description"]["identifier"] != assets::ELYTRA_GEOMETRY_IDENTIFIER
                    });
                *bytes = serde_json::to_vec(&json).unwrap();
            }
            let (entities, catalog) = compile_fixture(files);
            let expected = if mode == 0 {
                render_model::pack_equipment_rig_id(
                    render_model::find_geometry_index(
                        &entities,
                        assets::ELYTRA_GEOMETRY_IDENTIFIER,
                    )
                    .unwrap(),
                )
            } else {
                render_model::equipment_rig_id(
                    render_model::find_geometry_index(&base, ARMOR_GEOMETRY).unwrap(),
                )
            };
            install(&mut runtime, &mut pages, Arc::clone(&entities), catalog);
            expected
        };
        let batch = publish(
            &stream,
            &base,
            &mut runtime,
            &pages,
            &mut state,
            &input,
            true,
        );
        assert_category(&batch, Some(expected), mode == 0);
    }
}

#[test]
fn ownerless_preview_and_unknown_stacks_do_not_admit_mapped_wing_categories() {
    for wings_first in [true, false] {
        for ordinary in [false, true] {
            let mut files = variant_files(wings_first, Some("1"));
            if !ordinary {
                files.retain(|(path, _)| path.as_ref() != "attachables/armour.json");
            }
            let (entities, catalog) = compile_fixture(files);
            let (mut runtime, pages) = build_runtime(&entities, Some(catalog));
            let mut state = ActorFrameState::default();
            let mut stream = equipped_stream(Arc::clone(&entities), &mut state);
            stream.advance_actor_interpolation_ticks(1);
            let mut input = equipment::remote_input(&stream, 2);
            let authority = stream.authority();
            let rig = authority.actor_rig(2).unwrap();
            let body = crate::presentation::actors::actor_rig_presentation(
                &rig,
                authority.actor(2).unwrap(),
                None,
                1.0,
            )
            .unwrap();
            let expected = render_model::equipment_rig_id(
                render_model::find_geometry_index(&entities, ARMOR_GEOMETRY).unwrap(),
            );
            let drawn = runtime.layers_for(&body.submission, &input, None);
            assert_eq!(drawn.len(), usize::from(ordinary));
            assert_eq!(
                drawn.first().map(|draw| draw.submission.input.rig),
                ordinary.then_some(expected)
            );
            assert!(
                !runtime
                    .layers_for_profile(
                        &body.submission,
                        &input,
                        render_api::DEFAULT_COMBAT_VISUALS,
                        None,
                    )
                    .wearing_elytra
            );
            let owned = publish(
                &stream,
                &entities,
                &mut runtime,
                &pages,
                &mut state,
                &input,
                true,
            );
            assert_category(
                &owned,
                Some(render_model::equipment_rig_id(
                    render_model::find_geometry_index(
                        &entities,
                        assets::ELYTRA_GEOMETRY_IDENTIFIER,
                    )
                    .unwrap(),
                )),
                true,
            );
            input.armor[1].as_mut().unwrap().identifier = "fixture:unknown_chest".into();
            assert!(
                runtime
                    .layers_for(&body.submission, &input, None)
                    .is_empty()
            );
            assert!(
                !runtime
                    .layers_for_profile(
                        &body.submission,
                        &input,
                        render_api::DEFAULT_COMBAT_VISUALS,
                        None,
                    )
                    .wearing_elytra
            );
            assert!(
                !runtime
                    .layers_for_profile(
                        &body.submission,
                        &input,
                        render_api::DEFAULT_COMBAT_VISUALS,
                        Some(EquipmentAnimation {
                            owner: authority.actor(2).unwrap(),
                            rig: &rig,
                            frame_alpha: 1.0,
                            delta_seconds: 0.05,
                        })
                    )
                    .wearing_elytra
            );
            let batch = publish(
                &stream,
                &entities,
                &mut runtime,
                &pages,
                &mut state,
                &input,
                true,
            );
            assert_category(&batch, None, false);
        }
    }
}
