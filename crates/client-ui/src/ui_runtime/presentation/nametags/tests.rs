use super::*;

fn actor() -> ActorSnapshot {
    let stream = actor_stream(1, None);
    let mut actor = stream
        .authority()
        .actor(1)
        .expect("spawn committed")
        .clone();
    actor.movement_revision = 1;
    actor
}

/// Spawns nearby players in reverse distance order, optionally sharing synced score text.
fn actor_stream(count: usize, score: Option<Arc<str>>) -> chunk_pipeline::WorldStream {
    let mut stream = chunk_pipeline::WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 0,
            local_player_unique_id: 0,
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
            metadata: score
                .as_ref()
                .map(|score| protocol::ActorMetadata {
                    key: METADATA_SCORE,
                    value: ActorMetadataValue::String(Arc::clone(score)),
                })
                .into_iter()
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
        text_alpha: 1.0,
        distance: 5.0,
    }
}

fn scene(anchors: &[NametagAnchor]) -> NametagScene {
    let font = ui_engine::test_support::fixture_font();
    scene_with_font(anchors, &font)
}

/// Builds a scene with a supplied font and a fresh line atlas.
fn scene_with_font(anchors: &[NametagAnchor], font: &RuntimeFontCatalog) -> NametagScene {
    build_nametag_scene(
        anchors,
        font,
        &mut TextLayoutCache::new(8, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| super::super::nametag_atlas::font_page(font, page),
    )
}

#[test]
fn metadata_scale_raises_the_tag_without_rescaling_a_published_box() {
    let mut actor = actor();
    actor.metadata.insert(38, ActorMetadataValue::Float(2.0));
    assert_eq!(tag_height(&actor), 2.0 * DEFAULT_HEIGHT + HEAD_CLEARANCE);
    actor
        .metadata
        .insert(METADATA_HEIGHT, ActorMetadataValue::Float(3.6));
    assert_eq!(tag_height(&actor), 3.6 + HEAD_CLEARANCE);
}

#[test]
fn tag_anchor_follows_the_same_interpolated_feet_as_the_rig() {
    let mut actor = actor();
    actor.previous_pose.position = [2.0, 62.0, -2.0];
    actor.received_pose.position = [4.0, 64.0, -2.0];
    actor.position = actor.received_pose.position;
    for partial in [0.25, 0.5, 0.75] {
        let anchor = tag_world_position(&actor, partial).unwrap();
        let feet = Vec3::from_array(actor.interpolated_position(partial).unwrap());
        assert_eq!(anchor - Vec3::Y * (DEFAULT_HEIGHT + HEAD_CLEARANCE), feet);
    }
}

#[test]
fn multiline_lines_get_their_own_centred_backgrounds_before_their_glyphs() {
    let anchor = anchor("AB\nA");
    let scene = scene(std::slice::from_ref(&anchor));
    assert_eq!(scene.records.len(), 4);
    assert_eq!(scene.see_through, 4);
    let (plates, glyphs) = scene.records.split_at(2);
    for (index, (plate, glyph)) in plates.iter().zip(glyphs).enumerate() {
        assert_eq!(plate.color, PLATE_COLOR);
        assert_eq!(plate.text, 0);
        assert_eq!(plate.rect[0], glyph.rect[0] - 1.0);
        assert_eq!(plate.rect[0], -plate.rect[2]);
        assert_eq!(plate.rect[1], NAMETAG_LINE_PITCH_PX * index as f32 - 1.0);
        assert_eq!(plate.rect[3], plate.rect[1] + NAMETAG_LINE_PITCH_PX);
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
}

#[test]
fn oversized_tags_render_a_prefix_within_the_scene_budget() {
    let line_budget = MAX_NAMETAG_LINES;
    let tag = anchor(&"A\n".repeat(line_budget + 1));
    let scene = scene(&[tag]);
    assert!(!scene.records.is_empty());
    assert!(scene.records.len() <= MAX_NAMETAG_RECORDS);
    assert_eq!(scene.records.len(), line_budget * NAMETAG_RECORDS_PER_LINE);
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.text == 1)
            .count(),
        line_budget
    );
    assert_eq!(scene.see_through, scene.records.len());
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.line_lift == EXTRA_LINE_LIFT * (line_budget - 1) as f32)
    );
}

