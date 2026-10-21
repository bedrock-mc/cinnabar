use super::style::style_scene;
use super::*;
mod counter;
use protocol_ids::ActorFlag;

mod badges;
mod budget;
mod frustum;
mod opacity;
mod own_tag;
mod view_depth;
mod visibility;

/// Uses the supplied camera position looking along -Z.
fn view(camera_position: Vec3) -> NametagView {
    NametagView {
        camera_position,
        camera_forward: Vec3::NEG_Z,
    }
}

/// Returns a spawned player with an initialized render pose.
fn actor() -> ActorSnapshot {
    let stream = actor_stream(1, None, 0);
    let mut actor = stream
        .authority()
        .actor(1)
        .expect("spawn committed")
        .clone();
    actor.movement_revision = 1;
    actor
}

/// Spawns nearby players in reverse distance order, optionally sharing synced score text.
fn actor_stream(
    count: usize,
    score: Option<Arc<str>>,
    local_runtime_id: u64,
) -> chunk_pipeline::WorldStream {
    let mut stream = chunk_pipeline::WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: local_runtime_id,
            local_player_unique_id: local_runtime_id as i64,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::air_network_id(false),
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        [0.0; 3],
        None,
    );
    for index in 1..=count {
        let runtime_id = index as u64;
        let mut uuid = [0; 16];
        uuid[..8].copy_from_slice(&runtime_id.to_le_bytes());
        let spawn = protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: index as i64,
            runtime_id,
            kind: ActorKind::Player {
                uuid,
                username: "p".into(),
            },
            position: [(count - index) as f32 / count as f32 * 5.0, 0.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: std::iter::once(protocol::ActorMetadata {
                key: METADATA_ALWAYS_SHOW_NAMETAG,
                value: ActorMetadataValue::Byte(1),
            })
            .chain(score.as_ref().map(|score| protocol::ActorMetadata {
                key: METADATA_SCORE,
                value: ActorMetadataValue::String(Arc::clone(score)),
            }))
            .collect::<Vec<_>>()
            .into(),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        };
        stream
            .submit(
                runtime_id,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
            )
            .unwrap();
    }
    stream.poll([0.0; 3], 0);
    stream
}

/// Creates an ordinary tag for font and scene fixtures.
pub(in crate::ui_runtime::presentation) fn anchor(name: &str) -> NametagAnchor {
    NametagAnchor {
        runtime_id: 1,
        position: Vec3::new(0.0, 2.5, -5.0),
        lines: name
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(Arc::from)
            .collect::<Vec<_>>()
            .into(),
        depth_tested: false,
        sneaking: false,
        text_alpha: 1.0,
        view_depth: 5.0,
        camera_forward_y: 0.0,
        cinnabar_badge: false,
        counter: None,
    }
}

/// Builds label passes with the fixture font and a fresh atlas.
fn scene(anchors: &[NametagAnchor]) -> NametagScene {
    let font = ui_engine::test_support::fixture_font();
    scene_with_font(anchors, &font)
}

/// Builds a scene with a supplied font and a fresh line atlas.
fn scene_with_font(anchors: &[NametagAnchor], font: &RuntimeFontCatalog) -> NametagScene {
    build_nametag_scene(
        anchors,
        None,
        font,
        &mut TextLayoutCache::new(8, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| super::super::nametag_atlas::font_page(font, page),
    )
}

#[test]
fn metadata_scale_multiplies_the_box_height_and_the_head_clearance() {
    let mut actor = actor();
    actor.metadata.insert(38, ActorMetadataValue::Float(2.0));
    assert_eq!(tag_height(&actor), 2.0 * (DEFAULT_HEIGHT + HEAD_CLEARANCE));
    actor
        .metadata
        .insert(METADATA_HEIGHT, ActorMetadataValue::Float(3.6));
    assert_eq!(tag_height(&actor), 2.0 * (DEFAULT_HEIGHT + HEAD_CLEARANCE));
    let player_kind = actor.kind.clone();
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:zombie".into(),
    };
    assert_eq!(tag_height(&actor), 2.0 * (3.6 + HEAD_CLEARANCE));
    actor.kind = player_kind;
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Sneaking.wire_value()),
    );
    actor.metadata.remove(&METADATA_HEIGHT);
    assert_eq!(
        tag_height(&actor),
        2.0 * (DEFAULT_HEIGHT + HEAD_CLEARANCE) - SNEAK_PLAYER_Y_OFFSET
    );
}

