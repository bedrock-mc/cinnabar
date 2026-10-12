use super::*;

fn actor() -> ActorSnapshot {
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
    let spawn = protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: 1,
        runtime_id: 1,
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "p".into(),
        },
        position: [0.0; 3],
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
    };
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
        )
        .unwrap();
    stream.poll([0.0; 3], 0);
    let mut actor = stream
        .authority()
        .actor(1)
        .expect("spawn committed")
        .clone();
    actor.movement_revision = 1;
    actor
}

pub(in crate::ui_runtime::presentation) fn anchor(name: &str) -> NametagAnchor {
    NametagAnchor {
        runtime_id: 1,
        position: Vec3::new(0.0, 2.5, -5.0),
        lines: name
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(Arc::from)
            .collect(),
        depth_tested: false,
        text_alpha: 1.0,
        distance: 5.0,
    }
}

fn scene(anchors: &[NametagAnchor]) -> NametagScene {
    let font = ui_engine::test_support::fixture_font();
    build_nametag_scene(
        anchors,
        &font,
        &mut TextLayoutCache::new(8, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| super::super::nametag_atlas::font_page(&font, page),
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
