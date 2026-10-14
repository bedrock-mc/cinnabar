use super::*;

#[test]
fn bow_clock_keeps_native_large_duration_float_rounding() {
    for (ticks, base) in [(2, 0.0), (15, 13.0), (19, 17.0), (24, 22.0)] {
        for (alpha, fraction) in [(0.0, 0.0), (0.37, 0.3671875), (0.5, 0.5), (1.0, 1.0)] {
            assert_eq!(
                java_use(BOW, Some(BOW), ticks, None, alpha),
                Some(JavaUse::Bow {
                    pull: base + fraction
                }),
                "use tick {ticks}, frame {alpha}"
            );
        }
    }
}

#[test]
fn consume_clock_keeps_native_subtract_then_add_rounding() {
    for (ticks, bits) in [
        (1, 0x4202851e),
        (3, 0x41f50a3d),
        (20, 0x415a147b),
        (31, 0x402851ec),
    ] {
        assert_eq!(
            java_use(
                "minecraft:apple",
                Some("minecraft:apple"),
                ticks,
                Some(32),
                0.37
            ),
            Some(JavaUse::Consume {
                remaining: f32::from_bits(bits),
                duration: 32.0
            }),
            "use tick {ticks}"
        );
    }
}

#[test]
fn local_item_use_moves_between_physics_ticks_when_the_actor_fraction_wraps() {
    let mut stream = super::tests::head_stream();
    stream.sync_local_player_pose(&super::tests::head_feed());
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut rig = stream.authority().actor_rig(1).unwrap();
    // The captured actor counter can lag the local gameplay clock.
    rig.hand[1].use_ticks = 3;
    rig.java.equip = [1.0; 2];
    for (item, consume, elapsed) in [("minecraft:golden_apple", Some(32), 9), (BOW, None, 14)] {
        let mut previous = None;
        for (physics_alpha, actor_alpha) in [
            (0.0, 0.8),
            (0.25, 0.95),
            (0.5, 0.05),
            (0.75, 0.2),
            (1.0, 0.35),
        ] {
            let timing = actor_animation::AttachableAnimationInput {
                is_local_player: true,
                frame_alpha: physics_alpha,
                use_elapsed_ticks: Some(elapsed),
                ..Default::default()
            };
            let hand = first_person_hand(
                &rig,
                Some(item),
                Some(item),
                consume,
                actor_alpha,
                Some(&timing),
            );
            let expected = if item == BOW {
                JavaUse::Bow {
                    pull: 13.0 + physics_alpha,
                }
            } else {
                JavaUse::Consume {
                    remaining: 24.0 - physics_alpha,
                    duration: 32.0,
                }
            };
            assert_eq!(hand.using, Some(expected));
            let camera = java::first_person_item(hand, java::JavaItemMesh::Sprite, false);
            if let Some(previous) = previous {
                assert!(
                    !camera.abs_diff_eq(previous, 1e-6),
                    "{item} at {physics_alpha}"
                );
            }
            previous = Some(camera);
        }
        let next_tick = actor_animation::AttachableAnimationInput {
            is_local_player: true,
            frame_alpha: 0.0,
            use_elapsed_ticks: Some(elapsed + 1),
            ..Default::default()
        };
        let next_hand =
            first_person_hand(&rig, Some(item), Some(item), consume, 0.7, Some(&next_tick));
        assert_eq!(
            java::first_person_item(next_hand, java::JavaItemMesh::Sprite, false),
            previous.unwrap(),
            "the physics tick boundary must not jump the held item"
        );
    }
}

#[test]
fn local_use_stop_and_retained_items_ignore_a_stale_actor_use_counter() {
    let mut stream = super::tests::head_stream();
    stream.sync_local_player_pose(&super::tests::head_feed());
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut rig = stream.authority().actor_rig(1).unwrap();
    rig.hand[1].use_ticks = 20;
    let mut timing = actor_animation::AttachableAnimationInput {
        is_local_player: true,
        frame_alpha: 0.75,
        ..Default::default()
    };
    for item in ["minecraft:golden_apple", BOW] {
        assert_eq!(
            first_person_hand(&rig, Some(item), Some(item), Some(32), 0.2, Some(&timing)).using,
            None,
        );
    }
    timing.use_elapsed_ticks = Some(9);
    assert_eq!(
        first_person_hand(
            &rig,
            Some(BOW),
            Some("minecraft:golden_apple"),
            Some(32),
            0.2,
            Some(&timing),
        )
        .using,
        None,
    );
    timing.use_elapsed_ticks = None;
    assert_eq!(
        first_person_hand(
            &rig,
            Some("minecraft:iron_sword"),
            Some("minecraft:iron_sword"),
            None,
            0.2,
            Some(&timing),
        )
        .using,
        Some(JavaUse::Block),
    );
}

#[test]
fn local_use_does_not_move_equip_or_unpredicted_use_to_the_physics_clock() {
    let mut stream = super::tests::head_stream();
    stream.sync_local_player_pose(&super::tests::head_feed());
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut rig = stream.authority().actor_rig(1).unwrap();
    rig.hand[1].use_ticks = 3;
    rig.java.equip = [0.2, 0.6];
    let mut timing = actor_animation::AttachableAnimationInput {
        is_local_player: true,
        frame_alpha: 0.75,
        use_elapsed_ticks: Some(9),
        ..Default::default()
    };
    for local in [true, false] {
        timing.is_local_player = local;
        let hand = first_person_hand(
            &rig,
            Some("minecraft:golden_apple"),
            Some("minecraft:golden_apple"),
            Some(32),
            0.25,
            Some(&timing),
        );
        assert!((hand.equip - 0.3).abs() < 1e-6);
        assert_eq!(
            hand.using,
            Some(JavaUse::Consume {
                remaining: if local { 23.25 } else { 30.75 },
                duration: 32.0,
            }),
        );
    }
}

#[test]
fn hand_readiness_tracks_the_local_bow_fraction_and_reuses_unchanged_inputs() {
    let mut stream = super::tests::head_stream();
    stream.sync_local_player_pose(&super::tests::head_feed());
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_frame(1);
    let mut timing = actor_animation::AttachableAnimationInput {
        is_local_player: true,
        frame_alpha: 0.25,
        use_elapsed_ticks: Some(14),
        ..Default::default()
    };
    let key = super::super::hand::source_key(&stream, None, Some(timing), 0.5);
    assert!(key.is_some());
    assert!(key == super::super::hand::source_key(&stream, None, Some(timing), 0.5));
    timing.frame_alpha = 0.75;
    assert!(key != super::super::hand::source_key(&stream, None, Some(timing), 0.5));
}