#[test]
fn player_collision_height_does_not_lower_java_labels_again_while_crouching() {
    let mut actor = actor();
    let feet = Vec3::new(0.0, 0.0, -5.0);
    let boards = ui::ScoreboardStore::default();
    for scale in [0.5, 1.0, 2.0] {
        actor.metadata.insert(38, ActorMetadataValue::Float(scale));
        for (sneaking, collision_height) in [(false, 1.8), (true, 1.5), (false, 1.8)] {
            actor.metadata.insert(
                0,
                ActorMetadataValue::Flags(
                    (1 << ActorFlag::ShowName.wire_value())
                        | if sneaking {
                            1 << ActorFlag::Sneaking.wire_value()
                        } else {
                            0
                        },
                ),
            );
            actor
                .metadata
                .insert(METADATA_HEIGHT, ActorMetadataValue::Float(collision_height));
            let tag = extract_nametag_at_feet(
                &actor,
                view(Vec3::ZERO),
                None,
                "Player".into(),
                Some(&boards),
                feet,
            )
            .unwrap();
            let rendered = scene(&[tag]);
            let expected_anchor = feet.y + (DEFAULT_HEIGHT + HEAD_CLEARANCE) * scale
                - if sneaking { SNEAK_PLAYER_Y_OFFSET } else { 0.0 };
            for record in &rendered.records {
                assert!(
                    (record.anchor[1] - expected_anchor).abs() < 1e-6,
                    "collision height {collision_height}, scale {scale}: {record:?}"
                );
                let corners = record.world_corners(glam::Quat::IDENTITY).unwrap();
                let expected_top =
                    expected_anchor - record.rect[1] * render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;
                assert!((corners[0][1] - expected_top).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn near_zero_scale_floating_text_tags_sit_at_the_hosts_feet() {
    let mut actor = actor();
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:falling_block".into(),
    };
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    for (height, scale, expected) in [
        (0.98, 0.000_000_1, 0.0),
        (0.98, 0.0, 0.0),
        // An empty published box is a size, not a missing one.
        (0.0, 1.0, HEAD_CLEARANCE),
    ] {
        actor
            .metadata
            .insert(METADATA_HEIGHT, ActorMetadataValue::Float(height));
        actor.metadata.insert(38, ActorMetadataValue::Float(scale));
        let anchor = extract_nametag(
            &actor,
            view(Vec3::new(0.0, 1.62, 5.0)),
            None,
            "Floating text".into(),
            &ui::ScoreboardStore::default(),
            0.5,
        )
        .expect("an always-shown tag at any scale");
        let feet = Vec3::from_array(actor.interpolated_position(0.5).unwrap());
        let lift = anchor.position - feet;
        assert!(lift.is_finite());
        assert!(
            (lift.y - expected).abs() < 1e-5,
            "height {height} at scale {scale} lifted the tag {lift}"
        );
    }
}

#[test]
fn java_head_clearance_and_sneaking_offsets_keep_the_label_world_scale() {
    let mut actor = actor();
    assert!((tag_height(&actor) - 2.3).abs() < 1e-6);
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(
            (1 << ActorFlag::Sneaking.wire_value()) | (1 << ActorFlag::ShowName.wire_value()),
        ),
    );
    assert!((tag_height(&actor) - 2.175).abs() < 1e-6);
    let tag = extract_nametag(
        &actor,
        view(Vec3::new(0.0, 0.0, 5.0)),
        None,
        "AB".into(),
        &ui::ScoreboardStore::default(),
        1.0,
    )
    .unwrap();
    let rendered = scene(&[tag]);
    assert!(
        rendered
            .records
            .iter()
            .all(|record| record.depth_tested != 0)
    );
    assert_eq!(rendered.records.len(), 2);
    assert_eq!(rendered.records[0].color, [0.0, 0.0, 0.0, 0.25]);
    assert_eq!(rendered.records[1].color, [1.0, 1.0, 1.0, 32.0 / 255.0]);
    let quad = rendered.records[1]
        .world_corners(glam::Quat::IDENTITY)
        .unwrap();
    let origin = Vec3::from_array(rendered.records[1].anchor);
    let first = Vec3::from_array(quad[0]);
    assert!((first.y - origin.y + 0.25).abs() < 1e-6);
}

#[test]
fn sleeping_players_lower_the_label_with_the_body() {
    let mut actor = actor();
    actor.metadata.insert(26, ActorMetadataValue::Byte(2));
    assert!(actor.is_sleeping());
    assert!((tag_height(&actor) - 0.8).abs() < 1e-6);
}

#[test]
fn server_distance_overrides_default_without_a_sneaking_cap() {
    let mut actor = actor();
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    for sneaking in [false, true] {
        actor.metadata.insert(
            0,
            ActorMetadataValue::Flags(u64::from(sneaking) << ActorFlag::Sneaking.wire_value()),
        );
        for (maximum, cutoff) in [(None, 64.0), (Some(128.0), 128.0), (Some(12.0), 12.0)] {
            actor.metadata.remove(&METADATA_RENDER_DISTANCE);
            if let Some(maximum) = maximum {
                actor
                    .metadata
                    .insert(METADATA_RENDER_DISTANCE, ActorMetadataValue::Float(maximum));
            }
            for (distance, expected) in [
                (cutoff - 0.01, true),
                (cutoff, true),
                (cutoff + 0.01, false),
            ] {
                assert_eq!(
                    nametag_geometry(&actor, Vec3::X * distance, None, 1.0).is_some(),
                    expected,
                    "sneaking={sneaking}, maximum={maximum:?}, distance={distance}"
                );
            }
        }
    }
}

#[test]
fn player_visibility_honors_explicit_hides_and_always_show_overrides() {
    let mut actor = actor();
    actor.metadata.remove(&METADATA_ALWAYS_SHOW_NAMETAG);
    assert!(!visible(&actor, None));
    for flags in [
        0,
        1 << ActorFlag::ShowName.wire_value(),
        1 << ActorFlag::AlwaysShowName.wire_value(),
    ] {
        actor.metadata.insert(0, ActorMetadataValue::Flags(flags));
        assert!(!visible(&actor, None));
        assert!(visible(&actor, Some(actor.runtime_id)));
        actor
            .metadata
            .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(0));
        assert!(!visible(&actor, None));
        actor
            .metadata
            .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
        assert!(visible(&actor, None));
        actor.metadata.remove(&METADATA_ALWAYS_SHOW_NAMETAG);
    }
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Invisible.wire_value()),
    );
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    assert!(!visible(&actor, Some(actor.runtime_id)));
}

