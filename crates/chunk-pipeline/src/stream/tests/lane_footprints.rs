//! Every event kind's footprint names every ordered consumer its commit touches.

use super::*;
use client_world::ingestion::{Consumers, classify};
use protocol::{
    AbilitiesUpdate, AbilityLayersEvidence, ActorActionEvent, ActorActionKind, ActorEffectAction,
    ActorEffectEvent, ActorPropertySyncEvent, ActorStatusEvent, ActorStatusKind,
    ActorTakeItemEvent, ArmorEquipmentEvent, AudioEvent, BlockEventEvent, CameraEvent,
    CameraShakeAction, CameraShakeEvent, CameraShakeType, DaylightCycleUpdateEvent, EquipmentEvent,
    ExperienceMessage, GameModeEvent, GameModeUpdate, GameRulesEvent, ItemActorEvent,
    ItemRegistryEvent, LevelAudioEvent, MapDataEvent, MovementEffectEvent, MovementEffectKind,
    OpenSignEvent, ParticleEvent, PlayerGameMode, PrimitiveShapesEvent, ShowCreditsEvent,
    SubChunkReplyAdmissionEvent, SyncedBlockUpdateEvent, WorldClockState, WorldClockUpdateEvent,
};

const LOCAL: u64 = 1;
const REMOTE: u64 = 9;
const ITEM: u64 = 10;

/// Wire kinds; adding a `WorldEvent` variant fails to compile here until it gets a sample.
const KINDS: usize = 46;

fn kind(event: &WorldEvent) -> usize {
    match event {
        WorldEvent::DimensionHeights(_) => 0,
        WorldEvent::Experience(_) => 1,
        WorldEvent::Abilities(_) => 2,
        WorldEvent::BiomeDefinitions(_) => 3,
        WorldEvent::LevelChunk(_) => 4,
        WorldEvent::ChunkResync(_) => 5,
        WorldEvent::SubChunkReplyAdmission(_) => 6,
        WorldEvent::SubChunks(_) => 7,
        WorldEvent::BlockUpdates(_) => 8,
        WorldEvent::SyncedBlockUpdates(_) => 9,
        WorldEvent::BlockEntityUpdate(_) => 10,
        WorldEvent::BlockEvent(_) => 11,
        WorldEvent::MapData(_) => 12,
        WorldEvent::OpenSign(_) => 13,
        WorldEvent::ChunkRadiusUpdated(_) => 14,
        WorldEvent::PublisherUpdate(_) => 15,
        WorldEvent::ChangeDimension(_) => 16,
        WorldEvent::DimensionChangeAck { .. } => 17,
        WorldEvent::Respawn(_) => 18,
        WorldEvent::MovePlayer(_) => 19,
        WorldEvent::PlayerMovementCorrection(_) => 20,
        WorldEvent::ActorMotion(_) => 21,
        WorldEvent::MovementEffect(_) => 22,
        WorldEvent::NetworkStackLatency(_) => 23,
        WorldEvent::SetTime(_) => 24,
        WorldEvent::WorldClocks(_) => 25,
        WorldEvent::GameRules(_) => 26,
        WorldEvent::Weather(_) => 27,
        WorldEvent::Audio(_) => 28,
        WorldEvent::Camera(_) => 29,
        WorldEvent::Actor(event) => match event {
            ActorEvent::Identifiers(_) => 30,
            ActorEvent::Spawn(_) => 31,
            ActorEvent::PlayerSpawn { .. } => 32,
            ActorEvent::Remove(_) => 33,
            ActorEvent::Move(_) => 34,
            ActorEvent::Metadata(_) => 35,
            ActorEvent::Attributes(_) => 36,
            ActorEvent::PlayerList(_) => 37,
            ActorEvent::Skin { .. } => 38,
            ActorEvent::Status(_) => 39,
            ActorEvent::TakeItem(_) => 40,
        },
        WorldEvent::ActorEffect(_) => 41,
        WorldEvent::ActorLink(_) => 42,
        WorldEvent::ActorPropertySync(_) => 43,
        WorldEvent::Ui(_) => 44,
        WorldEvent::BlockCrack(_) => 45,
        WorldEvent::Equipment(_)
        | WorldEvent::ArmorEquipment(_)
        | WorldEvent::Inventory(_)
        | WorldEvent::ItemActor(_)
        | WorldEvent::Particle(_)
        | WorldEvent::PrimitiveShapes(_) => usize::MAX,
    }
}

