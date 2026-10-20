use super::*;

/// Places a spawned player's current and previous feet at one stable point.
fn placed_actor(runtime_id: u64, position: Vec3) -> ActorSnapshot {
    let mut actor = actor();
    actor.runtime_id = runtime_id;
    actor.unique_id = runtime_id as i64;
    actor.position = position.to_array();
    actor.previous_pose.position = actor.position;
    actor.received_pose.position = actor.position;
    actor
}

#[test]
fn third_person_labels_sort_by_camera_depth_instead_of_viewer_distance() {
    let camera = NametagView {
        viewer_position: Vec3::ZERO,
        camera_position: Vec3::new(0.0, 1.62, 4.0),
        camera_forward: Vec3::NEG_Z,
    };
    let boards = ui::ScoreboardStore::default();
    let far = extract_nametag(
        &placed_actor(1, Vec3::Z),
        camera,
        None,
        "AB".into(),
        &boards,
        1.0,
    )
    .unwrap();
    let near = extract_nametag(
        &placed_actor(2, Vec3::Z * 3.0),
        camera,
        None,
        "AB".into(),
        &boards,
        1.0,
    )
    .unwrap();
    assert_eq!(far.view_depth, 3.0);
    assert_eq!(near.view_depth, 1.0);
    let far_anchor = far.position.to_array();
    let scene = scene(&[near, far]);
    let mut pixel = 0.0;
    // Both plates cover the sample, with far glyph ink beneath a blank near glyph.
    for record in &scene.records {
        if record.text != 0 && record.anchor != far_anchor {
            continue;
        }
        let alpha = record.color[3];
        pixel = record.color[0] * alpha + pixel * (1.0 - alpha);
    }
    assert!((pixel - 0.75).abs() < 1e-6);
}

#[test]
fn camera_depth_ignores_sideways_distance_from_the_view_axis() {
    let camera = view(Vec3::ZERO);
    let boards = ui::ScoreboardStore::default();
    let offset = Vec3::new(10.0, 0.0, -2.0);
    let centered = Vec3::new(0.0, 0.0, -4.0);
    let near = extract_nametag(
        &placed_actor(1, offset),
        camera,
        None,
        "AB".into(),
        &boards,
        1.0,
    )
    .unwrap();
    let far = extract_nametag(
        &placed_actor(2, centered),
        camera,
        None,
        "AB".into(),
        &boards,
        1.0,
    )
    .unwrap();
    assert!(offset.length_squared() > centered.length_squared());
    assert_eq!(near.view_depth, 2.0);
    assert_eq!(far.view_depth, 4.0);
    let scene = scene(&[near, far]);
    assert_eq!(scene.records[0].anchor[2], centered.z);
}

#[test]
fn camera_changes_refresh_cached_depth_without_changing_viewer_score_range() {
    let mut actor = placed_actor(1, Vec3::Z);
    actor
        .metadata
        .insert(METADATA_SCORE, ActorMetadataValue::String("7 wins".into()));
    let boards = ui::ScoreboardStore::default();
    let mut cache = NametagTextCache::default();
    let name = Arc::from("AB");
    let first_view = NametagView {
        viewer_position: Vec3::ZERO,
        camera_position: Vec3::Z * 100.0,
        camera_forward: Vec3::NEG_Z,
    };
    let first = cache
        .extract(&actor, first_view, None, Arc::clone(&name), &boards, 1.0)
        .unwrap();
    assert_eq!(first.view_depth, 99.0);
    assert_eq!(first.lines.len(), 2);
    let turned = NametagView {
        camera_position: Vec3::X * 4.0,
        camera_forward: Vec3::NEG_X,
        ..first_view
    };
    let next = cache
        .extract(&actor, turned, None, Arc::clone(&name), &boards, 1.0)
        .unwrap();
    assert_eq!(next.view_depth, 4.0);
    assert!(Arc::ptr_eq(&first.lines, &next.lines));
    let pitched = cache
        .extract(
            &actor,
            NametagView {
                camera_forward: Vec3::new(0.0, -1.0, -1.0).normalize(),
                ..turned
            },
            None,
            Arc::clone(&name),
            &boards,
            1.0,
        )
        .unwrap();
    assert!(pitched.camera_forward_y < 0.0);
    assert!(Arc::ptr_eq(&next.lines, &pitched.lines));
    let viewer_out_of_range = NametagView {
        viewer_position: Vec3::NEG_Z * 64.0,
        ..turned
    };
    assert!(
        cache
            .extract(&actor, viewer_out_of_range, None, name, &boards, 1.0)
            .is_none()
    );
}