#[test]
fn projected_tags_keep_mount_names_and_hide_only_their_score() {
    let mut stream = actor_stream(2, Some("7 wins".into()), 1);
    let mut cache = NametagTextCache::default();
    let boards = ui::ScoreboardStore::default();
    let project = |cache: &mut NametagTextCache, stream: &chunk_pipeline::WorldStream| {
        project_nametags(
            cache,
            &boards,
            stream,
            view(Vec3::ZERO),
            None,
            1.0,
            true,
            |_| true,
        )
    };
    let tags = project(&mut cache, &stream);
    assert_eq!(
        tags.iter()
            .map(NametagAnchor::runtime_id)
            .collect::<Vec<_>>(),
        [2]
    );
    stream
        .submit(
            3,
            protocol::WorldEvent::ActorLink(protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 2,
                rider_unique_id: 1,
                link_type: protocol::ActorLinkType::Rider,
                immediate: true,
                rider_initiated: false,
            }),
        )
        .unwrap();
    stream.poll([0.0; 3], 0);
    let tags = project(&mut cache, &stream);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].lines.as_ref(), [Arc::from("p")]);
}

#[test]
fn tag_anchor_follows_the_same_interpolated_feet_as_the_rig() {
    let mut actor = actor();
    actor.previous_pose.position = [2.0, 62.0, -2.0];
    actor.received_pose.position = [4.0, 64.0, -2.0];
    actor.position = actor.received_pose.position;
    for partial in [0.25, 0.5, 0.75] {
        let feet = Vec3::from_array(actor.interpolated_position(partial).unwrap());
        let anchor = tag_world_position(&actor, feet);
        assert_eq!(anchor, feet + Vec3::Y * (DEFAULT_HEIGHT + HEAD_CLEARANCE));
    }
}

#[test]
fn multiline_lines_get_their_own_centred_backgrounds_before_their_glyphs() {
    let anchor = anchor("AB\nA");
    let scene = scene(std::slice::from_ref(&anchor));
    assert_eq!(scene.records.len(), 4);
    let (plates, glyphs) = scene.records[..4].split_at(2);
    for (index, (plate, glyph)) in plates.iter().zip(glyphs).enumerate() {
        assert_eq!(plate.color, PLATE_COLOR);
        assert_eq!(plate.text, 0);
        assert_eq!(plate.rect[0], glyph.rect[0] - 1.0);
        assert_eq!(plate.rect[0], -plate.rect[2]);
        assert_eq!(plate.rect[1], NAMETAG_LINE_PITCH_PX * index as f32 - 1.0);
        assert_eq!(plate.rect[3], NAMETAG_LINE_PITCH_PX * index as f32 + 8.0);
        assert_eq!(glyph.rect[1], NAMETAG_LINE_PITCH_PX * index as f32);
    }
    assert!(
        plates[1].rect[0] > plates[0].rect[0],
        "the shorter line's background is narrower"
    );
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.anchor == anchor.position.to_array())
    );
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.line_lift == EXTRA_LINE_LIFT)
    );
    assert!(
        glyphs
            .iter()
            .all(|record| record.text == 1 && record.color == [1.0; 4])
    );
    assert!(glyphs.iter().all(|record| record.depth_tested == 0));
}

