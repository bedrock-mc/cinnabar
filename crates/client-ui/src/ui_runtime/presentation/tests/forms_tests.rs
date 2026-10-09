use super::{SequencedUiEvent, UiPresentationRuntime, UiRuntime, fixture_font};
use crate::ui_runtime::{FormRespondError, LocalFormAction};
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent};
use std::sync::Arc;

fn form_runtime(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 7,
                    kind: FormKind::Menu,
                    title: Some(Arc::from("世界")),
                    json: Arc::from("{}"),
                    model: ServerFormModel::TextMenu(TextMenuForm {
                        title: protocol::FormText::from("Unicode 世界 ✓"),
                        content: protocol::FormText::from("long body α β\n".repeat(100)),
                        buttons: (0..256)
                            .map(|index| {
                                protocol::FormText::from(format!(
                                    "Button {index} 世界 {}",
                                    "x".repeat(100)
                                ))
                            })
                            .collect::<Vec<_>>()
                            .into(),
                        button_images: [].into(),
                        omitted_images: 2,
                    }),
                }),
            },
        )
        .unwrap();
    runtime
}

#[test]
fn form_scroll_layout_has_bounded_actionable_rows_at_narrow_and_large_scale() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for (size, scale) in [
        ([1280, 720], 2),
        ([1280, 720], 3),
        ([420, 720], 2),
        ([320, 240], 4),
    ] {
        let mut runtime = form_runtime(&mut player_runtime);
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_gui_scale_preference(Some(scale));
        let frame = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(
            frame.vertices.len() <= ui::UiLimits::MAX_UI_VERTICES,
            "bounded form text stays within the draw geometry budget"
        );
        assert!(
            frame.batches.iter().all(|batch| {
                batch.scissor.x.saturating_add(batch.scissor.width) <= size[0]
                    && batch.scissor.y.saturating_add(batch.scissor.height) <= size[1]
            }),
            "form text is clipped inside the viewport"
        );
        let state = &presentation.form_presentation;
        assert_eq!(state.offsets.len(), 257);
        assert!(state.maximum > 0);
        assert!(state.hits.iter().all(|(_, bounds)| bounds.min().x() >= 0.0
            && bounds.max().x() <= size[0] as f32
            && bounds.min().y() >= 0.0
            && bounds.max().y() <= size[1] as f32));
        let id = runtime.server_forms().active().unwrap().identity;
        let bottom = presentation.form_focus_scroll(id, 255).unwrap();
        runtime.server_forms_mut().set_scroll(bottom);
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(
            presentation
                .form_presentation
                .hits
                .iter()
                .any(|(action, _)| *action == LocalFormAction::SubmitButton(255))
        );
        assert!(
            presentation
                .form_presentation
                .hits
                .iter()
                .any(|(action, _)| *action == LocalFormAction::Dismiss)
        );
    }
}

#[test]
fn stale_pointer_hit_after_same_id_reissue_cannot_answer_new_content() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = form_runtime(&mut player_runtime);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let bounds = presentation
        .form_presentation
        .hits
        .iter()
        .find(|(action, _)| *action == LocalFormAction::Dismiss)
        .unwrap()
        .1;
    let captured = presentation.hit_test_form(bounds.min()).unwrap();
    let replacement = form_runtime(&mut player_runtime)
        .server_forms()
        .active()
        .unwrap()
        .clone();
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 1,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 7,
                    kind: FormKind::Menu,
                    title: replacement.title,
                    json: Arc::from("{}"),
                    model: replacement.model,
                }),
            },
        )
        .unwrap();
    assert_eq!(
        runtime.respond_to_server_form(captured.0, captured.1),
        Err(FormRespondError::StaleIdentity)
    );
}

#[test]
fn omitted_images_add_a_controlled_text_notice_without_changing_buttons() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let with_notice = form_runtime(&mut player_runtime);
    let mut model = with_notice.server_forms().active().unwrap().model.clone();
    let ServerFormModel::TextMenu(menu) = &mut model else {
        unreachable!()
    };
    menu.omitted_images = 0;
    let mut without_notice = UiRuntime::new(1);
    without_notice
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 7,
                    kind: FormKind::Menu,
                    title: Some(Arc::from("世界")),
                    json: Arc::from("{}"),
                    model,
                }),
            },
        )
        .unwrap();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let plain = presentation
        .build(
            &player_runtime,
            &without_notice,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let decorated = presentation
        .build(
            &player_runtime,
            &with_notice,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        decorated.vertices.len() > plain.vertices.len(),
        "the omission notice must actually render"
    );
    assert_eq!(
        presentation.form_button_count(with_notice.server_forms().active().unwrap().identity),
        Some(256)
    );
}

#[test]
fn image_notice_does_not_consume_the_valid_content_text_budget_or_hide_buttons() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut model = form_runtime(&mut player_runtime)
        .server_forms()
        .active()
        .unwrap()
        .model
        .clone();
    let ServerFormModel::TextMenu(menu) = &mut model else {
        unreachable!()
    };
    menu.content = "x".repeat(protocol::MAX_UI_TEXT_BYTES).into();
    assert_eq!(menu.content.len(), ui::UiLimits::MAX_TEXT_BYTES);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 7,
                    kind: FormKind::Menu,
                    title: None,
                    json: Arc::from("{}"),
                    model,
                }),
            },
        )
        .unwrap();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    for (size, scale) in [
        ([1280, 720], 2),
        ([1280, 720], 3),
        ([420, 720], 2),
        ([320, 240], 4),
    ] {
        presentation.set_gui_scale_preference(Some(scale));
        runtime.server_forms_mut().set_scroll(0);
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert_eq!(
            presentation.form_button_count(identity),
            Some(256),
            "a controlled notice must not turn valid buttons into cancel-only"
        );
        runtime
            .server_forms_mut()
            .set_scroll(presentation.form_focus_scroll(identity, 255).unwrap());
        let frame = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(presentation.form_button_visible(identity, 255));
        assert!(frame.vertices.len() <= ui::UiLimits::MAX_UI_VERTICES);
        assert!(frame.batches.iter().all(
            |batch| batch.scissor.x.saturating_add(batch.scissor.width) <= size[0]
                && batch.scissor.y.saturating_add(batch.scissor.height) <= size[1]
        ));
    }
}
