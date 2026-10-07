use super::*;
use crate::ui_runtime::presentation::tests::{fixture_font, fixture_hud};
use bevy::prelude::Transform;
use client_presentation::camera::{ServerCameraView, ViewContext};
use client_ui::ui_runtime::presentation::refresh_hud_frame;
use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};
use semantic_input::PerspectiveMode;

#[test]
fn free_camera_retires_cpu_hand_draws_and_clear_restores_them() {
    let player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(Some(&vec![255; 64 * 64 * 4]), Default::default());
    refresh_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        None,
        PerspectiveMode::FirstPerson,
        0,
    );
    let hand = presentation.hud_frame().right_hand.expect("hand carrier");
    let mut camera = ServerCameraView::default();
    let context = ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 70.0,
        actors: &|_| None,
    };
    camera.apply(
        1,
        &CameraEvent::Presets(
            vec![CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    for (sequence, instruction, expected) in [
        (2, CameraInstructionEvent::default(), true),
        (
            3,
            CameraInstructionEvent {
                set: Some(CameraSetInstruction {
                    preset_id: 0,
                    ease: None,
                    position: None,
                    rotation_degrees: None,
                    facing_position: None,
                    view_offset: None,
                    entity_offset: None,
                    default_preset: None,
                    remove_ignore_starting_values: false,
                }),
                ..Default::default()
            },
            false,
        ),
        (
            4,
            CameraInstructionEvent {
                clear: Some(true),
                ..Default::default()
            },
            true,
        ),
    ] {
        camera.apply(
            sequence,
            &CameraEvent::Instruction(Box::new(instruction)),
            &context,
        );
        presentation.hud_frame_mut().first_person =
            hand_first_person(PerspectiveMode::FirstPerson, Some(&camera));
        let input = presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let draws_hand = input.batches.iter().any(|batch| {
            batch.texture_page == u32::from(hand.page)
                && input.indices
                    [batch.first_index as usize..(batch.first_index + batch.index_count) as usize]
                    .iter()
                    .any(|&index| {
                        input.vertices[index as usize].uv
                            == [f32::from(hand.uv[0]), f32::from(hand.uv[1])]
                    })
        });
        assert_eq!(draws_hand, expected, "camera instruction {sequence}");
    }
}