#[test]
fn oversized_tags_render_a_prefix_within_the_scene_budget() {
    let records_per_line = NAMETAG_RECORDS_PER_LINE;
    let line_budget = MAX_NAMETAG_RECORDS / records_per_line;
    let tag = anchor(&"A\n".repeat(line_budget + 1));
    let scene = scene(&[tag]);
    assert!(!scene.records.is_empty());
    assert!(scene.records.len() <= MAX_NAMETAG_RECORDS);
    assert_eq!(scene.records.len(), line_budget * records_per_line);
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.text == 1)
            .count(),
        line_budget
    );
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.depth_tested == 0)
            .count(),
        line_budget * NAMETAG_RECORDS_PER_LINE
    );
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.line_lift == EXTRA_LINE_LIFT * (line_budget - 1) as f32)
    );
}

#[test]
fn nearest_tags_take_the_budget_before_farther_tags() {
    let mut first = anchor("AB");
    first.position.x = 1.0;
    first.view_depth = 2.0;
    let first_records = NAMETAG_RECORDS_PER_LINE;
    let remaining_lines = (MAX_NAMETAG_RECORDS - first_records) / NAMETAG_RECORDS_PER_LINE;
    let mut oversized = anchor(&"A\n".repeat(remaining_lines + 2));
    oversized.position.x = 2.0;
    oversized.depth_tested = true;
    oversized.sneaking = true;
    let scene = scene(&[oversized, first]);
    assert_eq!(
        scene.records.len(),
        first_records + remaining_lines * NAMETAG_RECORDS_PER_LINE
    );
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.depth_tested == 0)
            .count(),
        NAMETAG_RECORDS_PER_LINE
    );
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.anchor[0] == 2.0 && record.text == 1)
            .count(),
        remaining_lines
    );
}

#[test]
fn sneaking_preserves_plate_opacity_and_selects_tested_glyphs() {
    let ordinary = scene(&[anchor("AB")]);
    let mut anchor = anchor("AB");
    anchor.depth_tested = true;
    anchor.sneaking = true;
    anchor.text_alpha = DIM_TEXT_ALPHA;
    let scene = scene(&[anchor]);
    assert!(scene.records.iter().all(|record| record.depth_tested != 0));
    assert_eq!(scene.records[0].color, PLATE_COLOR);
    assert_eq!(scene.records.len(), NAMETAG_RECORDS_PER_LINE);
    assert_eq!(scene.records[1].color, [1.0, 1.0, 1.0, DIM_TEXT_ALPHA]);
    let drop = SNEAK_LABEL_DROP / render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;
    for (sneak, normal) in scene.records.iter().zip(&ordinary.records) {
        assert!((sneak.rect[1] - normal.rect[1] - drop).abs() < 1e-6);
        assert!((sneak.rect[3] - normal.rect[3] - drop).abs() < 1e-6);
    }
}

#[test]
fn ordinary_and_sneaking_labels_draw_together_back_to_front() {
    let mut far = anchor("far");
    far.runtime_id = 2;
    far.view_depth = 8.0;
    far.position.x = 8.0;
    let mut sneak = anchor("sneak");
    sneak.depth_tested = true;
    sneak.sneaking = true;
    sneak.view_depth = 9.0;
    sneak.position.x = 9.0;
    let mut near = anchor("near");
    near.view_depth = 2.0;
    near.position.x = 2.0;
    let scene = scene(&[near, sneak, far]);
    assert_eq!(
        scene
            .records
            .iter()
            .map(|record| record.anchor[0])
            .collect::<Vec<_>>(),
        [9.0, 9.0, 8.0, 8.0, 2.0, 2.0]
    );
    assert_eq!(
        scene
            .records
            .iter()
            .map(|record| record.depth_tested)
            .collect::<Vec<_>>(),
        [1, 1, 0, 0, 0, 0]
    );
}

