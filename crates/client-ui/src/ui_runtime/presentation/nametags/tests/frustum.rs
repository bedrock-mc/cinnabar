use super::super::bounds::intersects_view;
use super::*;
use glam::{Mat4, Quat};

/// Uses the production reverse-Z perspective convention with a square viewport.
fn clip(position: Vec3, rotation: Quat) -> Mat4 {
    glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_3,
        1.0,
        0.025,
    ) * Mat4::from_rotation_translation(rotation, position).inverse()
}

/// Sets every styling field so additions require an explicit bounds-test choice.
fn extended_style() -> ui::mod_hud::NametagStyle {
    ui::mod_hud::NametagStyle {
        hide: false,
        hide_players: false,
        opacity: 1.0,
        replace_own_text_color: true,
        replace_text_color: true,
        shadow: true,
        show_self: true,
        show_when_hud_hidden: true,
        hide_passengers: false,
        own_text_color: Some([1.0; 4]),
        border_width: 3.0,
        border_color: Some([1.0; 4]),
        scale: 2.0,
        text_color: Some([1.0; 4]),
        background_color: Some(PLATE_COLOR),
    }
}

#[test]
fn on_screen_ordinary_labels_need_no_text_measurement_for_admission() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let tag = anchor("A\n§lB\n§kAAAA");
    for style in [None, Some(extended_style())] {
        assert!(intersects_view(
            &tag,
            clip,
            Quat::IDENTITY,
            &font,
            &mut layouts,
            style.as_ref(),
        ));
    }
    assert_eq!(layouts.built_layout_count(), 0);
}

