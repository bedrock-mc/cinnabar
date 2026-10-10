use super::*;
use protocol::block_state_network_hash;

/// Supplies a local pose without server-authored health attributes.
fn local_health_feed() -> crate::LocalPlayerFeed {
    crate::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [0; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Standard(protocol::StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: vec![0; 64 * 64 * 4].into(),
        }),
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: crate::ACTOR_SWING_TICKS,
        java_swing_ticks: crate::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

#[test]
fn health_drops_animate_only_after_player_spawn_and_count_completed_actor_ticks() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority.sync_local_player_pose(&local_health_feed());
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 16 })),
            Some(1),
        )
        .unwrap();
    assert_eq!(authority.actor(1).unwrap().status.hurt_time, 0);
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::PlayerStatus(
                protocol::PlayerStatus::PlayerSpawn,
            ))),
            Some(2),
        )
        .unwrap();
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 7 })),
            Some(3),
        )
        .unwrap();
    assert_eq!(
        authority.actor(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS
    );
    assert_eq!(
        authority.actor(1).unwrap().status.damage.previous_health,
        16.0
    );
    assert!(authority.actor(1).unwrap().status.damage.flash_active());
    authority.advance_actor_interpolation_ticks(0);
    assert_eq!(
        authority.actor(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS
    );
    authority.advance_actor_interpolation_ticks(3);
    assert_eq!(
        authority.actor(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS - 3
    );
    assert!(!authority.actor(1).unwrap().status.damage.flash_active());
    assert_eq!(
        authority.actor(1).unwrap().status.damage.remaining_ticks,
        crate::HURT_DURATION_TICKS - 3
    );
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 7 })),
            Some(4),
        )
        .unwrap();
    assert_eq!(
        authority.actor(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS - 3
    );
}

#[test]
fn set_health_before_the_first_local_pose_survives_actor_creation() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 0 })),
            Some(1),
        )
        .unwrap();
    authority.sync_local_player_pose(&local_health_feed());
    assert!(authority.actor(1).unwrap().status.dead);
    authority.advance_actor_interpolation_ticks(4);
    assert_eq!(authority.actor(1).unwrap().status.native_death_ticks(), 4);
    authority.sync_local_player_pose(&local_health_feed());
    assert_eq!(authority.actor(1).unwrap().status.native_death_ticks(), 4);

    authority.reset_dimension(2, 1);
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Attributes(
                protocol::ActorAttributesUpdateEvent {
                    dimension: 1,
                    runtime_id: 1,
                    tick: 5,
                    attributes: Arc::from([protocol::ActorAttribute {
                        name: "minecraft:health".into(),
                        min: 0.0,
                        max: 40.0,
                        current: 7.0,
                        default: None,
                        modifiers: Arc::from([]),
                    }]),
                },
            )),
            Some(3),
        )
        .unwrap();
    authority.sync_local_player_pose(&local_health_feed());
    let actor = authority.actor(1).unwrap();
    assert!(!actor.status.dead);
    assert_eq!(actor.attributes["minecraft:health"].current, 7.0);
    assert_eq!(actor.attributes["minecraft:health"].max, 40.0);
}

#[test]
fn set_health_updates_the_local_actor_death_clock_and_preserves_ui_delivery() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: protocol::ActorKind::Player {
                    uuid: [0; 16],
                    username: "Player".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([protocol::ActorAttribute {
                    name: "minecraft:health".into(),
                    min: 0.0,
                    max: crate::DEFAULT_PLAYER_HEALTH,
                    current: crate::DEFAULT_PLAYER_HEALTH,
                    default: None,
                    modifiers: Arc::from([]),
                }]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    for (sequence, health) in [(2, 0), (3, 7)] {
        authority
            .apply_ordered_event(
                WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health })),
                Some(sequence),
            )
            .unwrap();
        let actor = authority.actor(1).unwrap();
        assert_eq!(actor.status.dead, health == 0);
        assert_eq!(actor.attributes["minecraft:health"].current, health as f32);
        assert_eq!(actor.status.native_death_ticks(), 0);
        authority.advance_actor_interpolation_ticks(3);
        assert_eq!(
            authority.actor(1).unwrap().status.native_death_ticks(),
            if health == 0 { 3 } else { 0 }
        );
    }
    assert!(matches!(
        authority.take_committed_ui().as_slice(),
        [
            CommittedUiEvent::Ui {
                sequence: 2,
                event: UiEvent::Hud(protocol::HudEvent::Health { health: 0 })
            },
            CommittedUiEvent::Ui {
                sequence: 3,
                event: UiEvent::Hud(protocol::HudEvent::Health { health: 7 })
            },
        ]
    ));
}