/// Item-actor, equipment, particle and shape kinds share the tail of the index space.
fn tail_kind(event: &WorldEvent) -> Option<usize> {
    match event {
        WorldEvent::Equipment(_) => Some(0),
        WorldEvent::ArmorEquipment(_) => Some(1),
        WorldEvent::Inventory(_) => Some(2),
        WorldEvent::ItemActor(_) => Some(3),
        WorldEvent::Particle(_) => Some(4),
        WorldEvent::PrimitiveShapes(_) => Some(5),
        _ => None,
    }
}
const TAIL_KINDS: usize = 6;

fn item_spawn(runtime_id: u64) -> WorldEvent {
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:apple")
        .unwrap()
        .network_id;
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: runtime_id as i64,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:item".into(),
        },
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: protocol::NetworkItemStack {
            network_id,
            count: 1,
            ..Default::default()
        },
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

fn mob_spawn(runtime_id: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: runtime_id as i64,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:pig".into(),
        },
        position: [4.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

fn player_spawn(runtime_id: u64) -> WorldEvent {
    let WorldEvent::Actor(ActorEvent::Spawn(mut spawn)) = mob_spawn(runtime_id) else {
        unreachable!("mob spawns are actor spawns");
    };
    spawn.kind = ActorKind::Player {
        username: "player".into(),
        uuid: [1; 16],
    };
    WorldEvent::Actor(ActorEvent::PlayerSpawn {
        spawn,
        game_mode: GameModeUpdate::Explicit(PlayerGameMode::Survival),
    })
}

fn level_sound(fire_at_position: Option<[f32; 3]>) -> WorldEvent {
    WorldEvent::Audio(AudioEvent::Level(LevelAudioEvent {
        sound_event: "pop".into(),
        position: [1.0, 2.0, 3.0],
        data: -1,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: REMOTE as i64,
        fire_at_position,
    }))
}

fn samples() -> Vec<WorldEvent> {
    let status = |runtime_id, kind| {
        WorldEvent::Actor(ActorEvent::Status(ActorStatusEvent {
            runtime_id,
            kind,
            data: 0,
        }))
    };
    let attributes = |runtime_id| {
        WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
            dimension: 0,
            runtime_id,
            attributes: Arc::from([ActorAttribute {
                name: Arc::from("minecraft:movement"),
                min: 0.0,
                max: 1.0,
                current: 0.1,
                default: Some(0.1),
                modifiers: Arc::from([]),
            }]),
            tick: 1,
        }))
    };
    let metadata = |runtime_id| {
        WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: protocol::ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
            tick: 1,
        }))
    };
    let motion = |actor_runtime_id| {
        WorldEvent::ActorMotion(ActorMotionEvent {
            actor_runtime_id,
            motion: [0.1, 0.2, 0.3],
            tick: 1,
        })
    };
    let movement_effect = |actor_runtime_id| {
        WorldEvent::MovementEffect(MovementEffectEvent {
            actor_runtime_id,
            kind: MovementEffectKind::GlideBoost,
            duration_ticks: 20,
            tick: 1,
        })
    };
    let effect = |actor_runtime_id| {
        WorldEvent::ActorEffect(ActorEffectEvent {
            dimension: 0,
            actor_runtime_id,
            action: ActorEffectAction::Add,
            effect_id: 1,
            amplifier: 0,
            particles: true,
            ambient: false,
            duration_ticks: 20,
            tick: 1,
        })
    };
    let abilities = |actor_unique_id| {
        WorldEvent::Abilities(AbilitiesUpdate {
            actor_unique_id,
            player_permission: 1,
            command_permission: 0,
            layers: AbilityLayersEvidence::Unavailable { declared_layers: 0 },
        })
    };
    let move_player = |mode| {
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: LOCAL,
            position: [0.5, 80.0, 0.5],
            mode,
            ..Default::default()
        })
    };
    let action = |kind| {
        WorldEvent::ItemActor(ItemActorEvent::Action(ActorActionEvent {
            actor_runtime_ids: Arc::from([REMOTE]),
            kind,
            data: 1.0,
            swing_source: None,
        }))
    };
    vec![
        WorldEvent::DimensionHeights(Vec::new()),
        WorldEvent::Experience(ExperienceMessage { bytes: Vec::new() }),
        abilities(LOCAL as i64),
        abilities(REMOTE as i64),
        WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
            definitions: Arc::from([]),
        }),
        inline_air_event(1),
        WorldEvent::ChunkResync(ChunkResyncEvent {
            dimension: 0,
            x: 0,
            z: 0,
            requested_sub_chunks: Some(1),
            requested_sub_chunk_ys: None,
        }),
        WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
            dimension: 0,
            positions: vec![[0, -4, 0]],
        }),
        WorldEvent::SubChunks(SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                diagnostics: None,
                position: [0, -3, 0],
                result: SubChunkResult::AllAir,
            }],
        }),
        WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: [0, -64, 0],
            layer: 0,
            network_id: 1,
        }]),
        WorldEvent::SyncedBlockUpdates(vec![SyncedBlockUpdateEvent {
            update: BlockUpdateEvent {
                dimension: 0,
                position: [0, -64, 0],
                layer: 0,
                network_id: 1,
            },
            flags: 0,
            sync: protocol::ActorBlockSyncMessage {
                actor_unique_id: REMOTE as i64,
                message: 1,
            },
        }]),
        WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
            dimension: 0,
            position: [0, -64, 0],
            nbt: block_entity_nbt("Chest", [0, -64, 0]),
        }),
        WorldEvent::BlockEvent(BlockEventEvent {
            dimension: 0,
            position: [0, -64, 0],
            event_type: 1,
            event_value: 1,
        }),
        WorldEvent::MapData(MapDataEvent {
            map_id: 1,
            start_x: 0,
            start_y: 0,
            width: 1,
            height: 1,
            pixels: Arc::from([0]),
        }),
        WorldEvent::OpenSign(OpenSignEvent {
            dimension: 0,
            position: [0, -64, 0],
            front: true,
        }),
        WorldEvent::ChunkRadiusUpdated(8),
        WorldEvent::PublisherUpdate(PublisherUpdateEvent {
            center: [0, 64, 0],
            radius_blocks: 64,
        }),
        WorldEvent::ChangeDimension(ChangeDimensionEvent::default()),
        WorldEvent::DimensionChangeAck { runtime_id: LOCAL },
        WorldEvent::Respawn(RespawnEvent {
            position: [0.5, 80.0, 0.5],
            state: 1,
            runtime_entity_id: LOCAL,
        }),
        move_player(MovePlayerMode::Normal),
        move_player(MovePlayerMode::Teleport),
        WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
            position: [0.5, 80.0, 0.5],
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: MovementCorrectionSubject::Player,
            on_ground: true,
            tick: 1,
        }),
        motion(LOCAL),
        motion(REMOTE),
        movement_effect(LOCAL),
        movement_effect(REMOTE),
        WorldEvent::NetworkStackLatency(1),
        WorldEvent::SetTime(SetTimeEvent { time: 1 }),
        WorldEvent::WorldClocks(vec![WorldClockUpdateEvent::Sync(WorldClockState {
            id: 0,
            time: 1,
            paused: false,
        })]),
        WorldEvent::GameRules(GameRulesEvent {
            daylight_cycle: Some(DaylightCycleUpdateEvent { enabled: true }),
            weather_cycle: Some(true),
            hud: Default::default(),
        }),
        WorldEvent::Weather(WeatherUpdateEvent {
            channel: WeatherChannel::Rain,
            level: 0.5,
        }),
        level_sound(None),
        level_sound(Some([1.0, 2.0, 3.0])),
        WorldEvent::Camera(CameraEvent::Shake(CameraShakeEvent {
            intensity: 1.0,
            duration_seconds: 1.0,
            shake_type: CameraShakeType::Positional,
            action: CameraShakeAction::Add,
        })),
        WorldEvent::Actor(ActorEvent::Identifiers(Default::default())),
        mob_spawn(11),
        WorldEvent::Actor(ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: REMOTE as i64,
        })),
        WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: REMOTE,
            position: [Some(5.0); 3],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        })),
        metadata(LOCAL),
        metadata(REMOTE),
        attributes(LOCAL),
        attributes(REMOTE),
        WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [0; 16] }]),
        })),
        WorldEvent::Actor(ActorEvent::Skin {
            uuid: [0; 16],
            skin: protocol::PlayerSkin::Unavailable(
                protocol::PlayerSkinUnavailable::InvalidDimensions,
            ),
        }),
        player_spawn(12),
        status(LOCAL, ActorStatusKind::Hurt),
        status(REMOTE, ActorStatusKind::Hurt),
        status(REMOTE, ActorStatusKind::Death),
        WorldEvent::Actor(ActorEvent::TakeItem(ActorTakeItemEvent {
            item_runtime_id: ITEM,
            collector_runtime_id: LOCAL,
        })),
        effect(LOCAL),
        effect(REMOTE),
        WorldEvent::ActorLink(ActorLinkEvent {
            dimension: 0,
            ridden_unique_id: REMOTE as i64,
            rider_unique_id: LOCAL as i64,
            link_type: ActorLinkType::Rider,
            immediate: true,
            rider_initiated: false,
        }),
        WorldEvent::ActorPropertySync(ActorPropertySyncEvent {
            data: Arc::from([]),
        }),
        WorldEvent::Ui(UiEvent::ShowCredits(ShowCreditsEvent { runtime_id: LOCAL })),
        WorldEvent::Ui(UiEvent::PlayerGameMode {
            actor_unique_id: LOCAL as i64,
            tick: 1,
            event: GameModeEvent {
                update: GameModeUpdate::Explicit(PlayerGameMode::Creative),
            },
        }),
        WorldEvent::Ui(UiEvent::DefaultGameMode(GameModeEvent {
            update: GameModeUpdate::Explicit(PlayerGameMode::Creative),
        })),
        WorldEvent::BlockCrack(BlockCrackEvent {
            position: [0, -64, 0],
            action: BlockCrackAction::Start {
                progress_per_tick: 7,
            },
        }),
        WorldEvent::Equipment(EquipmentEvent {
            actor_runtime_id: REMOTE,
            stack: Default::default(),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: None,
        }),
        WorldEvent::ArmorEquipment(Box::new(ArmorEquipmentEvent {
            actor_runtime_id: REMOTE,
            helmet: Default::default(),
            chestplate: Default::default(),
            leggings: Default::default(),
            boots: Default::default(),
            body: Default::default(),
        })),
        WorldEvent::Inventory(protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        )),
        WorldEvent::ItemActor(ItemActorEvent::Registry(ItemRegistryEvent {
            entries: protocol::vanilla_item_registry()
                .iter()
                .take(1)
                .cloned()
                .collect(),
        })),
        action(ActorActionKind::CriticalHit),
        action(ActorActionKind::SwingArm),
        WorldEvent::Particle(ParticleEvent::Spawn(protocol::SpawnParticleEffectEvent {
            dimension: 0,
            actor_unique_id: Some(REMOTE as i64),
            position: [0.0; 3],
            effect: "minecraft:heart_particle".into(),
            molang_variables: None,
        })),
        WorldEvent::Particle(ParticleEvent::Spawn(protocol::SpawnParticleEffectEvent {
            dimension: 0,
            actor_unique_id: None,
            position: [0.0; 3],
            effect: "minecraft:heart_particle".into(),
            molang_variables: None,
        })),
        WorldEvent::Ui(UiEvent::Boss(protocol::BossEvent {
            target_entity_id: REMOTE as i64,
            action: protocol::BossAction::Show,
            title: "boss".into(),
            filtered_title: "boss".into(),
            progress: 1.0,
            style: protocol::BossStyle {
                color: protocol::BossColor::Pink,
                overlay: protocol::BossOverlay::Progress,
                darken_sky: None,
                create_world_fog: None,
            },
        })),
        WorldEvent::Particle(ParticleEvent::ActorCritical {
            actor_runtime_id: REMOTE,
            magic: false,
            particle_count: 1.0,
        }),
        WorldEvent::PrimitiveShapes(PrimitiveShapesEvent::default()),
    ]
}

