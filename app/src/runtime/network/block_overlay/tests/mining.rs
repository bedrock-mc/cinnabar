use super::*;

#[test]
fn custom_mining_prepared_state_overrides_reach_both_overlay_modes() {
    let mut definition = block(
        "test:state_mining",
        2,
        CustomBlockVisuals {
            state_axes: [protocol::CustomStateAxis {
                name: "test:fast".into(),
                values: [false, true].map(protocol::CustomStateValue::Bool).into(),
            }]
            .into(),
            permutations: [protocol::CustomPermutation {
                condition: "q.block_state('test:fast')".into(),
                components: Default::default(),
                physical: protocol::CustomPhysicalComponents {
                    mining: Some(Arc::new(protocol::CustomBlockMining {
                        seconds_to_destroy: 0.5,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            }]
            .into(),
            ..Default::default()
        },
    );
    definition.mining = Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 4.0,
        ..Default::default()
    });
    let data = protocol::GameData {
        custom_blocks: CustomBlocks {
            blocks: [definition].into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let prepared = pack_runtime::prepare_session_packs(
        protocol::ResourcePackHandoff::from_archives(vec![]),
        &data,
        u64::MAX,
    );
    for hashed in [false, true] {
        let compiled =
            compile_block_overlay(&view(), &prepared.inputs.blocks, hashed, None).unwrap();
        assert_eq!(compiled.overlay.visuals.len(), 2);
        for (visual, rate) in compiled
            .overlay
            .visuals
            .iter()
            .zip([1.0 / 120.0, 1.0 / 15.0])
        {
            let info = sim::BlockDestroyInfo::from_facts(visual.destroy).unwrap();
            assert_eq!(
                sim::destroy_progress_per_tick(
                    &info,
                    &sim::DestroyConditions {
                        on_ground: true,
                        ..Default::default()
                    }
                ),
                Some(rate)
            );
        }
    }
}

#[test]
fn custom_mining_discarded_data_stays_unresolved_in_shared_materials() {
    let mut incomplete = block("test:incomplete_mining", 1, CustomBlockVisuals::default());
    Arc::make_mut(&mut incomplete.visual).mining_incomplete = true;
    let blocks = CustomBlocks {
        blocks: [
            incomplete,
            block("test:complete_mining", 1, CustomBlockVisuals::default()),
        ]
        .into(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &blocks, hashed, None).unwrap();
        assert!(sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[0].destroy).is_none());
        let known = sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[1].destroy).unwrap();
        assert_eq!(
            sim::destroy_progress_per_tick(&known, &Default::default()),
            Some(1.0)
        );
    }
}

#[test]
fn custom_mining_unresolved_permutation_cannot_use_an_instant_base() {
    let block = block(
        "test:unknown_mining",
        1,
        CustomBlockVisuals {
            permutations: [protocol::CustomPermutation {
                condition: "return 1;".into(),
                components: Default::default(),
                physical: protocol::CustomPhysicalComponents {
                    mining: Some(Arc::new(protocol::CustomBlockMining {
                        seconds_to_destroy: -1.0,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            }]
            .into(),
            ..Default::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: [block].into(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &blocks, hashed, None).unwrap();
        assert!(sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[0].destroy).is_none());
    }
}

/// Encodes a named tag in a small network NBT fixture.
fn named(tag: u8, name: &str, bytes: &mut Vec<u8>) {
    bytes.extend([tag, u8::try_from(name.len()).unwrap()]);
    bytes.extend(name.as_bytes());
}

#[test]
fn custom_mining_wire_seconds_complete_after_the_expected_ticks() {
    use gameplay::survival_mining::{
        BlockBreakingAuthority, DestroyInput, DestroyMachine, DestroyTarget, TickMotion,
    };
    let mut bytes = Vec::new();
    named(10, "", &mut bytes);
    named(10, "components", &mut bytes);
    named(10, "minecraft:destructible_by_mining", &mut bytes);
    named(5, "seconds_to_destroy", &mut bytes);
    bytes.extend(2.0_f32.to_le_bytes());
    bytes.extend([0, 0, 0]);
    let blocks = CustomBlocks::from_definitions([("test:mineable", bytes.as_slice())]);
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    let item = protocol::NetworkItemStack::empty();
    let target = DestroyTarget {
        local_runtime_id: 42,
        position: [0; 3],
        face: 1,
        runtime_id: 1,
        wire_runtime_id: 1,
        relative_hit: [0.5; 3],
        block: sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[0].destroy),
        conditions: sim::DestroyConditions::default(),
        selection: gameplay::mining::FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(item.clone(), item.nbt_digest)
                .unwrap(),
        },
        wear: None,
        instant: false,
    };
    let mut machine = DestroyMachine::default();
    let motion = TickMotion {
        on_ground: true,
        moved: 0.0,
    };
    machine.step(
        DestroyInput::Held(Some(&target)),
        motion,
        BlockBreakingAuthority::Server,
    );
    for tick in 1..=60 {
        let payload = machine.step(
            DestroyInput::Held(Some(&target)),
            motion,
            BlockBreakingAuthority::Server,
        );
        assert_eq!(
            payload
                .actions
                .iter()
                .any(|action| action.kind == protocol::BlockActionKind::PredictDestroy),
            tick == 60
        );
    }
}

#[test]
fn custom_mining_omitted_component_has_resolved_default() {
    let blocks = CustomBlocks {
        blocks: Arc::from([block("test:mineable", 1, CustomBlockVisuals::default())]),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    let info = sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[0].destroy)
        .expect("a custom block without a mining component has a resolved default");
    assert_eq!(
        sim::destroy_progress_per_tick(&info, &sim::DestroyConditions::default()),
        Some(1.0)
    );
}

#[test]
fn custom_mining_seconds_survive_overlay_deduplication_and_both_id_modes() {
    let blocks = CustomBlocks {
        blocks: [2.0, 0.5, -1.0]
            .map(|seconds| {
                let mut block = block(
                    &format!("test:mining_{}", seconds),
                    1,
                    CustomBlockVisuals::default(),
                );
                block.mining = Arc::new(protocol::CustomBlockMining {
                    seconds_to_destroy: seconds,
                    ..Default::default()
                });
                block
            })
            .into(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &blocks, hashed, None).unwrap();
        for (visual, rate) in
            compiled
                .overlay
                .visuals
                .iter()
                .zip([Some(1.0 / 60.0), Some(1.0 / 15.0), None])
        {
            let info = sim::BlockDestroyInfo::from_facts(visual.destroy).unwrap();
            assert_eq!(
                sim::destroy_progress_per_tick(
                    &info,
                    &sim::DestroyConditions {
                        on_ground: true,
                        ..Default::default()
                    }
                ),
                rate
            );
        }
    }
}

#[test]
fn custom_mining_declared_tool_tags_survive_the_overlay() {
    let mut bytes = Vec::new();
    named(10, "", &mut bytes);
    named(9, "blockTags", &mut bytes);
    bytes.extend([8, 2]);
    let tag = "minecraft:is_pickaxe_item_destructible";
    bytes.push(u8::try_from(tag.len()).unwrap());
    bytes.extend(tag.as_bytes());
    bytes.push(0);
    let mut blocks = CustomBlocks::from_definitions([("test:mineable", bytes.as_slice())]);
    Arc::make_mut(&mut blocks.blocks)[0].mining = Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 2.0,
        ..Default::default()
    });
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &blocks, hashed, None).unwrap();
        let info = sim::BlockDestroyInfo::from_facts(compiled.overlay.visuals[0].destroy).unwrap();
        let conditions = sim::DestroyConditions {
            on_ground: true,
            tool: Some(sim::HeldTool {
                kind: assets::compiled::block_destroy::ToolKind::Pickaxe,
                tier: Some(assets::facts::ItemTier::Diamond),
            }),
            ..Default::default()
        };
        assert_eq!(
            sim::destroy_progress_per_tick(&info, &conditions),
            Some(8.0 / 60.0)
        );
    }
}