#[test]
fn visibility_uses_the_picked_actor_not_screen_distance() {
    let mut actor = actor();
    actor.metadata.remove(&METADATA_ALWAYS_SHOW_NAMETAG);
    actor.kind = ActorKind::Entity {
        identifier: "test:npc".into(),
    };
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::ShowName.wire_value()),
    );
    assert!(!visible(&actor, None));
    assert!(visible(&actor, Some(actor.runtime_id)));
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    assert!(visible(&actor, None));
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Invisible.wire_value()),
    );
    assert!(!visible(&actor, Some(actor.runtime_id)));
}

#[test]
fn render_distance_uses_the_synced_value_and_skips_bad_optional_data() {
    let mut actor = actor();
    assert_eq!(render_distance(&actor), DEFAULT_RENDER_DISTANCE);
    for value in [0.0, 12.0, 128.0] {
        actor
            .metadata
            .insert(METADATA_RENDER_DISTANCE, ActorMetadataValue::Float(value));
        assert_eq!(render_distance(&actor), value);
    }
    actor.metadata.insert(
        METADATA_RENDER_DISTANCE,
        ActorMetadataValue::Float(f32::NAN),
    );
    assert_eq!(render_distance(&actor), DEFAULT_RENDER_DISTANCE);
}

#[test]
fn synced_below_name_score_uses_the_native_ten_block_gate() {
    let mut actor = actor();
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("7 wins".into()));
    let boards = ui::ScoreboardStore::default();
    assert_eq!(
        &*tag_text(&actor, "AB".into(), 99.0, Some(&boards)),
        "AB\n7 wins"
    );
    assert_eq!(&*tag_text(&actor, "AB".into(), 100.0, Some(&boards)), "AB");
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("".into()));
    assert_eq!(&*tag_text(&actor, "AB".into(), 99.0, Some(&boards)), "AB");
}

#[test]
fn empty_names_hide_players_without_screen_coordinate_visibility_rules() {
    let actor = actor();
    let eye = Vec3::new(0.0, 1.62, 5.0);
    assert!(
        extract_nametag(
            &actor,
            view(eye),
            None,
            "p".into(),
            &ui::ScoreboardStore::default(),
            0.5
        )
        .is_some()
    );
    assert!(
        extract_nametag(
            &actor,
            view(eye),
            None,
            "".into(),
            &ui::ScoreboardStore::default(),
            0.5
        )
        .is_none()
    );
}

#[test]
fn a_viewer_at_the_anchor_keeps_the_screen_aligned_tag() {
    let actor = actor();
    let feet = Vec3::from_array(actor.interpolated_position(0.5).unwrap());
    let eye = tag_world_position(&actor, feet);
    assert!(
        extract_nametag(
            &actor,
            view(eye),
            None,
            "p".into(),
            &ui::ScoreboardStore::default(),
            0.5
        )
        .is_some()
    );
}

