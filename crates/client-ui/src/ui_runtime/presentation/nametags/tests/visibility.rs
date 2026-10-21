use super::*;
use protocol_ids::ActorFlag;

#[test]
fn always_show_nametags_do_not_require_show_name_flag() {
    let mut actor = actor();
    actor.metadata.remove(&METADATA_ALWAYS_SHOW_NAMETAG);
    let boards = ui::ScoreboardStore::default();
    for player in [true, false] {
        if !player {
            actor.kind = ActorKind::Entity {
                identifier: "test:display".into(),
            };
        }
        for extended in [false, true] {
            for combination in 0_u64..8 {
                let show = combination & 1 != 0;
                let always_flag = combination & 2 != 0;
                let invisible = combination & 4 != 0;
                let flags = (u64::from(show) << ActorFlag::ShowName.wire_value())
                    | (u64::from(always_flag) << ActorFlag::AlwaysShowName.wire_value())
                    | (u64::from(invisible) << ActorFlag::Invisible.wire_value());
                actor.metadata.insert(
                    0,
                    if extended {
                        ActorMetadataValue::FlagsExtended(flags)
                    } else {
                        ActorMetadataValue::Flags(flags)
                    },
                );
                for always_byte in [None, Some(-1), Some(0), Some(1), Some(2)] {
                    actor.metadata.remove(&METADATA_ALWAYS_SHOW_NAMETAG);
                    if let Some(byte) = always_byte {
                        actor
                            .metadata
                            .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(byte));
                    }
                    for picked in [None, Some(actor.runtime_id), Some(actor.runtime_id + 1)] {
                        let tag = extract_nametag(
                            &actor,
                            view(Vec3::new(0.0, 0.0, 5.0)),
                            picked,
                            "§aPlayer\n§7Rank".into(),
                            &boards,
                            1.0,
                        );
                        let expected = !invisible
                            && (always_byte.is_some_and(|byte| byte != 0 && byte != -1)
                                || picked == Some(actor.runtime_id));
                        assert_eq!(
                            tag.is_some(),
                            expected,
                            "player={player}, extended={extended}, flags={flags}, always={always_byte:?}, picked={picked:?}"
                        );
                        if let Some(tag) = tag {
                            assert_eq!(
                                tag.lines.as_ref(),
                                [Arc::from("§aPlayer"), Arc::from("§7Rank")]
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn projected_player_tags_follow_always_show_metadata_updates() {
    let mut stream = actor_stream(1, None, 0);
    let mut cache = NametagTextCache::default();
    let boards = ui::ScoreboardStore::default();
    for (index, (always, invisible, name, score, expected_lines)) in [
        (1, false, "§aPlayer\n§7Rank", "7 wins", 3),
        (-1, false, "§aPlayer\n§7Rank", "7 wins", 3),
        (0, false, "§aPlayer\n§7Rank", "7 wins", 0),
        (-1, false, "§aPlayer\n§7Rank", "7 wins", 0),
        (1, false, "§aPlayer\n§7Rank", "", 2),
        (1, true, "§aPlayer\n§7Rank", "", 0),
        (1, false, "", "7 wins", 0),
        (1, false, "§bPlayer", "8 wins", 2),
    ]
    .into_iter()
    .enumerate()
    {
        stream
            .submit(
                index as u64 + 2,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                    protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 1,
                        metadata: Arc::from([
                            protocol::ActorMetadata {
                                key: 0,
                                value: ActorMetadataValue::Flags(
                                    u64::from(invisible) << ActorFlag::Invisible.wire_value(),
                                ),
                            },
                            protocol::ActorMetadata {
                                key: 4,
                                value: ActorMetadataValue::String(name.into()),
                            },
                            protocol::ActorMetadata {
                                key: METADATA_ALWAYS_SHOW_NAMETAG,
                                value: ActorMetadataValue::Byte(always),
                            },
                            protocol::ActorMetadata {
                                key: METADATA_SCORE,
                                value: ActorMetadataValue::String(score.into()),
                            },
                        ]),
                        properties: Arc::from([]),
                        tick: 0,
                    },
                )),
            )
            .unwrap();
        stream.poll([0.0; 3], 0);
        let tags = project_nametags(
            &mut cache,
            &boards,
            &stream,
            view(Vec3::new(0.0, 0.0, 5.0)),
            None,
            1.0,
            true,
            |_| true,
        );
        assert_eq!(tags.len(), usize::from(expected_lines != 0));
        if let Some(tag) = tags.first() {
            let mut expected: Vec<Arc<str>> = name.split('\n').map(Arc::from).collect();
            if !score.is_empty() {
                expected.push(score.into());
            }
            assert_eq!(tag.lines.as_ref(), expected);
            assert_eq!(tag.lines.len(), expected_lines);
            assert_eq!(
                scene(&tags).records.len(),
                expected_lines * NAMETAG_RECORDS_PER_LINE
            );
        }
    }
}

#[test]
fn camera_position_controls_visibility_and_score_range() {
    let actor = actor();
    let boards = ui::ScoreboardStore::default();
    for (camera_distance, visible, scored) in [
        (5.0, true, true),
        (10.0, true, false),
        (40.0, true, false),
        (65.0, false, false),
    ] {
        let mut actor = actor.clone();
        actor
            .metadata
            .insert(METADATA_SCORE, ActorMetadataValue::String("7 wins".into()));
        let tag = extract_nametag_at_feet(
            &actor,
            NametagView {
                camera_position: Vec3::X * camera_distance,
                camera_forward: Vec3::NEG_Z,
            },
            None,
            "Player".into(),
            Some(&boards),
            Vec3::ZERO,
        );
        assert_eq!(tag.is_some(), visible, "camera distance {camera_distance}");
        if let Some(tag) = tag {
            assert_eq!(tag.lines.len(), if scored { 2 } else { 1 });
        }
    }
}

#[test]
fn nameplate_depth_flag_keeps_full_alpha_unless_sneaking() {
    let mut actor = actor();
    actor.metadata.insert(
        protocol_ids::ActorDataKey::FlagsThird.wire_value(),
        ActorMetadataValue::FlagsExtended(
            1 << (ActorFlag::NameplateDepthTested.wire_value() - 128),
        ),
    );
    let boards = ui::ScoreboardStore::default();
    for sneaking in [false, true] {
        actor.metadata.insert(
            0,
            ActorMetadataValue::Flags(u64::from(sneaking) << ActorFlag::Sneaking.wire_value()),
        );
        let tag = extract_nametag(
            &actor,
            view(Vec3::ZERO),
            None,
            "Player".into(),
            &boards,
            1.0,
        )
        .unwrap();
        assert!(tag.depth_tested);
        assert_eq!(tag.sneaking, sneaking);
        assert_eq!(tag.text_alpha, if sneaking { DIM_TEXT_ALPHA } else { 1.0 });
    }
}

#[test]
fn prone_players_use_live_height_even_when_sneaking() {
    let mut actor = actor();
    for flag in [ActorFlag::Swimming, ActorFlag::Crawling, ActorFlag::Gliding] {
        let bit = flag.wire_value();
        for sneaking in [false, true] {
            let primary = (u64::from(sneaking) << ActorFlag::Sneaking.wire_value())
                | if bit < 64 { 1 << bit } else { 0 };
            actor.metadata.insert(0, ActorMetadataValue::Flags(primary));
            actor.metadata.insert(
                protocol_ids::ActorDataKey::FlagsTwo.wire_value(),
                ActorMetadataValue::FlagsExtended(if bit >= 64 { 1 << (bit - 64) } else { 0 }),
            );
            actor
                .metadata
                .insert(METADATA_HEIGHT, ActorMetadataValue::Float(0.6));
            for scale in [0.5, 1.0, 2.0] {
                actor.metadata.insert(38, ActorMetadataValue::Float(scale));
                assert!(
                    (tag_height(&actor) - (0.6 + HEAD_CLEARANCE) * scale).abs() < 1e-6,
                    "{flag:?}, sneaking={sneaking}, scale={scale}"
                );
            }
        }
    }
}

#[test]
fn depth_tested_nameplates_keep_the_upright_quad_position() {
    let mut actor = actor();
    let boards = ui::ScoreboardStore::default();
    let ordinary = extract_nametag(
        &actor,
        view(Vec3::ZERO),
        None,
        "Player".into(),
        &boards,
        1.0,
    )
    .unwrap();
    actor.metadata.insert(
        ActorDataKey::FlagsThird.wire_value(),
        ActorMetadataValue::FlagsExtended(
            1 << (ActorFlag::NameplateDepthTested.wire_value() - 128),
        ),
    );
    let tested = extract_nametag(
        &actor,
        view(Vec3::ZERO),
        None,
        "Player".into(),
        &boards,
        1.0,
    )
    .unwrap();
    for (ordinary, tested) in scene(&[ordinary])
        .records
        .iter()
        .zip(&scene(&[tested]).records)
    {
        assert_eq!(ordinary.rect, tested.rect);
        assert_eq!(ordinary.anchor, tested.anchor);
        assert_eq!(ordinary.color, tested.color);
        assert_eq!(tested.depth_tested, 1);
    }
}
