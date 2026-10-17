use super::*;

/// Loaded air and a tall wall isolate collision permission from travel selection.
struct AbilityWall;

impl AbilityWall {
    /// Keeps every material and collision query in one immutable fixture revision.
    fn identity() -> sim::WorldCollisionIdentity {
        sim::WorldCollisionIdentity::new(
            sim::CollisionRegistryIdentity {
                preg_sha256: [71; 32],
                ..CollisionQuery::synthetic(()).identity.registry
            },
            [world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, 0, 0),
                revision: 1,
            }],
        )
        .unwrap()
    }
}

impl CollisionWorld for AbilityWall {
    fn registry_identity(&self) -> sim::CollisionRegistryIdentity {
        Self::identity().registry
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let wall = Aabb::new(
            Vec3::new(10.0, -1024.0, -64.0),
            Vec3::new(11.0, 1024.0, 64.0),
        );
        Ok(CollisionQuery {
            value: if wall.intersects(query) {
                vec![wall]
            } else {
                vec![]
            },
            identity: Self::identity(),
        })
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        Ok(sim::BlockPhysicsSample {
            layers: vec![sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: Default::default(),
                surface_response: sim::SurfaceResponse::None,
            }]
            .into(),
            identity: Self::identity(),
        })
    }
}

impl crate::GameplayWorld for AbilityWall {
    fn canonical_item_stack(
        &self,
        _: &protocol::NetworkItemStack,
    ) -> Option<client_world::CanonicalItemStack> {
        None
    }
    fn actor_by_unique_id(&self, _: i64) -> Option<&client_world::ActorSnapshot> {
        None
    }
    fn actor(&self, _: u64) -> Option<&client_world::ActorSnapshot> {
        None
    }
    fn local_player_runtime_id(&self) -> u64 {
        1
    }
    fn local_player_unique_id(&self) -> i64 {
        17
    }
    fn local_rider_seat_pose(&self) -> Option<([f32; 3], f32)> {
        None
    }
    fn network_id_mode(&self) -> assets::NetworkIdMode {
        assets::NetworkIdMode::Sequential
    }
    fn resolve_block_network_id(&self, id: u32) -> u32 {
        id
    }
    fn air_block_id(&self) -> u32 {
        0
    }
}

/// Builds one received layer without adding unrelated permission evidence.
fn ability_layer(kind: u16, defined: u32, values: u32) -> protocol::AbilityLayerEvidence {
    protocol::AbilityLayerEvidence {
        layer_type: kind,
        abilities: defined,
        values,
        fly_speed_bits: 0,
        vertical_fly_speed_bits: 0,
        walk_speed_bits: 0,
    }
}

/// Admits the supplied ability layers through the player fact boundary.
fn ability_player(layers: Vec<protocol::AbilityLayerEvidence>) -> player_state::PlayerState {
    let mut player = player_state::PlayerState::new(7);
    player.facts.publish_bootstrap_game_modes(
        protocol::PlayerGameMode::Survival,
        protocol::PlayerGameMode::Survival,
        false,
    );
    player.facts.bind_local_abilities(7, 4, 17, true);
    player.facts.apply_local_abilities(
        7,
        4,
        1,
        protocol::AbilitiesUpdate {
            actor_unique_id: 17,
            player_permission: 1,
            command_permission: 0,
            layers: protocol::AbilityLayersEvidence::Received(layers.into()),
        },
    );
    assert_eq!(player.facts.local_abilities_sequence(), Some(1));
    player
}

/// Drives admitted facts through the production frame, mode, simulation and send paths.
fn run_ability_axes(layers: Vec<protocol::AbilityLayerEvidence>, flying: bool, collision: bool) {
    run_ability_axes_updates(layers, |_, _| (flying, collision));
}

/// Applies policy changes before sampling each frame and verifies delayed correction replay.
fn run_ability_axes_updates(
    layers: Vec<protocol::AbilityLayerEvidence>,
    mut policy: impl FnMut(u64, &mut player_state::PlayerState) -> (bool, bool),
) {
    let mut player = ability_player(layers);
    let world = AbilityWall;
    let position = [8.0, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, position);
    ticker.set_source(MovementSource::Physics);
    let mut locals = super::super::super::LocomotionState::default();
    let mut effects = super::super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::super::LocalMovementSpeedAuthority::default();
    let mut hit_wall = false;
    let mut last_collision = true;
    for index in 1..=80 {
        let (flying, collision) = policy(index, &mut player);
        last_collision = collision;
        let mut frame = flight_frame(index, 1.0, index == 5 || index == 7, false, true);
        frame.facts = super::super::super::local_facts::read(Some(&player), &world, false);
        assert!(
            locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &world
            ),
            "frame {index}: {:?}",
            ticker.pending_authority_fault()
        );
        assert_eq!(physics.state().unwrap().tick, 100 + index);
        assert_eq!(
            physics.mode(),
            if flying {
                MovementMode::Flying
            } else {
                MovementMode::Walking
            },
            "frame {index}"
        );
        assert!(ticker.can_advance_physics_frame());
        hit_wall |= physics.state().unwrap().collisions.x;
        if collision {
            assert!(
                sim::Aabb::player_at(physics.state().unwrap().position)
                    .max
                    .x
                    <= 10.00001,
                "crossed wall at frame {index}"
            );
        }
        let mut admitted = Vec::new();
        super::super::super::flush_player_auth_inputs(
            &mut ticker,
            8,
            Some(super::super::super::integration_tests::evidence_context()),
            |identity, _| {
                admitted.push(identity);
                Ok::<_, &str>(())
            },
        )
        .unwrap();
        for identity in admitted {
            assert!(ticker.acknowledge_physics_send(identity));
        }
        let _ = ticker.take_tick_evidence();
        if index == 50 {
            let expected = physics.state().unwrap().clone();
            let old = physics.sample_at(expected.tick - 20).unwrap().clone();
            reconcile_candidate_physics_correction(
                &mut ticker,
                &mut physics,
                old.position,
                old.tick,
                old.grounded_after_tick,
                super::super::super::PhysicsCorrectionMode::ReplayIfRetained,
                &world,
            )
            .unwrap();
            let replayed = physics.state().unwrap();
            assert_eq!(replayed.tick, expected.tick);
            assert!((replayed.position - expected.position).length_squared() < 1.0e-8);
            assert_eq!(replayed.velocity, expected.velocity);
            assert_eq!(
                physics.mode(),
                if flying {
                    MovementMode::Flying
                } else {
                    MovementMode::Walking
                }
            );
        }
    }
    if last_collision {
        assert!(hit_wall, "never registered horizontal collision");
    } else {
        assert!(
            physics.state().unwrap().position.x > 11.0,
            "did not pass wall"
        );
    }
}

