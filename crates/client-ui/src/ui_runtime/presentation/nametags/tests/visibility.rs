use super::*;
use protocol_ids::ActorFlag;

#[test]
fn always_show_nametags_do_not_require_show_name_flag() {
    let mut actor = actor();
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
                    | (u64::from(always_flag) << ACTOR_FLAG_ALWAYS_SHOW_NAME)
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
                            && (always_flag
                                || always_byte.is_some_and(|byte| byte != 0 && byte != -1)
                                || (show && (player || picked == Some(actor.runtime_id))));
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