/// A stream with a loaded column, a remote mob and a dropped item, and drained consumers.
fn fixture() -> (WorldStream, u64) {
    fixture_with_actors(true)
}

/// Without actors the same sequences carry inert events, so outputs differ only by what an
/// event reads from the actor store.
fn fixture_with_actors(actors: bool) -> (WorldStream, u64) {
    let mut stream = block_entity_visual_stream();
    stream.submit(1, inline_air_event(0)).unwrap();
    if actors {
        stream.submit(2, mob_spawn(REMOTE)).unwrap();
        stream.submit(3, item_spawn(ITEM)).unwrap();
    } else {
        stream
            .submit(2, WorldEvent::NetworkStackLatency(0))
            .unwrap();
        stream
            .submit(3, WorldEvent::NetworkStackLatency(0))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    drain(&mut stream);
    (stream, 4)
}

/// Commits `event` on a fresh fixture; returns the consumers it touched and their contents.
fn commit_sample(event: WorldEvent, actors: bool) -> (Consumers, String) {
    let (mut stream, sequence) = fixture_with_actors(actors);
    let actors_before = stream.authority.actor_sequence_frontier();
    let label = format!("{event:?}");
    stream.submit(sequence, event).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.order.is_finished(sequence), "{label} did not commit");
    let (mut touched, contents) = drain(&mut stream);
    if stream.authority.actor_sequence_frontier() != actors_before {
        touched = touched.with(Consumers::ACTORS);
    }
    (touched, contents)
}