#[test]
fn cosmetic_shadow_and_border_preserve_each_records_depth_selection_and_record_bound() {
    let plate = NametagRecord {
        uv: [0., 0., -1., -1.],
        rect: [-2., -1., 2., 9.],
        ..Default::default()
    };
    let text = NametagRecord {
        uv: [0., 0., 1., 1.],
        text: 1,
        rect: [-1., 0., 1., 8.],
        color: [1., 1., 1., 0.125],
        ..Default::default()
    };
    let mut scene = NametagScene {
        records: vec![
            plate,
            text,
            NametagRecord {
                depth_tested: 1,
                ..plate
            },
            NametagRecord {
                depth_tested: 1,
                ..text
            },
        ],
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(
        serde_json::json!({"scale":2.,"shadow":true,"border_color":[1.,1.,1.,1.]}),
    )
    .unwrap();
    style_scene(&mut scene, &style, None);
    assert!(
        scene.records[..7]
            .iter()
            .all(|record| record.depth_tested == 0)
    );
    assert!(
        scene.records[7..]
            .iter()
            .all(|record| record.depth_tested != 0)
    );
    assert_eq!(scene.records.len(), 14);
    assert_eq!(
        scene.records[6].color[3], 0.125,
        "sneaking alpha survives cosmetic styling"
    );
    let mut hidden = style;
    hidden.hide = true;
    style_scene(&mut scene, &hidden, None);
    assert!(scene.records.is_empty());
}

#[test]
fn own_tag_color_and_border_width_preserve_other_tags() {
    let plate = NametagRecord {
        rect: [-2., -1., 2., 9.],
        uv: [0., 0., -1., -1.],
        ..Default::default()
    };
    let glyph = NametagRecord {
        anchor: [1., 2., 3.],
        runtime_id: 11,
        color: [1., 1., 1., 0.125],
        ..Default::default()
    };
    let remote = NametagRecord {
        anchor: [4., 5., 6.],
        runtime_id: 12,
        ..glyph
    };
    let mut scene = NametagScene {
        records: vec![plate, glyph, remote],
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(serde_json::json!({"border_width":3.,"border_color":[1.,1.,1.,1.],"text_color":[0.,1.,0.,1.],"own_text_color":[1.,0.,0.,1.]})).unwrap();
    style_scene(&mut scene, &style, Some(glyph.runtime_id));
    assert_eq!(scene.records[0].rect, [-5., -4., 5., -1.]);
    assert_eq!(scene.records[5].color, [1., 0., 0., 0.125]);
    assert_eq!(scene.records[6].color, [0., 1., 0., 0.125]);
}

#[test]
fn long_formatted_server_tags_keep_every_line() {
    let lines: Vec<String> = (0..40)
        .map(|row| format!("§{}§lrow {row:02} §rof a long server tag", row % 10))
        .collect();
    let text = lines.join("\n");
    assert!(text.len() > ui_screens::text::MAX_PRESENTED_TEXT_BYTES);
    let anchor = extract_nametag(
        &actor(),
        view(Vec3::new(0.0, 1.62, 5.0)),
        None,
        text.into(),
        &ui::ScoreboardStore::default(),
        0.5,
    )
    .unwrap();
    let kept: Vec<&str> = anchor.lines.iter().map(|line| &**line).collect();
    assert_eq!(kept, lines);
}

#[test]
fn huge_tags_allocate_only_the_scene_consumable_nonempty_lines() {
    let actor = actor();
    let scoreboards = ui::ScoreboardStore::default();
    let name: Arc<str> = "A\n"
        .repeat(protocol::MAX_ACTOR_METADATA_STRING_BYTES / "A\n".len())
        .into();
    let (tag, allocations) = test_alloc::count(|| {
        extract_nametag(
            &actor,
            view(Vec3::new(0.0, 1.62, 5.0)),
            None,
            Arc::clone(&name),
            &scoreboards,
            0.5,
        )
        .unwrap()
    });
    assert_eq!(tag.lines.len(), MAX_NAMETAG_LINES);
    assert!(tag.lines.iter().all(|line| line.as_ref() == "A"));
    assert!(allocations <= MAX_NAMETAG_LINES * 2);
    let text = format!(
        "{}{}",
        "\n".repeat(MAX_NAMETAG_LINES),
        "A\n".repeat(MAX_NAMETAG_LINES)
    );
    assert_eq!(nametag_lines(&text).len(), MAX_NAMETAG_LINES);
}

#[test]
fn projection_limits_extraction_to_the_nearest_tags_and_reuses_their_text() {
    let count = MAX_PRESENTED_NAMETAGS * 3;
    let score = "A\n"
        .repeat(protocol::MAX_ACTOR_METADATA_STRING_BYTES / "A\n".len())
        .into();
    let stream = actor_stream(count, Some(score), 0);
    let mut cache = NametagTextCache::default();
    let boards = ui::ScoreboardStore::default();
    let project = |cache: &mut NametagTextCache, show_players| {
        project_nametags(
            cache,
            &boards,
            &stream,
            view(Vec3::new(0.0, 1.62, 5.0)),
            None,
            0.5,
            show_players,
            |_| true,
        )
    };
    let (first, cold_allocations) = test_alloc::count(|| project(&mut cache, true));
    assert_eq!(first.len(), MAX_PRESENTED_NAMETAGS);
    assert_eq!(
        first.iter().map(|tag| tag.runtime_id).collect::<Vec<_>>(),
        (count - MAX_PRESENTED_NAMETAGS + 1..=count)
            .rev()
            .map(|id| id as u64)
            .collect::<Vec<_>>()
    );
    assert!(cold_allocations <= MAX_PRESENTED_NAMETAGS * MAX_NAMETAG_LINES * 2);
    let (second, warm_allocations) = test_alloc::count(|| project(&mut cache, true));
    assert!(warm_allocations < MAX_PRESENTED_NAMETAGS);
    assert!(
        first
            .iter()
            .zip(&second)
            .all(|(first, second)| Arc::ptr_eq(&first.lines, &second.lines))
    );
    assert!(project(&mut cache, false).is_empty());
    assert!(cache.entries.is_empty());
}

#[test]
fn cached_text_refreshes_pose_visibility_name_score_and_score_distance() {
    let mut actor = actor();
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("7 wins".into()));
    let boards = ui::ScoreboardStore::default();
    let name = Arc::from("Name");
    let eye = Vec3::new(0.0, 1.62, 5.0);
    let mut cache = NametagTextCache::default();
    let first = cache
        .extract(
            &actor,
            view(eye),
            None,
            Arc::clone(&name),
            Some(&boards),
            0.5,
        )
        .unwrap();
    actor.position[0] = 1.0;
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(
            (1 << ActorFlag::Sneaking.wire_value()) | (1 << ActorFlag::ShowName.wire_value()),
        ),
    );
    actor.metadata.insert(38, ActorMetadataValue::Float(2.0));
    let (moved, allocations) = test_alloc::count(|| {
        cache
            .extract(
                &actor,
                view(eye),
                None,
                Arc::clone(&name),
                Some(&boards),
                0.5,
            )
            .unwrap()
    });
    assert_eq!(allocations, 0);
    assert!(Arc::ptr_eq(&first.lines, &moved.lines));
    assert_ne!(first.position, moved.position);
    assert_eq!(
        moved.position.y,
        2.0 * (DEFAULT_HEIGHT + HEAD_CLEARANCE) - SNEAK_PLAYER_Y_OFFSET
    );
    assert!(moved.depth_tested);
    assert_eq!(moved.text_alpha, DIM_TEXT_ALPHA);
    actor.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Invisible.wire_value()),
    );
    assert!(
        cache
            .extract(
                &actor,
                view(eye),
                None,
                Arc::clone(&name),
                Some(&boards),
                0.5
            )
            .is_none()
    );
    actor.metadata.remove(&0);
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("8 wins".into()));
    let changed = cache
        .extract(
            &actor,
            view(eye),
            None,
            Arc::clone(&name),
            Some(&boards),
            0.5,
        )
        .unwrap();
    assert_eq!(changed.lines[1].as_ref(), "8 wins");
    let far = cache
        .extract(
            &actor,
            view(Vec3::new(0.0, 1.62, 20.0)),
            None,
            Arc::clone(&name),
            Some(&boards),
            0.5,
        )
        .unwrap();
    assert_eq!(far.lines.len(), 1);
    let renamed = cache
        .extract(&actor, view(eye), None, "Other".into(), Some(&boards), 0.5)
        .unwrap();
    assert_eq!(renamed.lines[0].as_ref(), "Other");
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("".into()));
    assert_eq!(
        cache
            .extract(&actor, view(eye), None, name, Some(&boards), 0.5)
            .unwrap()
            .lines
            .len(),
        1
    );
}