#[test]
fn nearer_plates_attenuate_farther_text_in_every_sneak_combination() {
    for far_sneaking in [false, true] {
        for near_sneaking in [false, true] {
            let mut far = anchor("AB");
            far.runtime_id = 2;
            far.position.z = -8.0;
            far.view_depth = 8.0;
            far.depth_tested = far_sneaking;
            far.text_alpha = if far_sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let mut near = anchor("AB");
            near.position.z = -4.0;
            near.view_depth = 4.0;
            near.depth_tested = near_sneaking;
            near.text_alpha = if near_sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let scene = scene(&[near, far]);
            let mut pixel = 0.0;
            let mut depth = f32::INFINITY;
            // Both plates cover this pixel; the far glyph has ink and the near glyph is blank.
            for record in &scene.records {
                let coverage = if record.text != 0 && record.anchor[2] == -4.0 {
                    0.0
                } else {
                    1.0
                };
                let alpha = record.color[3] * coverage;
                let tested = record.depth_tested != 0;
                let at = -record.anchor[2];
                if coverage <= render_model::NAMETAG_ALPHA_CUTOFF || (tested && at > depth) {
                    continue;
                }
                pixel = record.color[0] * alpha + pixel * (1.0 - alpha);
                if tested && record.text != 0 {
                    depth = at;
                }
            }
            let far_ink = if far_sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            assert!(
                (pixel - far_ink * 0.75).abs() < 1e-6,
                "far sneak {far_sneaking}, near sneak {near_sneaking}: {pixel}"
            );
        }
    }
}

