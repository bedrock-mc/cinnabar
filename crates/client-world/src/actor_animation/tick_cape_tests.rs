use super::*;

fn fixture() -> (ActorSnapshot, ActorAnimationStore) {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.on_ground = Some(true);
    let assets = super::super::render_frame::tests::counting_random_assets();
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    (actor, store)
}

fn advance(
    actor: &ActorSnapshot,
    store: &mut ActorAnimationStore,
    context: ActorTickContext,
) -> super::super::java::JavaMotion {
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        false,
        true,
        |_| context.clone(),
    );
    store.get(actor.runtime_id).unwrap().java
}

#[test]
fn cape_bob_reads_native_motion_and_decays_after_remote_interpolation_clears_it() {
    let (mut actor, mut store) = fixture();
    actor.velocity = [5.0, 0.0, 0.0];
    actor.status.native_velocity = [0.025, 0.0, 0.0];
    let motion = advance(&actor, &mut store, ActorTickContext::default());
    assert!((motion.bob[1] - 0.01).abs() < 1e-8);
    actor.status.native_velocity = [0.0; 3];
    let motion = advance(&actor, &mut store, ActorTickContext::default());
    assert!((motion.bob[1] - 0.006).abs() < 1e-8);
}

#[test]
fn cape_bob_decays_when_the_actor_is_dead_or_has_zero_health() {
    for dead_status in [false, true] {
        let (mut actor, mut store) = fixture();
        actor.status.native_velocity = [0.1, 0.0, 0.0];
        actor.velocity = actor.status.native_velocity;
        let motion = advance(&actor, &mut store, ActorTickContext::default());
        assert!((motion.bob[1] - 0.04).abs() < 1e-8);
        if dead_status {
            actor.status.dead = true;
        } else {
            actor.attributes.insert(
                "minecraft:health".into(),
                protocol::ActorAttribute {
                    name: "minecraft:health".into(),
                    min: 0.0,
                    max: 20.0,
                    current: 0.0,
                    default: None,
                    modifiers: Arc::from([]),
                },
            );
        }
        let motion = advance(&actor, &mut store, ActorTickContext::default());
        assert!((motion.bob[1] - 0.024).abs() < 1e-8);
    }
}

#[test]
fn cape_bob_clears_immediately_while_riding() {
    let (mut actor, mut store) = fixture();
    actor.velocity = [0.1, 0.0, 0.0];
    actor.status.native_velocity = actor.velocity;
    let motion = advance(&actor, &mut store, ActorTickContext::default());
    assert!((motion.bob[1] - 0.04).abs() < 1e-8);
    let previous_bob = motion.bob[1];
    let motion = advance(
        &actor,
        &mut store,
        ActorTickContext {
            is_riding: true,
            ..Default::default()
        },
    );
    assert_eq!(motion.bob, [previous_bob, 0.0]);
}

#[test]
fn local_flight_freezes_cape_walk_phase_but_keeps_chase_and_resumes_walking() {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.animation_clips[0].symbol = 2;
    let assets = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    store.exclude_remote_state_for(1);
    let mut feed = crate::LocalPlayerFeed {
        prefer_client_skin: false,
        uuid: [1; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.2, 0.0, 0.0],
        on_ground: true,
        flying: false,
        yaw: 90.0,
        head_yaw: 90.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: crate::ACTOR_SWING_TICKS,
        java_swing_ticks: crate::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    };
    let step = |store: &mut crate::actor_store::ActorStore, feed: &crate::LocalPlayerFeed| {
        store.sync_local_player(1, -1, feed);
        store.advance_interpolation_ticks(1);
        store.actor_rig(1).unwrap().java
    };
    step(&mut store, &feed);
    feed.position[0] = 0.2;
    let walking = step(&mut store, &feed);
    assert!(walking.walked[1] > 0.0);
    feed.flying = true;
    feed.on_ground = false;
    feed.position[0] = 0.4;
    let flying = step(&mut store, &feed);
    assert_eq!(flying.walked, [walking.walked[1]; 2]);
    assert_ne!(
        flying.cape[1], walking.cape[1],
        "flight still advances inertia"
    );
    assert!(flying.limb_swing[1] > walking.limb_swing[1]);
    feed.position[0] = 0.6;
    let flying = step(&mut store, &feed);
    assert_eq!(flying.walked, [walking.walked[1]; 2]);
    feed.flying = false;
    feed.on_ground = true;
    feed.position[0] = 0.8;
    let resumed = step(&mut store, &feed);
    assert_eq!(resumed.walked[0], walking.walked[1]);
    assert!((resumed.walked[1] - walking.walked[1] - 0.12).abs() < 1e-6);
}

#[test]
fn rig_snapshot_retains_java_equip_and_native_wing_inputs() {
    let (mut actor, mut store) = fixture();
    actor.on_ground = Some(false);
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << query::FLAG_GLIDING));
    let context = ActorTickContext {
        main_hand: Some(Arc::from("minecraft:potion")),
        main_hand_metadata: 21,
        has_cape: true,
        armor: [
            None,
            Some(WornArmor {
                item: "minecraft:elytra".into(),
                dye_rgb: None,
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    };
    for tick in 0..3 {
        actor.position = [tick as f32 * 0.2, tick as f32 * -0.4, 0.0];
        advance(&actor, &mut store, context.clone());
    }
    let rig = store.get(actor.runtime_id).unwrap();
    let input = rig
        .animation_variables
        .input()
        .expect("worn item owner inputs");
    assert_eq!(input.position_delta, [0.2, -0.4, 0.0]);
    assert_eq!(rig.java_equipped.unwrap().metadata, 21);
    assert!(
        rig.java.vanilla_posture,
        "gliding retains its authored body pose"
    );
    assert_ne!(rig.java.cape[0], rig.java.cape[1]);
}

/// Published progress must use the duration active on the current tick in both modes.
#[test]
fn published_swing_progress_tracks_fatigue_expiry_in_both_modes() {
    let (actor, mut store) = fixture();
    store.start_swing(actor.runtime_id, 8);
    for expected in [0.0, 1.0 / 8.0, 2.0 / 8.0] {
        advance(
            &actor,
            &mut store,
            ActorTickContext {
                bedrock_swing_ticks: 8,
                java_swing_ticks: 8,
                ..Default::default()
            },
        );
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.hand[1].attack_time, expected);
        assert_eq!(rig.java.swing[1], expected);
    }
    advance(
        &actor,
        &mut store,
        ActorTickContext {
            bedrock_swing_ticks: 6,
            java_swing_ticks: 6,
            ..Default::default()
        },
    );
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing[1], 0.5);
    assert_eq!(rig.hand[1].attack_time, 0.5);
}