#[test]
fn cached_below_name_scores_follow_scoreboard_updates_and_clears() {
    let actor = actor();
    let mut boards = ui::ScoreboardStore::default();
    boards
        .apply(
            1,
            ui::ScoreboardEvent::DisplayObjective {
                display_slot: "belowname".into(),
                objective_name: "wins".into(),
                display_name: "wins".into(),
                criteria_name: "dummy".into(),
                sort_order: 0,
            },
        )
        .unwrap();
    let mut cache = NametagTextCache::default();
    let name = Arc::from("Name");
    let eye = Vec3::new(0.0, 1.62, 5.0);
    for (sequence, score) in [(2, 7), (3, 8)] {
        boards
            .apply(
                sequence,
                ui::ScoreboardEvent::Scores {
                    entries: Arc::from([ui::ScoreEntry {
                        action: ui::ScoreAction::Change,
                        scoreboard_id: 1,
                        objective_name: "wins".into(),
                        score,
                        owner: ui::ScoreOwner::Player(actor.unique_id),
                    }]),
                },
            )
            .unwrap();
        let tag = cache
            .extract(
                &actor,
                view(eye),
                None,
                Arc::clone(&name),
                Some(&boards),
                0.5,
            )
            .unwrap();
        assert_eq!(tag.lines[1].as_ref(), format!("{score} wins"));
    }
    boards.clear();
    assert_eq!(
        cache
            .extract(&actor, view(eye), None, name, Some(&boards), 0.5)
            .unwrap()
            .lines
            .len(),
        1
    );
}

#[test]
fn long_server_tag_lines_render_bounded_utf8_prefixes() {
    for text in ["A".repeat(20_000), "界".repeat(20_000 / "界".len() + 1)] {
        let tag = extract_nametag(
            &actor(),
            view(Vec3::new(0.0, 1.62, 5.0)),
            None,
            Arc::from(text.as_str()),
            &ui::ScoreboardStore::default(),
            0.5,
        )
        .unwrap();
        assert_eq!(tag.lines.len(), 1);
        assert_eq!(
            tag.lines[0].as_ref(),
            bounded_text(&text, ui::UiLimits::MAX_TEXT_BYTES)
        );
        assert!(tag.lines[0].chars().count() <= ui::MAX_GLYPHS_PER_LAYOUT);
        let scene = scene(&[tag]);
        assert_eq!(scene.records.len(), NAMETAG_RECORDS_PER_LINE);
        assert!(scene.atlas[0].rgba8.iter().any(|&byte| byte != 0));
        assert!(scene.atlas[0].cell[2] <= NAMETAG_ATLAS_SIDE);
        assert!(scene.atlas[0].cell[3] <= NAMETAG_ATLAS_SIDE);
    }
}