#[test]
fn ability_axes_flying_only_collides() {
    use client_world::game_mode_capabilities::ability_bit as b;
    run_ability_axes(
        vec![
            ability_layer(1, b::MAY_FLY | b::FLYING | b::NO_CLIP, b::MAY_FLY),
            ability_layer(2, b::FLYING, b::FLYING),
        ],
        true,
        true,
    );
}

#[test]
fn ability_axes_noclip_only_does_not_fly() {
    use client_world::game_mode_capabilities::ability_bit as b;
    run_ability_axes(
        vec![ability_layer(
            1,
            b::MAY_FLY | b::FLYING | b::NO_CLIP,
            b::NO_CLIP,
        )],
        false,
        false,
    );
}

#[test]
fn ability_axes_freecam_keeps_independent_axes() {
    use client_world::game_mode_capabilities::ability_bit as b;
    let all = b::MAY_FLY | b::FLYING | b::NO_CLIP;
    run_ability_axes(
        vec![
            ability_layer(1, all, all),
            ability_layer(2, b::FLYING, b::FLYING),
        ],
        true,
        false,
    );
}

#[test]
fn ability_axes_higher_flying_denial_wins() {
    use client_world::game_mode_capabilities::ability_bit as b;
    for kind in 3..=5 {
        for reverse in [false, true] {
            let mut layers = vec![
                ability_layer(1, b::MAY_FLY | b::FLYING | b::NO_CLIP, b::MAY_FLY),
                ability_layer(2, b::FLYING, b::FLYING),
                ability_layer(kind, b::FLYING, 0),
            ];
            if reverse {
                layers.reverse();
            }
            run_ability_axes(layers, false, true);
        }
    }
}

#[test]
fn ability_axes_higher_authority_without_spectator_record() {
    use client_world::game_mode_capabilities::ability_bit as b;
    for kind in 3..=5 {
        for flying in [false, true] {
            run_ability_axes(
                vec![
                    ability_layer(1, b::MAY_FLY | b::FLYING | b::NO_CLIP, b::MAY_FLY),
                    ability_layer(kind, b::FLYING, if flying { b::FLYING } else { 0 }),
                ],
                flying,
                true,
            );
        }
    }
}

#[test]
fn ability_axes_undefined_higher_layer_keeps_lower_flight_authority() {
    use client_world::game_mode_capabilities::ability_bit as b;
    run_ability_axes(
        vec![
            ability_layer(1, b::MAY_FLY | b::FLYING | b::NO_CLIP, b::MAY_FLY),
            ability_layer(2, b::FLYING, b::FLYING),
            ability_layer(5, b::MAY_FLY, b::MAY_FLY),
        ],
        true,
        true,
    );
}

#[test]
fn ability_axes_replay_retains_policy_changes() {
    use client_world::game_mode_capabilities::ability_bit as b;
    let all = b::MAY_FLY | b::FLYING | b::NO_CLIP;
    run_ability_axes_updates(
        vec![
            ability_layer(1, all, b::MAY_FLY),
            ability_layer(3, b::FLYING, 0),
        ],
        |index, player| {
            if index == 41 {
                player.facts.apply_local_abilities(
                    7,
                    4,
                    2,
                    protocol::AbilitiesUpdate {
                        actor_unique_id: 17,
                        player_permission: 1,
                        command_permission: 0,
                        layers: protocol::AbilityLayersEvidence::Received(
                            vec![
                                ability_layer(1, all, all),
                                ability_layer(4, b::FLYING, b::FLYING),
                            ]
                            .into(),
                        ),
                    },
                );
                assert_eq!(player.facts.local_abilities_sequence(), Some(2));
            }
            (index >= 41, index < 41)
        },
    );
}

#[test]
fn ability_axes_noclip_does_not_exempt_hunger_or_enable_air_sprint() {
    use client_world::game_mode_capabilities::ability_bit as b;
    let player = ability_player(vec![ability_layer(
        1,
        b::MAY_FLY | b::FLYING | b::NO_CLIP,
        b::NO_CLIP,
    )]);
    let facts = super::super::super::local_facts::read(Some(&player), &AbilityWall, false);
    assert!(facts.no_clip);
    assert!(!facts.spectator && !facts.can_fly);
    assert!(facts.sprint_blocked && facts.swim_hunger_blocked);
}