/// Events whose consumers look up an actor when they run, after the commit.
fn consumer_resolves_actor(event: &WorldEvent) -> bool {
    match event {
        WorldEvent::Particle(ParticleEvent::Spawn(spawn)) => spawn.actor_unique_id.is_some(),
        WorldEvent::Camera(_) | WorldEvent::Ui(UiEvent::Boss(_)) => true,
        WorldEvent::SyncedBlockUpdates(updates) => updates
            .iter()
            .any(|update| update.sync.actor_unique_id != -1 && update.sync.message != 0),
        _ => false,
    }
}

/// Consumers holding anything, in [`Consumers`] terms, and what they held.
fn drain(stream: &mut WorldStream) -> (Consumers, String) {
    let mut touched = Consumers::NONE;
    let mut mark = |nonempty: bool, consumer| {
        if nonempty {
            touched = touched.with(consumer);
        }
    };
    let retained = stream.authority.retained_commit_count();
    let controls = stream.take_committed_controls();
    let ui = stream.take_committed_ui();
    let audio = stream.take_committed_audio();
    let camera = stream.take_committed_camera();
    let particles = stream.take_committed_particles();
    let mut shapes = Vec::new();
    while let Some(shape) = stream.pop_primitive_shapes() {
        shapes.push(shape);
    }
    // Stream identities differ per fixture instance, not by actor state.
    let contents = format!("{controls:?}{ui:?}{audio:?}{camera:?}{particles:?}{shapes:?}")
        .split("stream_identity")
        .map(|part| part.trim_start_matches(|c: char| c == ':' || c == ' ' || c.is_ascii_digit()))
        .collect::<String>();
    let (controls, ui, audio, camera, shapes) = (
        controls.len(),
        ui.len(),
        audio.len(),
        camera.len(),
        shapes.len(),
    );
    mark(controls != 0, Consumers::CONTROLS);
    mark(ui != 0, Consumers::UI);
    mark(audio != 0, Consumers::AUDIO);
    mark(camera != 0, Consumers::CAMERA);
    mark(shapes != 0, Consumers::SHAPES);
    mark(!particles.is_empty(), Consumers::PARTICLES);
    // Synchronized sounds wait in the actor store and still count as retained audio.
    mark(
        retained > controls + ui + audio + camera + shapes,
        Consumers::AUDIO.with(Consumers::ACTORS),
    );
    (touched, contents)
}