#[test]
fn death_and_hud_rules_commit_in_one_ui_envelope() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    let hud = protocol::HudRules {
        show_coordinates: Some(true),
        show_days_played: None,
    };
    let death = protocol::DeathRules {
        show_messages: Some(false),
        immediate_respawn: Some(true),
    };
    authority
        .apply_ordered_event(
            WorldEvent::GameRules(protocol::GameRulesEvent {
                daylight_cycle: None,
                weather_cycle: None,
                hud,
                death,
            }),
            Some(7),
        )
        .unwrap();
    assert!(matches!(authority.take_committed_ui().as_slice(),
        [CommittedUiEvent::Ui { sequence: 7, event: UiEvent::GameRules { hud: found_hud, death: found_death } }]
        if *found_hud == hud && *found_death == death));
}

#[test]
fn block_interactions_encode_the_current_session_palette() {
    for hashes in [false, true] {
        let mut authority = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashes,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        );
        authority
            .set_sequential_id_remap(assets::SequentialIdRemap::from_palette(vec![0, 4, 1, 6], 7));
        if hashes {
            assert_eq!(authority.block_network_id(4), Some(4));
            assert_eq!(authority.block_network_id(0x8000_0001), Some(0x8000_0001));
            assert_eq!(authority.block_network_id(u32::MAX), Some(u32::MAX));
        } else {
            assert_eq!(authority.block_network_id(4), Some(1));
            assert_eq!(authority.block_network_id(6), Some(3));
            assert_eq!(authority.block_network_id(2), None);
            authority.set_sequential_id_remap(assets::SequentialIdRemap::default());
            assert_eq!(authority.block_network_id(4), Some(4));
        }
    }
}

#[test]
fn credits_admission_targets_the_live_local_runtime_actor() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 5,
            local_player_runtime_id: 41,
            dimension: 2,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    for (sequence, runtime_id) in [(7, 72), (8, 41)] {
        authority
            .apply_ordered_event(
                WorldEvent::Ui(UiEvent::ShowCredits(protocol::ShowCreditsEvent {
                    runtime_id,
                })),
                Some(sequence),
            )
            .unwrap();
    }
    let events = authority.take_committed_ui();
    assert!(matches!(
        events.as_slice(),
        [CommittedUiEvent::Ui {
            sequence: 8,
            event: UiEvent::ShowCredits(protocol::ShowCreditsEvent { runtime_id: 41 })
        }]
    ));
}

#[test]
fn biome_tint_revision_overflow_keeps_the_previous_atomic_snapshot() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority.biome_tint_revision = u64::MAX;
    let previous = Arc::clone(authority.resolved_biome_tints());

    let report = authority.apply_biome_definitions(Arc::from([BiomeDefinitionEvent {
        biome_id: Some(42),
        name: Arc::from("example:overflow"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    }]));

    assert_eq!(authority.biome_tint_revision(), u64::MAX);
    assert!(authority.biome_definitions().is_empty());
    assert!(Arc::ptr_eq(&previous, authority.resolved_biome_tints()));
    assert!(report.revision_overflow);
    assert!(!report.changed);
    assert_eq!(report.resolution_failures, 0);
}

#[test]
fn persistent_custom_states_decode_before_visual_overlay_is_ready() {
    use world::{BlockIds, NbtCompound, NbtValue, SubChunk};

    let definitions = CustomBlocks {
        blocks: Arc::from([
            CustomBlock {
                state_physics: Default::default(),
                name: "example:plain".into(),
                tags: Default::default(),
                state_count: 1,
                collides: true,
                collision_boxes: None,
                selection: CustomSelection::Default,
                visual: Arc::default(),
            },
            CustomBlock {
                state_physics: Default::default(),
                name: "example:powered".into(),
                tags: Default::default(),
                state_count: 2,
                collides: true,
                collision_boxes: None,
                selection: CustomSelection::Default,
                visual: Arc::new(CustomBlockVisuals {
                    state_axes: Box::new([CustomStateAxis {
                        name: "custom:powered".into(),
                        values: Box::new([
                            CustomStateValue::Bool(false),
                            CustomStateValue::Bool(true),
                        ]),
                    }]),
                    ..Default::default()
                }),
            },
        ]),
        skipped: 0,
        ..Default::default()
    };
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut authority = custom_identity_authority(mode);
        authority.set_custom_block_ids(if mode == NetworkIdMode::Sequential {
            1..4
        } else {
            0..0
        });
        authority.set_sequential_id_remap(assets::SequentialIdRemap::new([(0, 1, 4)]));
        authority.set_custom_block_identities(&definitions);
        let ids = authority.decode_ids(0);
        assert_eq!(ids.assets.visual_count(), 1);
        assert!(!ids.assets.is_diagnostic());
        for (block, state_index, internal_id) in [(0, 0, 1), (1, 1, 3)] {
            let definition = &definitions.blocks[block];
            let state = &definition.hashed_states()[state_index];
            let mut entry = NbtCompound::default();
            entry.insert("name", NbtValue::String(definition.name.to_string().into()));
            if block == 1 {
                let mut states = NbtCompound::default();
                states.insert("custom:powered", NbtValue::Byte(1));
                entry.insert("states", NbtValue::Compound(states));
            }
            let expected = if mode == NetworkIdMode::Sequential {
                internal_id
            } else {
                state.hash
            };
            assert_ne!(expected, ids.air);
            assert_eq!(ids.resolve_persistent(&entry), expected);
            if mode == NetworkIdMode::Hashed {
                assert!(!ids.assets.is_known(mode, expected));
                assert_eq!(ids.resolve(expected), expected);
            }
            let mut payload = vec![8, 1, 0];
            payload.extend(entry.encode_root().unwrap());
            let decoded = SubChunk::decode(&payload, &ids);
            assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(expected));
        }
    }
}

