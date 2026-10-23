use super::super::bounds::BoundsCache;
use super::*;
use glam::{Mat4, Quat};

#[test]
fn unchanged_full_projection_reuses_on_and_off_screen_text_without_allocating() {
    let stream = actor_stream(6, Some("A\n".repeat(MAX_NAMETAG_LINES - 1).into()), 0);
    let boards = ui::ScoreboardStore::default();
    let camera = Vec3::new(2.5, HEAD_CLEARANCE + DEFAULT_HEIGHT, 1.0);
    let clip = clip(camera, Quat::IDENTITY);
    let mut presentation = crate::test_support::mini_engine_presentation();
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let mut text = NametagTextCache::default();
    let mut rejected = 0;
    let mut project =
        |text: &mut NametagTextCache,
         presentation: &mut crate::ui_runtime::presentation::UiPresentationRuntime| {
            presentation.prepare_nametag_layout(&runtime);
            let tags = project_nametags(
                text,
                &boards,
                &stream,
                view(camera),
                None,
                1.0,
                true,
                |tag| {
                    let visible = presentation.nametag_intersects_view(tag, clip, Quat::IDENTITY);
                    rejected += usize::from(!visible);
                    visible
                },
            );
            text.recycle_anchors(presentation.set_nametag_anchors(tags));
        };
    project(&mut text, &mut presentation);
    let expected = presentation.nametag_anchors.clone();
    assert!(!expected.is_empty() && expected.len() < 6);
    let cached: Vec<_> = text
        .entries
        .values()
        .map(|entry| Arc::clone(&entry.lines))
        .collect();
    assert_eq!(cached.len(), 6);
    let built = presentation.layouts.built_layout_count();
    let (_, allocations) = test_alloc::count(|| project(&mut text, &mut presentation));
    assert_eq!(expected, presentation.nametag_anchors);
    assert!(rejected != 0);
    assert_eq!(allocations, 0);
    assert_eq!(presentation.layouts.built_layout_count(), built);
    assert!(
        text.entries
            .values()
            .all(|entry| cached.iter().any(|lines| Arc::ptr_eq(lines, &entry.lines)))
    );
}

#[test]
fn newline_only_names_check_score_visibility_without_building_text() {
    let mut actor = actor();
    let boards = ui::ScoreboardStore::default();
    for bytes in [
        1,
        protocol::MAX_ACTOR_METADATA_STRING_BYTES - 2,
        protocol::MAX_ACTOR_METADATA_STRING_BYTES - 1,
    ] {
        let name: Arc<str> = "\n".repeat(bytes).into();
        for score in ["", "\n", "7", "§a7"] {
            actor
                .metadata
                .insert(METADATA_SCORE, ActorMetadataValue::String(score.into()));
            let expected =
                !nametag_lines(&tag_text(&actor, Arc::clone(&name), 1.0, Some(&boards))).is_empty();
            let (visible, allocations) =
                test_alloc::count(|| has_nametag_lines(&actor, &name, 1.0, Some(&boards)));
            assert_eq!(visible, expected, "name bytes {bytes}, score {score:?}");
            assert_eq!(allocations, 0);
        }
    }
}