#[test]
fn every_event_footprint_covers_the_consumers_it_writes_and_reads() {
    let samples = samples();
    let mut covered = [false; KINDS];
    let mut covered_tail = [false; TAIL_KINDS];
    let mut omissions = Vec::new();
    for event in samples {
        match tail_kind(&event) {
            Some(index) => covered_tail[index] = true,
            None => covered[kind(&event)] = true,
        }
        let footprint = classify(&event, fixture().0.lane_context());
        let label = format!("{event:?}");
        let (mut touched, contents) = commit_sample(event.clone(), true);
        // A commit whose output depends on actor state reads the actor store.
        if commit_sample(event.clone(), false).1 != contents || consumer_resolves_actor(&event) {
            touched = touched.with(Consumers::ACTORS);
        }
        if !footprint.barrier && !footprint.consumers.contains(touched) {
            omissions.push(format!(
                "{} touched {touched:?}, footprint {:?}",
                label.chars().take(80).collect::<String>(),
                footprint.consumers
            ));
        }
    }
    assert!(
        covered.iter().all(|covered| *covered),
        "a kind has no sample"
    );
    assert!(
        covered_tail.iter().all(|covered| *covered),
        "a kind has no sample"
    );
    assert!(omissions.is_empty(), "{omissions:#?}");
}