#[test]
fn oversized_tags_use_the_budget_left_by_earlier_tags() {
    let mut first = anchor("AB");
    first.position.x = 1.0;
    let remaining_lines = MAX_NAMETAG_LINES - 1;
    let mut oversized = anchor(&"A\n".repeat(remaining_lines + 2));
    oversized.position.x = 2.0;
    oversized.depth_tested = true;
    let scene = scene(&[oversized, first]);
    assert_eq!(scene.records.len(), MAX_NAMETAG_RECORDS);
    assert_eq!(scene.see_through, NAMETAG_RECORDS_PER_LINE);
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
    let mut anchor = anchor("AB");
    anchor.depth_tested = true;
    anchor.text_alpha = SNEAK_TEXT_ALPHA;
    let scene = scene(&[anchor]);
    assert_eq!(scene.see_through, 0);
    assert_eq!(scene.records[0].color, PLATE_COLOR);
    assert_eq!(scene.records[1].color, [1.0, 1.0, 1.0, SNEAK_TEXT_ALPHA]);
}

#[test]
fn ordinary_then_sneaking_tags_keep_farthest_first_record_order() {
    let mut far = anchor("far");
    far.runtime_id = 2;
    far.distance = 8.0;
    far.position.x = 8.0;
    let mut sneak = anchor("sneak");
    sneak.depth_tested = true;
    sneak.distance = 9.0;
    sneak.position.x = 9.0;
    let mut near = anchor("near");
    near.distance = 2.0;
    near.position.x = 2.0;
    let scene = scene(&[near, sneak, far]);
    assert_eq!(scene.see_through, 4);
    assert_eq!(
        scene
            .records
            .iter()
            .map(|record| record.anchor[0])
            .collect::<Vec<_>>(),
        [8.0, 8.0, 2.0, 2.0, 9.0, 9.0]
    );
}

#[test]
fn visibility_uses_the_picked_actor_not_screen_distance() {
    let mut actor = actor();
    actor.kind = ActorKind::Entity {
        identifier: "test:npc".into(),
    };
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << ACTOR_FLAG_SHOW_NAME));
    assert!(!visible(&actor, None));
    assert!(visible(&actor, Some(actor.runtime_id)));
    actor
        .metadata
        .insert(METADATA_ALWAYS_SHOW_NAMETAG, ActorMetadataValue::Byte(1));
    assert!(visible(&actor, None));
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << ACTOR_FLAG_INVISIBLE));
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
    assert_eq!(&*tag_text(&actor, "AB".into(), 99.0, &boards), "AB\n7 wins");
    assert_eq!(&*tag_text(&actor, "AB".into(), 100.0, &boards), "AB");
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("".into()));
    assert_eq!(&*tag_text(&actor, "AB".into(), 99.0, &boards), "AB");
}

#[test]
fn empty_names_hide_players_without_screen_coordinate_visibility_rules() {
    let actor = actor();
    let eye = Vec3::new(0.0, 1.62, 5.0);
    assert!(
        extract_nametag(
            &actor,
            eye,
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
            eye,
            None,
            "".into(),
            &ui::ScoreboardStore::default(),
            0.5
        )
        .is_none()
    );
}

#[test]
fn coincident_eye_skips_only_the_degenerate_actor() {
    let actor = actor();
    let eye = tag_world_position(&actor, 0.5).unwrap();
    assert!(
        extract_nametag(
            &actor,
            eye,
            None,
            "p".into(),
            &ui::ScoreboardStore::default(),
            0.5
        )
        .is_none()
    );
}

#[test]
fn cosmetic_shadow_and_border_preserve_native_depth_partition_and_record_bound() {
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
        records: vec![plate, text, plate, text],
        see_through: 2,
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(
        serde_json::json!({"scale":2.,"shadow":true,"border_color":[1.,1.,1.,1.]}),
    )
    .unwrap();
    style_scene(&mut scene, &style, None);
    assert_eq!(scene.see_through, 7);
    assert_eq!(scene.records.len(), 14);
    assert_eq!(
        scene.records[6].color[3], 0.125,
        "sneaking alpha survives cosmetic styling"
    );
    let mut hidden = style;
    hidden.hide = true;
    style_scene(&mut scene, &hidden, None);
    assert!(scene.records.is_empty());
    assert_eq!(scene.see_through, 0);
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
        color: [1., 1., 1., 0.125],
        ..Default::default()
    };
    let remote = NametagRecord {
        anchor: [4., 5., 6.],
        ..glyph
    };
    let mut scene = NametagScene {
        records: vec![plate, glyph, remote],
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(serde_json::json!({"border_width":3.,"border_color":[1.,1.,1.,1.],"text_color":[0.,1.,0.,1.],"own_text_color":[1.,0.,0.,1.]})).unwrap();
    style_scene(&mut scene, &style, Some([1., 2., 3.]));
    assert_eq!(scene.records[0].rect, [-5., -4., 5., -1.]);
    assert_eq!(scene.records[5].color, [1., 0., 0., 0.125]);
    assert_eq!(scene.records[6].color, [0., 1., 0., 0.125]);
}