#[test]
fn candidate_bounds_stay_warm_beyond_the_shared_layout_cache() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    let tags: Vec<_> = (0..3)
        .map(|id| {
            let mut tag = anchor("A");
            tag.runtime_id = id;
            tag.lines = (0..MAX_NAMETAG_LINES)
                .map(|line| Arc::from(format!("Tag {id} line {line}")))
                .collect();
            tag
        })
        .collect();
    assert!(
        tags.iter().map(|tag| tag.lines.len()).sum::<usize>()
            > crate::ui_runtime::presentation::TEXT_CACHE_ENTRIES
    );
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    presentation.prepare_nametag_layout(&runtime);
    for tag in &tags {
        assert!(presentation.nametag_intersects_view(tag, clip, Quat::IDENTITY));
    }
    presentation.set_nametag_anchors(tags.clone());
    assert_eq!(
        presentation.nametag_scene().records.len(),
        MAX_NAMETAG_RECORDS
    );
    let built = presentation.layouts.built_layout_count();
    let (_, allocations) = test_alloc::count(|| {
        presentation.prepare_nametag_layout(&runtime);
        for tag in &tags {
            assert!(presentation.nametag_intersects_view(tag, clip, Quat::IDENTITY));
        }
        presentation.set_nametag_anchors(Vec::new());
    });
    assert_eq!(presentation.layouts.built_layout_count(), built);
    assert_eq!(allocations, 0);
}

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
fn label_bounds_reject_off_screen_tags_but_keep_ink_at_the_frustum_edge() {
    let font = ui_engine::test_support::fixture_font();
    let mut cache = BoundsCache::default();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let mut tag = anchor("AAAAAA");
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.position.z = 5.0;
    assert!(!cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.position.z = -5.0;
    tag.position.x = 20.0;
    assert!(!cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.position.x = (std::f32::consts::FRAC_PI_6).tan() * 5.0 + 0.25;
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    assert!(cache.intersects_view(&tag, Mat4::ZERO, Quat::IDENTITY, &font, &mut layouts, None));
}

#[test]
fn badges_counters_and_cosmetic_extensions_keep_painted_edge_fragments_admitted() {
    let font = ui_engine::test_support::fixture_font();
    let mut cache = BoundsCache::default();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let mut tag = anchor("A");
    tag.position.x = (std::f32::consts::FRAC_PI_6).tan() * 5.0 + 0.15;
    assert!(!cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.cinnabar_badge = true;
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.cinnabar_badge = false;
    tag.set_counter_annotation(
        ui::mod_hud::TotemCounterStyle::default()
            .annotation(200)
            .map(Arc::new),
    );
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.set_counter_annotation(None);
    let style = serde_json::from_value(
        serde_json::json!({"border_color": [1., 1., 1., 1.], "border_width": 3.0}),
    )
    .unwrap();
    assert!(cache.intersects_view(
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
    let mut cache = BoundsCache::default();
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
            camera_position: camera,
            camera_forward: rotation * Vec3::NEG_Z,
        };
        let clip = clip(camera, rotation);
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut text_cache = NametagTextCache::default();
        let unculled = project_nametags(
            &mut text_cache,
            &boards,
            &stream,
            view,
            None,
            1.0,
            true,
            |_| true,
        );
        assert_eq!(unculled.len(), MAX_PRESENTED_NAMETAGS);
        assert!(unculled.iter().any(|tag| !cache.intersects_view(
            tag,
            clip,
            rotation,
            &font,
            &mut layouts,
            None
        )));
        let visible_candidates = stream
            .authority()
            .remote_actors()
            .filter_map(|actor| {
                extract_nametag(
                    actor,
                    view,
                    None,
                    stream.authority().actor_name_tag(actor.unique_id)?,
                    &boards,
                    1.0,
                )
            })
            .filter(|tag| cache.intersects_view(tag, clip, rotation, &font, &mut layouts, None))
            .count();
        let tags = project_nametags(
            &mut text_cache,
            &boards,
            &stream,
            view,
            None,
            1.0,
            true,
            |tag| cache.intersects_view(tag, clip, rotation, &font, &mut layouts, None),
        );
        assert!(visible_candidates != 0);
        assert_eq!(tags.len(), visible_candidates.min(MAX_PRESENTED_NAMETAGS));
        assert!(tags.iter().all(|tag| cache.intersects_view(
            tag,
            clip,
            rotation,
            &font,
            &mut layouts,
            None
        )));
        assert!(text_cache.entries.len() >= tags.len());
        let scene = scene_with_font(&tags, &font);
        assert_eq!(scene.records.len(), tags.len() * NAMETAG_RECORDS_PER_LINE);
    }
}

#[test]
fn admission_bounds_cover_the_production_quads_for_every_label_extension() {
    let font = ui_engine::test_support::fixture_font();
    let mut cache = BoundsCache::default();
    let mut layouts = TextLayoutCache::new(32, 1 << 20);
    for rotation in [
        Quat::IDENTITY,
        Quat::from_euler(glam::EulerRot::YXZ, 0.4, -0.6, 0.2),
    ] {
        let forward = rotation * Vec3::NEG_Z;
        for (sneaking, depth_tested) in [(false, false), (true, true), (false, true)] {
            for separate in [false, true] {
                for before in [false, true] {
                    let mut tag = anchor("AAAA\nB");
                    tag.sneaking = sneaking;
                    tag.depth_tested = depth_tested;
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
                            cache.intersects_view(
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
fn on_screen_ordinary_labels_need_no_text_measurement_for_admission() {
    let mut cache = BoundsCache::default();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let clip = clip(Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY);
    let tag = anchor("A\n§lB\n§kAAAA");
    for style in [None, Some(extended_style())] {
        assert!(cache.intersects_view(
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
fn static_behind_camera_labels_do_not_reshape_after_warmup() {
    let mut cache = BoundsCache::default();
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
    cache.begin_frame();
    for tag in &tags {
        assert!(!cache.intersects_view(tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    }
    cache.finish_frame();
    let built = layouts.built_layout_count();
    for rotation in [
        Quat::IDENTITY,
        Quat::from_rotation_y(std::f32::consts::PI),
        Quat::IDENTITY,
    ] {
        let visible = rotation != Quat::IDENTITY;
        let clip = self::clip(Vec3::new(0.0, 2.5, 0.0), rotation);
        cache.begin_frame();
        for tag in &tags {
            assert_eq!(
                cache.intersects_view(tag, clip, rotation, &font, &mut layouts, None),
                visible
            );
        }
        cache.finish_frame();
        assert_eq!(layouts.built_layout_count(), built);
    }
}
