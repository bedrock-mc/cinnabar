use super::*;
use render_api::primitive_shapes::{
    PrimitiveShapeChange, PrimitiveShapeData, PrimitiveShapeKind, PrimitiveShapeUpdate,
    PrimitiveShapesEvent,
};

/// Supplies native default text options with a caller-selected string.
fn text(value: &str) -> PrimitiveText {
    PrimitiveText {
        text: value.into(),
        use_rotation: false,
        background_color: None,
        line_gap_height: 0.0,
        depth_test: false,
        show_backface: true,
        show_text_backface: false,
    }
}

/// Creates one packet-like update without filling unrelated optional properties.
fn update(id: u64, data: PrimitiveShapeData) -> PrimitiveShapeUpdate {
    PrimitiveShapeUpdate {
        network_id: id,
        kind: PrimitiveShapeKind::Text,
        location: None,
        rotation: None,
        scale: None,
        color: None,
        total_time_left: None,
        maximum_render_distance: None,
        dimension: None,
        attached_actor: None,
        data,
    }
}

/// Sends one text patch through retained storage.
fn apply(store: &mut PrimitiveShapeStore, update: PrimitiveShapeUpdate) {
    store.apply(PrimitiveShapesEvent {
        changes: vec![PrimitiveShapeChange::Upsert(update)],
        skipped_entries: 0,
    });
}

#[test]
fn primitive_text_centers_lines_and_uses_one_multiline_background() {
    let lines = [
        AtlasLine {
            cell: [0, 0, 24, 16],
            width_px: 5.5,
            top_px: -1.0,
        },
        AtlasLine {
            cell: [24, 0, 8, 16],
            width_px: 2.0,
            top_px: 0.0,
        },
    ];
    let mut style = text("one\ntwo");
    style.background_color = Some([0.1, 0.2, 0.3, 0.4]);
    style.depth_test = true;
    style.use_rotation = true;
    let output = records(&style, &lines);
    assert_eq!(output.len(), 3);
    assert_eq!(output[0].rect, [-3.0, -1.0, 3.0, LINE_PITCH_PX * 2.0 - 1.0]);
    assert_eq!(output[0].color, style.background_color.unwrap());
    assert_eq!(output[1].rect[0], -2.0);
    assert_eq!(output[1].rect[1], -1.0);
    assert_eq!(output[2].rect[0], -1.0);
    assert_eq!(output[2].rect[1], LINE_PITCH_PX);
    assert_eq!(output[1].color, [1.0; 4]);
    assert_eq!(output[1].meta[2], 1 | 2 | 8);
    assert_eq!(f32::from_bits(output[1].meta[3]), EXTRA_LINE_LIFT);
}

#[test]
fn primitive_text_resolves_text_objects_and_literal_escaped_lines() {
    let runtime = UiRuntime::new(1);
    assert_eq!(
        resolve_text("first\\n\\nlast", &runtime),
        ("first\n\nlast".into(), false)
    );
    assert_eq!(
        resolve_text(
            r#"{"rawtext":[{"text":"hello"},{"text":" world"}]}"#,
            &runtime
        ),
        ("hello world".into(), true)
    );
    assert_eq!(
        resolve_text("{literal}", &runtime),
        ("{literal}".into(), false)
    );
}

#[test]
fn primitive_text_steady_and_movement_frames_keep_atlas_and_quad_slots() {
    let font = super::super::tests::fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let runtime = UiRuntime::new(1);
    let mut store = PrimitiveShapeStore::default();
    apply(
        &mut store,
        update(1, PrimitiveShapeData::Text(text("A\\n\\nB"))),
    );
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert_eq!(store.text_records.values.len(), 3);
    let atlas = Arc::clone(&store.atlas);
    let quads = store.text_records.values.clone();
    store.drain_uploads(|_, _, _, _| {});
    store.text_records.drain(|_, _| {});
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(Arc::ptr_eq(&atlas, &store.atlas));
    assert!(store.text_records.is_clean());
    let mut moved = update(1, PrimitiveShapeData::None);
    moved.location = Some([2.0, 3.0, 4.0]);
    apply(&mut store, moved);
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(Arc::ptr_eq(&atlas, &store.atlas));
    assert_eq!(store.text_records.values, quads);
    assert!(store.text_records.is_clean());
}

/// Changes one translated debug string without replacing the presentation font.
fn set_translation(runtime: &mut UiRuntime, value: &str) {
    let input = format!("shape.key={value}\n");
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input.as_bytes());
        true
    }));
}

#[test]
fn primitive_text_rawtext_refreshes_after_common_or_equal_patch_and_retains_steady_frames() {
    let font = super::super::tests::fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    let mut store = PrimitiveShapeStore::default();
    set_translation(&mut runtime, "A");
    apply(
        &mut store,
        update(
            1,
            PrimitiveShapeData::Text(text(r#"{"rawtext":[{"translate":"shape.key"}]}"#)),
        ),
    );
    presentation.prepare_primitive_text(&mut store, &runtime);
    let before_atlas = Arc::clone(&store.atlas);
    let before_quads = store.text_records.values.clone();
    store.drain_uploads(|_, _, _, _| {});
    store.text_records.drain(|_, _| {});
    set_translation(&mut runtime, "AAA");
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(Arc::ptr_eq(&before_atlas, &store.atlas));
    assert_eq!(before_quads, store.text_records.values);
    assert!(!store.has_changes());
    let mut moved = update(1, PrimitiveShapeData::None);
    moved.location = Some([1.0, 0.0, 0.0]);
    apply(&mut store, moved);
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(!Arc::ptr_eq(&before_atlas, &store.atlas));
    assert_ne!(before_quads, store.text_records.values);
    let after_atlas = Arc::clone(&store.atlas);
    store.drain_uploads(|_, _, _, _| {});
    store.text_records.drain(|_, _| {});
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(Arc::ptr_eq(&after_atlas, &store.atlas));
    assert!(!store.has_changes());
    let previous_right = store.text_records.values[0].rect[2];
    let rebuilds = store.instance_rebuilds;
    set_translation(&mut runtime, "AAAAA");
    let mut repeated = update(1, PrimitiveShapeData::None);
    repeated.location = Some([1.0, 0.0, 0.0]);
    apply(&mut store, repeated);
    assert_eq!(store.instance_rebuilds, rebuilds);
    assert_eq!(store.drain_uploads(|_, _, _, _| {}).bytes, 0);
    presentation.prepare_primitive_text(&mut store, &runtime);
    assert!(!Arc::ptr_eq(&after_atlas, &store.atlas));
    assert!(store.text_records.values[0].rect[2] > previous_right);
}