/// Replacement removes a formatted tint, while opacity still preserves native sneaking alpha.
#[test]
fn replacement_and_default_text_opacity_preserve_native_coverage() {
    let glyph = NametagRecord {
        anchor: [1., 2., 3.],
        color: [0., 1., 0., 0.125],
        uv: [0., 0., 1., 1.],
        ..Default::default()
    };
    let remote = NametagRecord {
        anchor: [4., 5., 6.],
        ..glyph
    };
    let mut scene = NametagScene {
        records: vec![glyph, remote],
        ..Default::default()
    };
    let style: ui::mod_hud::NametagStyle = serde_json::from_value(serde_json::json!({"opacity":0.5,"own_text_color":[1.,0.,0.,1.],"replace_own_text_color":true})).unwrap();
    style_scene(&mut scene, &style, Some(glyph.anchor));
    assert_eq!(scene.records[0].color, [1., 0., 0., 0.0625]);
    assert_eq!(scene.records[1].color, [0., 1., 0., 0.0625]);
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
        Vec3::new(0.0, 1.62, 5.0),
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
            Vec3::new(0.0, 1.62, 5.0),
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
    let stream = actor_stream(count, Some(score));
    let mut cache = NametagTextCache::default();
    let boards = ui::ScoreboardStore::default();
    let project = |cache: &mut NametagTextCache, show_players| {
        project_nametags(
            cache,
            &boards,
            &stream,
            Vec3::new(0.0, 1.62, 5.0),
            None,
            0.5,
            show_players,
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
        .extract(&actor, eye, None, Arc::clone(&name), &boards, 0.5)
        .unwrap();
    actor.position[0] = 1.0;
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << ACTOR_FLAG_SNEAKING));
    let (moved, allocations) = test_alloc::count(|| {
        cache
            .extract(&actor, eye, None, Arc::clone(&name), &boards, 0.5)
            .unwrap()
    });
    assert_eq!(allocations, 0);
    assert!(Arc::ptr_eq(&first.lines, &moved.lines));
    assert_ne!(first.position, moved.position);
    assert!(moved.depth_tested);
    assert_eq!(moved.text_alpha, SNEAK_TEXT_ALPHA);
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << ACTOR_FLAG_INVISIBLE));
    assert!(
        cache
            .extract(&actor, eye, None, Arc::clone(&name), &boards, 0.5)
            .is_none()
    );
    actor.metadata.remove(&0);
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("8 wins".into()));
    let changed = cache
        .extract(&actor, eye, None, Arc::clone(&name), &boards, 0.5)
        .unwrap();
    assert_eq!(changed.lines[1].as_ref(), "8 wins");
    let far = cache
        .extract(
            &actor,
            Vec3::new(0.0, 1.62, 20.0),
            None,
            Arc::clone(&name),
            &boards,
            0.5,
        )
        .unwrap();
    assert_eq!(far.lines.len(), 1);
    let renamed = cache
        .extract(&actor, eye, None, "Other".into(), &boards, 0.5)
        .unwrap();
    assert_eq!(renamed.lines[0].as_ref(), "Other");
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("".into()));
    assert_eq!(
        cache
            .extract(&actor, eye, None, name, &boards, 0.5)
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
            .extract(&actor, eye, None, Arc::clone(&name), &boards, 0.5)
            .unwrap();
        assert_eq!(tag.lines[1].as_ref(), format!("{score} wins"));
    }
    boards.clear();
    assert_eq!(
        cache
            .extract(&actor, eye, None, name, &boards, 0.5)
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
            Vec3::new(0.0, 1.62, 5.0),
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
        Vec3::new(0.0, 1.62, 5.0),
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
