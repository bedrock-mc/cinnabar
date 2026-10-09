//! Form packet replay against the installed UI carrier and language catalog.

use super::pack_harness;
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};
use protocol::wire::valentine::bedrock::version::v1_26_51::ModalFormRequestPacket;
use protocol::{ServerFormModel, UiEvent, WorldEvent};
use std::sync::Arc;

pub(crate) const CONTROLS: &str =
    include_str!("../../../../../protocol/fixtures/custom_form_controls.json");

/// Replays one form through packet normalization and the UI's admission path.
pub(crate) fn replay(
    player: &mut player_state::PlayerState,
    json: &str,
    old_fallback: bool,
) -> UiRuntime {
    let Some(WorldEvent::Ui(UiEvent::Form(mut event))) = protocol::into_world_event(
        ModalFormRequestPacket {
            form_id: 7,
            form_uijson: json.into(),
        }
        .into(),
        0,
    )
    .unwrap() else {
        panic!("form event");
    };
    if old_fallback {
        event.model = ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls);
    }
    let mut runtime = pack_harness::menu_runtime();
    runtime
        .apply(
            player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(event),
            },
        )
        .unwrap();
    runtime
}

#[test]
fn captured_menu_renders_every_original_button_index() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let runtime = replay(
        &mut player,
        include_str!("../../../../../protocol/fixtures/dimension_clash_menu.json"),
        false,
    );
    let nodes = pack_harness::render(&mut presentation, &runtime, [1280, 65536], 1.0);
    let texts = pack_harness::drawn_texts(&nodes);
    assert!(texts.iter().any(|text| text == "Play"));
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation
        .form_engine_frame(identity)
        .expect("captured menu rendered through JSON-UI");
    assert!(
        frame
            .hits
            .iter()
            .any(|hit| hit.collection_index == Some(345))
    );
}

#[test]
fn structured_controls_render_and_publish_a_complete_custom_response() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = replay(&mut player, CONTROLS, false);
    runtime
        .server_forms_mut()
        .engine_mut()
        .open_multiselects
        .insert(8);
    let nodes = pack_harness::render(&mut presentation, &runtime, [1280, 1440], 1.0);
    let mut labels = pack_harness::drawn_texts(&nodes);
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    scroll_to_end(&mut runtime, &frame);
    labels.extend(pack_harness::drawn_texts(&pack_harness::render(
        &mut presentation,
        &runtime,
        [1280, 1440],
        1.0,
    )));
    for label in [
        "Form controls",
        "Enabled",
        "Volume: 4",
        "Speed: Fast",
        "Alpha",
        "Beta",
        "Gamma",
    ] {
        assert!(
            labels.iter().any(|text| text.contains(label)),
            "missing {label}: {labels:?}"
        );
    }
    assert!(labels.iter().all(|text| !text.contains("rawtext")));
    let state = runtime.server_forms().engine();
    assert_eq!(
        state.submission().as_ref(),
        &[
            protocol::CustomFormValue::Null,
            protocol::CustomFormValue::Null,
            protocol::CustomFormValue::Null,
            protocol::CustomFormValue::Toggle(true),
            protocol::CustomFormValue::Slider(4.0),
            protocol::CustomFormValue::Step(1),
            protocol::CustomFormValue::Dropdown(1),
            protocol::CustomFormValue::Input("Draft".into()),
            protocol::CustomFormValue::MultiSelect(Arc::from([2, 0])),
        ]
    );
}

#[test]
fn form_compatibility_before_after_snapshots() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    for (before, name) in [(true, "forms-before"), (false, "forms-after")] {
        let mut runtime = replay(&mut player, CONTROLS, before);
        runtime
            .server_forms_mut()
            .engine_mut()
            .open_multiselects
            .insert(8);
        let input = presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 960],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        super::snapshot::write(&input, name);
        if !before {
            let identity = runtime.server_forms().active().unwrap().identity;
            let frame = presentation.form_engine_frame(identity).unwrap().clone();
            scroll_to_end(&mut runtime, &frame);
            let input = presentation
                .build(
                    &player,
                    &runtime,
                    0,
                    [1280, 960],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            super::snapshot::write(&input, "forms-after-multiselect");
        }
        assert_eq!(
            presentation
                .form_engine_frame(runtime.server_forms().active().unwrap().identity)
                .is_none(),
            before
        );
    }
}

/// Reveals the lower controls using the rendered scroll extents.
fn scroll_to_end(runtime: &mut UiRuntime, frame: &crate::ui_runtime::forms::EngineFrame) {
    for (key, metrics) in &frame.report.scrolls {
        runtime
            .server_forms_mut()
            .engine_mut()
            .view
            .scroll
            .insert(key.clone(), metrics.max_offset());
    }
}