/// Builds and styles a scene with the same lift scale used for sorting its planes.
fn scaled_scene(anchors: &[NametagAnchor], scale: f32) -> NametagScene {
    let font = ui_engine::test_support::fixture_font();
    let mut scene = build_nametag_scene(
        anchors,
        scale,
        &font,
        &mut TextLayoutCache::new(8, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| super::super::super::nametag_atlas::font_page(&font, page),
    );
    let style = serde_json::from_value(serde_json::json!({"scale": scale})).unwrap();
    style_scene(&mut scene, &style, None);
    scene
}

#[test]
fn pitched_camera_sorts_lifted_multiline_planes_and_keeps_label_passes_together() {
    let forward = Vec3::new(0.0, -1.0, -1.0).normalize();
    for scale in [1.0, 2.0] {
        for sneaking in [false, true] {
            let mut multiline = anchor("A\nA\nA\nA\nA");
            multiline.position = forward * 5.0;
            multiline.view_depth = 5.0;
            multiline.camera_forward_y = forward.y;
            multiline.depth_tested = sneaking;
            multiline.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let mut single = anchor("A");
            single.runtime_id = 2;
            // At scale two the unscaled lift would leave the multiline label farther away.
            let single_depth = if scale == 1.0 { 4.8 } else { 4.5 };
            single.position = forward * single_depth;
            single.view_depth = single_depth;
            single.camera_forward_y = forward.y;
            let near_anchor = multiline.position.to_array();
            let far_anchor = single.position.to_array();
            let scene = scaled_scene(&[multiline, single], scale);
            assert!(
                scene.records[..NAMETAG_RECORDS_PER_LINE]
                    .iter()
                    .all(|record| record.anchor == far_anchor)
            );
            assert!(
                scene.records[NAMETAG_RECORDS_PER_LINE..]
                    .iter()
                    .all(|record| record.anchor == near_anchor)
            );
            let first_glyph = scene
                .records
                .iter()
                .find(|record| record.anchor == near_anchor && record.text != 0)
                .unwrap();
            let sample = [
                (first_glyph.rect[0] + first_glyph.rect[2]) * 0.5,
                first_glyph.rect[1] + scale * 0.5,
            ];
            let mut pixel = 0.0;
            let mut depth = f32::INFINITY;
            let mut near_text_samples = 0;
            // Sample ink in the near label's first line over a blank far glyph.
            // Other near lines do not cover this pixel, including after sneak drop and scaling.
            for record in &scene.records {
                let covered = if record.anchor == near_anchor {
                    sample[0] >= record.rect[0]
                        && sample[0] < record.rect[2]
                        && sample[1] >= record.rect[1]
                        && sample[1] < record.rect[3]
                } else {
                    record.text == 0
                };
                let alpha = if covered { record.color[3] } else { 0.0 };
                let plane = Vec3::from_array(record.anchor) + Vec3::Y * record.line_lift;
                let at = forward.dot(plane);
                if !covered || (record.depth_tested != 0 && at > depth) {
                    continue;
                }
                pixel = record.color[0] * alpha + pixel * (1.0 - alpha);
                if record.anchor == near_anchor && record.text != 0 {
                    near_text_samples += 1;
                }
                if record.depth_tested != 0 && record.text != 0 {
                    depth = at;
                }
            }
            assert_eq!(near_text_samples, 1);
            let near_ink = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            assert!(
                (pixel - near_ink).abs() < 1e-6,
                "scale {scale}, sneak {sneaking}: pixel {pixel}, expected {near_ink}"
            );
        }
    }
}

#[test]
fn lifted_sorting_uses_the_line_count_that_fits_the_record_budget() {
    let forward = Vec3::new(0.0, 1.0, -1.0).normalize();
    let mut single = anchor("A");
    let placed_lines = (MAX_NAMETAG_RECORDS - NAMETAG_RECORDS_PER_LINE) / NAMETAG_RECORDS_PER_LINE;
    let full_depth = 5.0 + forward.y * EXTRA_LINE_LIFT * (MAX_NAMETAG_LINES - 1) as f32;
    let truncated_depth = 5.0 + forward.y * EXTRA_LINE_LIFT * (placed_lines - 1) as f32;
    single.view_depth = (full_depth + truncated_depth) * 0.5;
    single.position = forward * single.view_depth;
    let single_depth = single.view_depth;
    single.camera_forward_y = forward.y;
    let mut multiline = anchor(&vec!["A"; MAX_NAMETAG_LINES].join("\n"));
    multiline.runtime_id = 2;
    multiline.position = forward * 5.0;
    multiline.view_depth = 5.0;
    multiline.camera_forward_y = forward.y;
    let far_anchor = single.position.to_array();
    let near_anchor = multiline.position.to_array();
    let scene = scene(&[multiline, single]);
    // The requested lift would put the multiline label beyond the single-line plane.
    // Its budget-truncated lift leaves it nearer, so the single label still draws first.
    assert!(
        scene.records[..NAMETAG_RECORDS_PER_LINE]
            .iter()
            .all(|record| record.anchor == far_anchor)
    );
    assert_eq!(
        scene.records.len(),
        NAMETAG_RECORDS_PER_LINE + placed_lines * NAMETAG_RECORDS_PER_LINE
    );
    for record in &scene.records[NAMETAG_RECORDS_PER_LINE..] {
        assert_eq!(record.anchor, near_anchor);
        assert_eq!(
            record.line_lift,
            EXTRA_LINE_LIFT * (placed_lines - 1) as f32
        );
        assert!(
            forward.dot(Vec3::from_array(record.anchor) + Vec3::Y * record.line_lift)
                < single_depth
        );
    }
}
