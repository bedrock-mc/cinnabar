use super::*;
use protocol_ids::ActorFlag;

fn fixture() -> (TestActor, ActorAnimationStore) {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.on_ground = Some(true);
    let assets = crate::test_support::counting_random_assets();
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    (actor, store)
}

fn advance(
    actor: &TestActor,
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

/// Samples the published chase offsets at the same frame fractions used by presentation.
fn cape_frame(
    motion: super::super::java::JavaMotion,
    alpha: f32,
) -> render_model::java_animation::JavaCapeInput {
    let lerp = |[from, to]: [f32; 2]| from + (to - from) * alpha;
    let chase: [f32; 3] =
        std::array::from_fn(|axis| lerp([motion.cape[0][axis], motion.cape[1][axis]]));
    render_model::java_animation::JavaCapeInput {
        chase: chase.into(),
        body_yaw: motion.body_yaw_at(alpha),
        bob: lerp(motion.bob),
        walked: lerp(motion.walked),
        sneaking: false,
    }
}

#[test]
fn local_cape_flap_keeps_velocity_after_a_catch_up_tick_consumes_the_position() {
    let (mut actor, mut store) = fixture();
    let context = ActorTickContext {
        is_local: true,
        ..Default::default()
    };
    advance(&actor, &mut store, context.clone());
    actor.position[2] += 0.3;
    actor.velocity = [0.0, 0.0, 0.1];
    for _ in 0..3 {
        advance(&actor, &mut store, context.clone());
        let rig = store.get(actor.runtime_id).unwrap();
        let flap = rig
            .particle_queries(&actor, 1.0, 0)
            .query("cape_flap_amount");
        assert!((flap - 0.4).abs() < 1e-6);
    }
    actor.velocity = [0.0; 3];
    advance(&actor, &mut store, context);
    assert_eq!(
        store
            .get(actor.runtime_id)
            .unwrap()
            .particle_queries(&actor, 1.0, 0)
            .query("cape_flap_amount"),
        0.0
    );
}

#[test]
fn local_and_remote_flying_capes_stay_bounded_and_settle_after_stopping() {
    use render_model::java_animation::{CAPE_MAX_SWING_DEGREES, java_cape_angles, java_cape_bone};

    for local in [false, true] {
        for (velocity, expected_tilt) in [
            ([0.0, 2.0, 0.0], 0.0),
            ([0.0, -2.0, 0.0], 38.0),
            ([0.0, 0.0, 2.0], CAPE_MAX_SWING_DEGREES),
        ] {
            let (mut actor, mut store) = fixture();
            actor.on_ground = Some(false);
            let context = ActorTickContext {
                is_local: local,
                is_flying: local,
                ..Default::default()
            };
            advance(&actor, &mut store, context.clone());
            for tick in 1..=70 {
                actor.velocity = if tick <= 40 { velocity } else { [0.0; 3] };
                actor.status.native_velocity = actor.velocity;
                for axis in 0..3 {
                    actor.position[axis] += actor.velocity[axis];
                }
                let motion = advance(&actor, &mut store, context.clone());
                for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let input = cape_frame(motion, alpha);
                    let (tilt, side) = java_cape_angles(&input);
                    assert!((0.0..=CAPE_MAX_SWING_DEGREES).contains(&tilt));
                    assert!(side.abs() <= CAPE_MAX_SWING_DEGREES);
                    let (rotation, _) = java_cape_bone(&input, [0.0, 1.5, 0.1875].into());
                    let down = rotation.mul_vec3([0.0, -1.0, 0.0].into());
                    assert!(down.y <= 1e-6, "local {local}, tick {tick}, alpha {alpha}");
                }
                let tilt = java_cape_angles(&cape_frame(motion, 1.0)).0;
                if tick == 40 {
                    assert!((tilt - expected_tilt).abs() < 1e-4);
                } else if tick == 70 {
                    assert!((tilt - 6.0).abs() < 0.1, "cape must settle: {tilt}");
                }
            }
        }
    }
}

#[test]
fn java_pose_retains_only_explicitly_tagged_spear_postures() {
    for (identifier, is_spear) in [
        ("test:authored_weapon", true),
        ("test:kinetic_blade", false),
        ("minecraft:iron_spear", false),
    ] {
        let (actor, mut store) = fixture();
        let motion = advance(
            &actor,
            &mut store,
            ActorTickContext {
                main_hand: Some(identifier.into()),
                main_hand_is_spear: is_spear,
                main_hand_kinetic: Some(Default::default()),
                ..Default::default()
            },
        );
        assert_eq!(motion.vanilla_posture, is_spear, "{identifier}");
        let unequipped = advance(&actor, &mut store, ActorTickContext::default());
        assert!(!unequipped.vanilla_posture);
    }
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
fn rig_snapshot_retains_java_equip_and_native_wing_inputs() {
    let (mut actor, mut store) = fixture();
    actor.on_ground = Some(false);
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Gliding.wire_value()),
    );
    let context = ActorTickContext {
        main_hand: Some(Arc::from("minecraft:potion")),
        main_hand_metadata: 21,
        has_cape: true,
        armor: [
            None,
            Some(WornArmor {
                item: "minecraft:elytra".into(),
                fact: *assets::facts::ItemFacts::builtin()
                    .unwrap()
                    .by_identifier("minecraft:elytra")
                    .unwrap(),
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
    store.start_swing(actor.runtime_id, 8, 8);
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
        store.start_swing(actor.runtime_id, duration, duration);
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
    store.start_swing(actor.runtime_id, 4, 4);
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

/// A held stack whose data, block or tags change re-equips through the dip, but a count or
/// instance change does not.
#[test]
fn equip_dips_when_the_held_stack_variant_changes_but_not_its_count() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let sword: Option<Arc<str>> = Some(Arc::from("minecraft:iron_sword"));
    let fresh = StackVariant::default();
    let damaged = StackVariant {
        nbt_digest: [7; 32],
        ..fresh
    };
    let heights = |variants: &[StackVariant]| {
        let mut store =
            ActorAnimationStore::with_assets(crate::test_support::counting_random_assets());
        store.insert(1, 0, &actor);
        variants
            .iter()
            .map(|variant| {
                let context = ActorTickContext {
                    main_hand: sword.clone(),
                    held_variants: [*variant, StackVariant::default()],
                    ..Default::default()
                };
                store.advance_tick(&actors, None, None, true, true, |_| context.clone());
                store.get(1).unwrap().hand[1].arm_height
            })
            .collect::<Vec<_>>()
    };
    let near = |actual: Vec<f32>, expected: &[f32]| {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-6)
    };
    let dipped = heights(&[fresh, fresh, damaged, damaged, damaged, damaged]);
    assert!(
        near(dipped.clone(), &[1.0, 1.0, 0.6, 0.2, 0.0, 0.4]),
        "{dipped:?}"
    );
    let steady = heights(&[fresh, fresh, fresh, fresh]);
    assert!(near(steady.clone(), &[1.0; 4]), "{steady:?}");
}