fn custom_identity_authority(mode: NetworkIdMode) -> WorldAuthority {
    use assets::{
        BlobProvenance, BlockFlags, BlockVisual, CompiledAssets, ContributorRole, LightProperties,
        NO_ANIMATION, NO_MODEL_TEMPLATE, VisualKind, VisualSupport,
    };
    let diagnostic = RuntimeAssets::diagnostic();
    let compiled = CompiledAssets {
        visuals: Box::new([BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::AIR,
            kind: VisualKind::Invisible,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Air,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }]),
        light_properties: Box::new([LightProperties::default()]),
        hashed: Box::new([(HASHED_AIR_NETWORK_ID, 0)]),
        materials: diagnostic.materials().into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: diagnostic.texture_pages().into(),
        biomes: diagnostic.biome_assets().clone(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: mode == NetworkIdMode::Hashed,
        },
        Arc::new(RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()),
        None,
        [0.0; 3],
        None,
    )
}

fn plain_identity_block(name: &str) -> CustomBlock {
    CustomBlock {
        state_physics: Default::default(),
        name: name.into(),
        tags: Default::default(),
        state_count: 1,
        collides: true,
        collision_boxes: None,
        selection: CustomSelection::Default,
        visual: Arc::default(),
    }
}

fn plain_identity_entry(name: &str) -> world::NbtCompound {
    let mut entry = world::NbtCompound::default();
    entry.insert("name", world::NbtValue::String(name.into()));
    entry
}

#[test]
fn custom_identity_snapshots_preserve_incomplete_offsets_and_survive_replacement() {
    use world::BlockIds;

    let mut incomplete = plain_identity_block("example:incomplete");
    incomplete.state_count = 2;
    incomplete.visual = Arc::new(CustomBlockVisuals {
        state_identity_incomplete: true,
        ..Default::default()
    });
    let definitions = CustomBlocks {
        blocks: Arc::from([
            plain_identity_block("example:first"),
            incomplete,
            plain_identity_block("example:last"),
        ]),
        skipped: 0,
        ..Default::default()
    };
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut authority = custom_identity_authority(mode);
        authority.set_custom_block_ids(if mode == NetworkIdMode::Sequential {
            10..14
        } else {
            0..0
        });
        authority.set_custom_block_identities(&definitions);
        let captured = authority.decode_ids(0);
        for (name, sequential) in [("example:first", 10), ("example:last", 13)] {
            let entry = plain_identity_entry(name);
            let expected = if mode == NetworkIdMode::Sequential {
                sequential
            } else {
                block_state_network_hash(name, std::iter::empty())
            };
            assert_eq!(captured.resolve_persistent(&entry), expected);
        }
        assert_eq!(
            captured.resolve_persistent(&plain_identity_entry("example:incomplete")),
            captured.air(),
        );
        authority.replace_runtime_assets(Arc::new(RuntimeAssets::diagnostic()));
        authority.set_custom_block_identities(&CustomBlocks::default());
        let entry = plain_identity_entry("example:last");
        assert_ne!(captured.resolve_persistent(&entry), captured.air());
        let fresh = authority.decode_ids(0);
        assert_eq!(fresh.resolve_persistent(&entry), fresh.air());
    }
}

#[test]
fn custom_identity_registry_stops_at_admitted_range_and_offset_overflow() {
    use world::BlockIds;

    let mut authority = custom_identity_authority(NetworkIdMode::Sequential);
    let definitions = CustomBlocks {
        blocks: Arc::from([
            plain_identity_block("example:first"),
            plain_identity_block("example:last"),
        ]),
        skipped: 0,
        ..Default::default()
    };
    authority.set_custom_block_ids(10..11);
    authority.set_custom_block_identities(&definitions);
    let bounded = authority.decode_ids(0);
    assert_eq!(
        bounded.resolve_persistent(&plain_identity_entry("example:first")),
        10,
    );
    assert_eq!(
        bounded.resolve_persistent(&plain_identity_entry("example:last")),
        bounded.air(),
    );

    let mut overflowing = plain_identity_block("example:overflow");
    overflowing.state_count = u32::MAX;
    let overflowing = CustomBlocks {
        blocks: Arc::from([overflowing, plain_identity_block("example:after_overflow")]),
        skipped: 0,
        ..Default::default()
    };
    authority.set_custom_block_ids(10..12);
    authority.set_custom_block_identities(&overflowing);
    let exhausted = authority.decode_ids(0);
    assert_eq!(
        exhausted.resolve_persistent(&plain_identity_entry("example:after_overflow")),
        exhausted.air(),
    );
}
