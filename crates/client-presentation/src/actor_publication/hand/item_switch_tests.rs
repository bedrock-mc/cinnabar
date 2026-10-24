use super::*;
use crate::presentation::equipment::{HeldKind, WornItem};
use native_tests::{fixture, hand_source_for_mode, player_feed};

/// Builds an ordinary stack without any live inventory or installed game assets.
fn item(identifier: &str) -> WornItem {
    WornItem {
        variant: Default::default(),
        map_id: None,
        charged_projectile: None,
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        grip: assets::item::grip::builtin_item_grip(identifier),
        use_facts: client_world::EffectiveItemUse::default(),
        kind: HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
        glint: Default::default(),
    }
}

/// Returns the actual main-hand mesh chosen by the production publication path.
fn main_mesh(source: HandSource) -> render_model::EntityRigId {
    source.items[0]
        .as_ref()
        .expect("the fixture item draws")
        .0
        .presentation
        .submission
        .input
        .rig
}

#[test]
fn both_modes_draw_the_outgoing_stack_until_the_equip_low_point() {
    for java_mode in [false, true] {
        for (old, new) in [
            ("minecraft:diamond_sword", "minecraft:shield"),
            ("minecraft:shield", "minecraft:diamond_sword"),
        ] {
            let (mut stream, mut equipment, artwork, mut input) = fixture(Some(old));
            let mut cache = java::HandCache::default();
            let outgoing = main_mesh(hand_source_for_mode(
                &stream,
                &mut equipment,
                &artwork,
                &input,
                &mut cache,
                1.0,
                java_mode,
            ));
            let mut feed = player_feed();
            feed.first_person = true;
            feed.main_hand = Some(new.into());
            stream.sync_local_player_pose(&feed);
            input.main = Some(item(new));

            // Input changes before the next animation tick; artwork still belongs to the old item.
            assert_eq!(
                main_mesh(hand_source_for_mode(
                    &stream,
                    &mut equipment,
                    &artwork,
                    &input,
                    &mut cache,
                    1.0,
                    java_mode,
                )),
                outgoing
            );
            for tick in 1..=6 {
                stream.advance_actor_interpolation_frame(1);
                let rig = stream.authority().actor_rig(1).unwrap();
                let height = if java_mode {
                    rig.java.equip[1]
                } else {
                    rig.hand[1].arm_height
                };
                for alpha in [0.0, 0.25, 0.75, 1.0] {
                    let drawn = main_mesh(hand_source_for_mode(
                        &stream,
                        &mut equipment,
                        &artwork,
                        &input,
                        &mut cache,
                        alpha,
                        java_mode,
                    ));
                    assert_eq!(drawn == outgoing, tick < 3, "mode {java_mode}, tick {tick}");
                }
                if tick == 3 {
                    assert_eq!(height, 0.0);
                }
                if tick == 6 {
                    assert_eq!(height, 1.0);
                }
            }
        }
    }
}

#[test]
fn equip_samples_and_retained_source_keys_follow_every_frame_fraction() {
    let (mut stream, _, _, _) = fixture(Some("minecraft:diamond_sword"));
    let mut feed = player_feed();
    feed.first_person = true;
    feed.main_hand = Some("minecraft:shield".into());
    stream.sync_local_player_pose(&feed);
    let mut previous_end = None;
    for _ in 0..6 {
        stream.advance_actor_interpolation_frame(1);
        let rig = stream.authority().actor_rig(1).unwrap();
        let at = |alpha| {
            [
                hand_progress(rig.hand, None, alpha).equip,
                java::first_person_hand(&rig, None, None, None, alpha, None).equip,
            ]
        };
        let start = at(0.0);
        let end = at(1.0);
        if let Some(previous) = previous_end {
            assert_eq!(start, previous);
        }
        for alpha in [0.25, 0.5, 0.75] {
            let sampled = at(alpha);
            for mode in 0..2 {
                assert!(
                    (sampled[mode] - (start[mode] + (end[mode] - start[mode]) * alpha)).abs()
                        < 1e-6
                );
            }
        }
        assert!(source_key(&stream, None, None, 0.25) != source_key(&stream, None, None, 0.75));
        previous_end = Some(end);
    }
}

#[test]
fn same_stack_updates_refresh_artwork_without_re_equipping() {
    let (mut stream, _, _, mut input) = fixture(Some("minecraft:diamond_sword"));
    let mut cache = java::HandCache::default();
    let rig = stream.authority().actor_rig(1).unwrap();
    cache.remember(&rig, input.main.as_ref());
    let mut feed = player_feed();
    feed.first_person = true;
    feed.main_hand = Some("minecraft:diamond_sword".into());
    feed.main_hand_stack_id = Some(17);
    stream.sync_local_player_pose(&feed);
    stream.advance_actor_interpolation_frame(1);
    feed.main_hand_metadata = 12;
    stream.sync_local_player_pose(&feed);
    let selected = input.main.as_mut().unwrap();
    selected.damage = Some(12);
    selected.enchanted = true;
    for _ in 0..3 {
        stream.advance_actor_interpolation_frame(1);
        let rig = stream.authority().actor_rig(1).unwrap();
        cache.remember(&rig, input.main.as_ref());
        assert_eq!(rig.hand.map(|hand| hand.arm_height), [1.0; 2]);
        assert_eq!(rig.java.equip, [1.0; 2]);
        let drawn = [
            cache.native_displayed_main(rig.main_hand_equipped, input.main.as_ref()),
            cache.displayed_main(rig.java_equipped, input.main.as_ref()),
        ];
        for item in drawn {
            let item = item.expect("the unchanged item remains drawn");
            assert_eq!(item.damage, Some(12));
            assert!(item.enchanted);
        }
    }
}

#[test]
fn a_missing_retained_stack_never_draws_the_unadopted_selection() {
    let (stream, _, _, input) = fixture(Some("minecraft:diamond_sword"));
    let mut rig = stream.authority().actor_rig(1).unwrap();
    let selected = item("minecraft:shield");
    let mut cache = java::HandCache::default();
    assert!(
        cache
            .native_displayed_main(rig.main_hand_equipped, Some(&selected))
            .is_none()
    );
    assert!(
        cache
            .displayed_main(rig.java_equipped, Some(&selected))
            .is_none()
    );

    cache.remember(&rig, input.main.as_ref());
    assert_eq!(
        cache
            .native_displayed_main(rig.main_hand_equipped, Some(&selected))
            .unwrap()
            .identifier,
        input.main.as_ref().unwrap().identifier,
    );
    rig.actor.spawn_revision += 1;
    cache.remember(&rig, Some(&selected));
    assert!(
        cache
            .native_displayed_main(rig.main_hand_equipped, Some(&selected))
            .is_none()
    );
    assert!(
        cache
            .displayed_main(rig.java_equipped, Some(&selected))
            .is_none()
    );
}