#[test]
fn label_bounds_reject_off_screen_tags_but_keep_ink_at_the_frustum_edge() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let mut tag = anchor("AAAAAA");
    assert!(intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.position.z = 5.0;
    assert!(!intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.position.z = -5.0;
    tag.position.x = 20.0;
    assert!(!intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.position.x = (std::f32::consts::FRAC_PI_6).tan() * 5.0 + 0.25;
    assert!(intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    assert!(intersects_view(
        &tag,
        Mat4::ZERO,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
}

#[test]
fn badges_counters_and_cosmetic_extensions_keep_painted_edge_fragments_admitted() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let mut tag = anchor("A");
    tag.position.x = (std::f32::consts::FRAC_PI_6).tan() * 5.0 + 0.15;
    assert!(!intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.cinnabar_badge = true;
    assert!(intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.cinnabar_badge = false;
    tag.set_counter_annotation(
        ui::mod_hud::TotemCounterStyle::default()
            .annotation(200)
            .map(Arc::new),
    );
    assert!(intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        None
    ));
    tag.set_counter_annotation(None);
    let style = serde_json::from_value(
        serde_json::json!({"border_color": [1., 1., 1., 1.], "border_width": 3.0}),
    )
    .unwrap();
    assert!(intersects_view(
        &tag,
        clip,
        Quat::IDENTITY,
        &font,
        &mut layouts,
        Some(&style)
    ));
}

#[test]
fn off_screen_near_actors_do_not_consume_the_tag_or_record_budgets() {
    let stream = actor_stream(MAX_PRESENTED_NAMETAGS * 4, None, 0);
    let boards = ui::ScoreboardStore::default();
    let font = ui_engine::test_support::fixture_font();
    for (camera, rotation) in [
        (
            Vec3::new(2.5, HEAD_CLEARANCE + DEFAULT_HEIGHT, 0.0),
            Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        ),
        (
            Vec3::new(5.0, HEAD_CLEARANCE + DEFAULT_HEIGHT, 1.0),
            Quat::IDENTITY,
        ),
    ] {
        let view = NametagView {
            viewer_position: Vec3::ZERO,
            camera_position: camera,
            camera_forward: rotation * Vec3::NEG_Z,
        };
        let clip = clip(camera, rotation);
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut cache = NametagTextCache::default();
        let unculled =
            project_nametags(&mut cache, &boards, &stream, view, None, 1.0, true, |_| {
                true
            });
        assert_eq!(unculled.len(), MAX_PRESENTED_NAMETAGS);
        assert!(unculled.iter().all(|tag| !intersects_view(
            tag,
            clip,
            rotation,
            &font,
            &mut layouts,
            None
        )));
        let tags = project_nametags(&mut cache, &boards, &stream, view, None, 1.0, true, |tag| {
            intersects_view(tag, clip, rotation, &font, &mut layouts, None)
        });
        assert!(!tags.is_empty());
        assert!(tags.len() <= MAX_PRESENTED_NAMETAGS);
        assert!(tags.iter().all(|tag| intersects_view(
            tag,
            clip,
            rotation,
            &font,
            &mut layouts,
            None
        )));
        assert_eq!(cache.entries.len(), tags.len());
        let scene = scene_with_font(&tags, &font);
        assert_eq!(scene.records.len(), tags.len() * NAMETAG_RECORDS_PER_LINE);
    }
}

#[test]
fn admission_bounds_cover_the_production_quads_for_every_label_extension() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(32, 1 << 20);
    for rotation in [
        Quat::IDENTITY,
        Quat::from_euler(glam::EulerRot::YXZ, 0.4, -0.6, 0.2),
    ] {
        let forward = rotation * Vec3::NEG_Z;
        for sneaking in [false, true] {
            for separate in [false, true] {
                for before in [false, true] {
                    let mut tag = anchor("AAAA\nB");
                    tag.depth_tested = sneaking;
                    tag.camera_forward_y = forward.y;
                    tag.cinnabar_badge = true;
                    tag.set_counter_annotation(Some(Arc::new(ui::mod_hud::CounterAnnotation {
                        prefix: Arc::from("Count: "),
                        value: Arc::from("123"),
                        prefix_color: [255; 4],
                        value_color: [255; 4],
                        separate,
                        before,
                    })));
                    let style = extended_style();
                    let mut painted = scene_with_font(&[tag.clone()], &font);
                    style_scene(&mut painted, &style, None);
                    for record in &painted.records {
                        let corners = record
                            .world_corners(rotation)
                            .unwrap()
                            .map(Vec3::from_array);
                        let center = (corners[0] + corners[2]) * 0.5;
                        let camera = center - forward * 2.0;
                        let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(
                            0.001, 1.0, 0.025,
                        ) * Mat4::from_rotation_translation(rotation, camera).inverse();
                        tag.view_depth = forward.dot(tag.position - camera);
                        assert!(
                            intersects_view(
                                &tag,
                                clip,
                                rotation,
                                &font,
                                &mut layouts,
                                Some(&style)
                            ),
                            "rotation {rotation:?}, sneak {sneaking}, separate {separate}, before {before}, record {record:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn static_behind_camera_labels_do_not_reshape_after_warmup() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts =
        TextLayoutCache::new(crate::ui_runtime::presentation::TEXT_CACHE_ENTRIES, 8 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let tags: Vec<_> = (0..MAX_PRESENTED_NAMETAGS)
        .map(|id| {
            let mut tag = anchor("A");
            tag.runtime_id = id as u64;
            tag.position.z = 5.0;
            tag.lines = (0..10)
                .map(|line| Arc::from(format!("Tag {id} line {line}")))
                .collect();
            tag
        })
        .collect();
    assert!(
        tags.iter().map(|tag| tag.lines.len()).sum::<usize>()
            > crate::ui_runtime::presentation::TEXT_CACHE_ENTRIES
    );
    for tag in &tags {
        assert!(!intersects_view(
            tag,
            clip,
            Quat::IDENTITY,
            &font,
            &mut layouts,
            None
        ));
    }
    let built = layouts.built_layout_count();
    for _ in 0..2 {
        for tag in &tags {
            assert!(!intersects_view(
                tag,
                clip,
                Quat::IDENTITY,
                &font,
                &mut layouts,
                None
            ));
        }
        assert_eq!(layouts.built_layout_count(), built);
    }
}