/// Simulation-tick progress and its frame interpolation must finish at the reference duration.
#[test]
fn published_swing_progress_matches_no_effects_haste_ii_and_fatigue_i() {
    for duration in [6, 4, 8] {
        let (actor, mut store) = fixture();
        store.start_swing(actor.runtime_id, duration);
        for counter in 0..=duration + 1 {
            advance(
                &actor,
                &mut store,
                ActorTickContext {
                    bedrock_swing_ticks: duration,
                    java_swing_ticks: duration,
                    ..Default::default()
                },
            );
            let rig = store.get(actor.runtime_id).unwrap();
            let expected = if counter < duration {
                counter as f32 / duration as f32
            } else {
                0.0
            };
            assert_eq!(
                rig.java.swing[1], expected,
                "Java duration {duration}, tick {counter}"
            );
            assert_eq!(
                rig.hand[1].attack_time, expected,
                "Bedrock duration {duration}, tick {counter}"
            );
            for alpha in [0.0, 0.25, 0.5, 0.75] {
                let hand = rig.item_animation[0].interpolate(rig.item_animation[1], alpha);
                assert_eq!(hand.attack_time, rig.java.swing_progress(alpha));
            }
        }
    }
}

/// Expired Haste returns both published modes to the base duration mid-swing.
#[test]
fn published_swing_progress_tracks_haste_expiry_in_both_modes() {
    let (actor, mut store) = fixture();
    store.start_swing(actor.runtime_id, 4);
    for _ in 0..4 {
        advance(
            &actor,
            &mut store,
            ActorTickContext {
                bedrock_swing_ticks: 4,
                java_swing_ticks: 4,
                ..Default::default()
            },
        );
    }
    advance(
        &actor,
        &mut store,
        ActorTickContext {
            bedrock_swing_ticks: 6,
            java_swing_ticks: 6,
            ..Default::default()
        },
    );
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing[1], 4.0 / 6.0);
    assert_eq!(rig.hand[1].attack_time, 4.0 / 6.0);
}

/// A start on the final catch-up tick has zero current progress and preserves the prior tick.
#[test]
fn published_local_swing_catchup_keeps_each_start_on_its_tick() {
    let (actor, mut store) = fixture();
    store.sync_local_swing(
        actor.runtime_id,
        crate::LocalSwingProgress {
            bedrock: [0.5, 0.0],
            java: [0.5, 0.0],
            frame_alpha: None,
        },
    );
    for _ in 0..5 {
        advance(&actor, &mut store, ActorTickContext::default());
    }
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing, [0.5, 0.0]);
    assert_eq!(rig.hand.map(|phase| phase.attack_time), [0.5, 0.0]);
}

/// Local swing samples remain exact when the remote actor clock advances zero or several ticks.
#[test]
fn local_swing_samples_survive_actor_clock_phase_and_refresh() {
    let (actor, mut store) = fixture();
    advance(&actor, &mut store, ActorTickContext::default());
    let progress = crate::LocalSwingProgress {
        bedrock: [0.25, 0.5],
        java: [0.25, 0.5],
        frame_alpha: None,
    };
    assert!(store.sync_local_swing(actor.runtime_id, progress));
    store.refresh_local_view(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        actor.runtime_id,
        |_| ActorTickContext::default(),
    );
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing, progress.java);
    assert_eq!(rig.hand.map(|phase| phase.attack_time), progress.bedrock);
    assert!(!store.sync_local_swing(actor.runtime_id, progress));
    for _ in 0..5 {
        advance(&actor, &mut store, ActorTickContext::default());
    }
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing, progress.java);
    assert_eq!(rig.hand.map(|phase| phase.attack_time), progress.bedrock);
}

/// A correction's physical frame phase takes precedence over the remote actor phase.
#[test]
fn local_swing_sampling_uses_physics_phase_without_changing_equip() {
    let (actor, mut store) = fixture();
    advance(&actor, &mut store, ActorTickContext::default());
    let progress = crate::LocalSwingProgress {
        bedrock: [0.25, 0.5],
        java: [0.25, 0.5],
        frame_alpha: Some(0.75),
    };
    store.sync_local_swing(actor.runtime_id, progress);
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(rig.java.swing_progress(0.0), 0.4375);
    assert_eq!(rig.java.swing_progress(1.0), 0.4375);
    assert_eq!(progress.bedrock_progress(0.0), 0.4375);
    assert_eq!(rig.java.equip, [1.0; 2]);
    let idle = crate::LocalSwingProgress {
        frame_alpha: Some(0.1),
        ..Default::default()
    };
    assert!(store.sync_local_swing(actor.runtime_id, idle));
    assert!(!store.sync_local_swing(
        actor.runtime_id,
        crate::LocalSwingProgress {
            frame_alpha: Some(0.9),
            ..idle
        }
    ));
}