#[test]
fn long_formatted_lines_keep_the_aggregate_multiline_budget() {
    let line = "§aA§bB".repeat(ui::MAX_TEXT_SPANS / 2 + 1);
    let tag = extract_nametag(
        &actor(),
        view(Vec3::new(0.0, 1.62, 5.0)),
        None,
        format!("{line}\n{line}").into(),
        &ui::ScoreboardStore::default(),
        0.5,
    )
    .unwrap();
    assert_eq!(tag.lines.len(), 2);
    assert!(tag.lines.iter().map(|line| line.len()).sum::<usize>() > ui::UiLimits::MAX_TEXT_BYTES);
    for line in tag.lines.iter() {
        assert!(line.len() <= ui::UiLimits::MAX_TEXT_BYTES);
        assert!(
            ui::parse_bedrock_text(line, ui::UiLimits::MAX_TEXT_BYTES)
                .unwrap()
                .len()
                <= ui::MAX_TEXT_SPANS
        );
    }
    assert_eq!(scene(&[tag]).records.len(), 2 * NAMETAG_RECORDS_PER_LINE);
}

#[test]
fn wide_advance_lines_keep_a_prefix_instead_of_exceeding_the_wrap_limit() {
    let font = ui_engine::test_support::fixture_font();
    let mut metrics = *font.glyph('0').unwrap();
    metrics.advance_64 = i16::MAX;
    let font = font.with_glyphs(
        &[assets::SheetGlyph {
            metrics,
            draw_size_64: [
                u32::from(metrics.uv[2] - metrics.uv[0]) * 64,
                u32::from(metrics.uv[3] - metrics.uv[1]) * 64,
            ],
        }],
        |_| true,
    );
    let glyphs_per_line = NAMETAG_ATLAS_SIDE as usize / (i16::MAX as usize / 64 + 1);
    let text = "0".repeat(ui::MAX_WRAP_LINES * glyphs_per_line + 1);
    assert!(text.len() <= ui::UiLimits::MAX_TEXT_BYTES);
    let scene = scene_with_font(&[anchor(&text)], &font);
    assert_eq!(scene.records.len(), NAMETAG_RECORDS_PER_LINE);
    assert!(scene.atlas[0].cell[2] <= NAMETAG_ATLAS_SIDE);
}

#[test]
fn oversized_glyph_ink_is_clipped_to_the_atlas_cell() {
    let font = ui_engine::test_support::fixture_font();
    let font = font.with_glyphs(
        &[assets::SheetGlyph {
            metrics: *font.glyph('0').unwrap(),
            draw_size_64: [(NAMETAG_ATLAS_SIDE + 1) * 64; 2],
        }],
        |_| true,
    );
    let scene = scene_with_font(&[anchor("0")], &font);
    assert_eq!(scene.records.len(), NAMETAG_RECORDS_PER_LINE);
    assert_eq!(scene.atlas[0].cell[2..], [NAMETAG_ATLAS_SIDE; 2]);
    assert!(scene.atlas[0].rgba8.iter().any(|&byte| byte != 0));
}

/// An oversized opaque cosmetic may cover the label plane without covering ordinary name ink.
#[test]
fn overlapping_cosmetic_keeps_ordinary_names_readable_but_hides_sneaking_names() {
    for sneaking in [false, true] {
        let mut tag = anchor("Player\n7 wins");
        tag.depth_tested = sneaking;
        tag.sneaking = sneaking;
        tag.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
        tag.set_cinnabar_badge(true);
        let scene = scene(&[tag]);
        // Sample opaque ink on each run with an opaque hat closer than the label plane.
        let hat = [0.2, 0.4, 0.6];
        for glyph in scene.records.iter().filter(|record| record.text != 0) {
            let mut pixel = hat;
            if glyph.depth_tested == 0 {
                let fragment = glyph.fragment_color([1.0; 4]).unwrap();
                pixel = std::array::from_fn(|i| {
                    fragment[i] * fragment[3] + pixel[i] * (1.0 - fragment[3])
                });
            }
            assert_eq!(pixel, if sneaking { hat } else { [1.0; 3] });
        }
    }
}