/// A pickup held behind local knockback, which waits for a decoding block batch, still
/// sounds: a later ordinary sound must not reach the audio consumer before it.
#[test]
fn pickup_sound_keeps_its_order_with_later_sounds() {
    let (mut stream, sequence) = fixture();
    // Large enough that the batch decodes on a worker and holds local authority behind it.
    let batch = (0..256)
        .map(|index| BlockUpdateEvent {
            dimension: 0,
            position: [index % 16, -64, index / 16],
            layer: 0,
            network_id: 1,
        })
        .collect();
    stream
        .submit(sequence, WorldEvent::BlockUpdates(batch))
        .unwrap();
    assert_eq!(stream.order.blocking_block_updates(), Some(sequence));
    stream
        .submit(
            sequence + 1,
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: LOCAL,
                motion: [0.1, 0.2, 0.3],
                tick: 1,
            }),
        )
        .unwrap();
    stream
        .submit(
            sequence + 2,
            WorldEvent::Actor(ActorEvent::TakeItem(ActorTakeItemEvent {
                item_runtime_id: ITEM,
                collector_runtime_id: LOCAL,
            })),
        )
        .unwrap();
    stream.submit(sequence + 3, level_sound(None)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    let sequences = stream
        .take_committed_audio()
        .iter()
        .map(|sound| sound.sequence)
        .collect::<Vec<_>>();
    assert_eq!(sequences, [sequence + 2, sequence + 3]);
}

/// An actor-bound particle waits for the spawn of its actor, which local knockback behind a
/// decoding block batch holds back, instead of reaching its consumer with no actor to find.
#[test]
fn actor_bound_particle_waits_for_its_held_actor_spawn() {
    let (mut stream, sequence) = fixture();
    let batch = (0..256)
        .map(|index| BlockUpdateEvent {
            dimension: 0,
            position: [index % 16, -64, index / 16],
            layer: 0,
            network_id: 1,
        })
        .collect();
    stream
        .submit(sequence, WorldEvent::BlockUpdates(batch))
        .unwrap();
    stream
        .submit(
            sequence + 1,
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: LOCAL,
                motion: [0.1, 0.2, 0.3],
                tick: 1,
            }),
        )
        .unwrap();
    stream.submit(sequence + 2, mob_spawn(11)).unwrap();
    stream
        .submit(
            sequence + 3,
            WorldEvent::Particle(ParticleEvent::Spawn(protocol::SpawnParticleEffectEvent {
                dimension: 0,
                actor_unique_id: Some(11),
                position: [0.0; 3],
                effect: "minecraft:heart_particle".into(),
                molang_variables: None,
            })),
        )
        .unwrap();
    assert!(stream.take_committed_particles().is_empty());
    assert!(stream.authority.actor_by_unique_id(11).is_none());
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_committed_particles().len(), 1);
    assert!(stream.authority.actor_by_unique_id(11).is_some());
}
