//! Original indexed packs exercise the client adapter without installed assets.

use std::{path::Path, sync::Arc};

use assets::{RuntimeUiAssets, UiAtlasPage, UiFile, encode_ui_catalog};
use json_ui::Draw;
use protocol::{ObjectiveEvent, ScoreAction, ScoreEntry, ScoreEvent, ScoreIdentity, UiEvent};
use ui::{DpiScale, UiPoint};

use super::super::{ServerUiPack, pack_harness, snapshot};
use crate::ui_runtime::{
    SequencedUiEvent, UiRuntime, flush_form_response,
    forms::engine_input::{EngineInput, PointerButtons, drive},
    presentation::UiPresentationRuntime,
};

const GREEN: [u8; 4] = [24, 156, 80, 255];
const BLUE: [u8; 4] = [32, 88, 192, 255];
const BACKDROP: [u8; 4] = [70, 90, 110, 255];
const VIEWPORT: [u32; 2] = [960, 540];

/// Locates the committed original pack layers, including their definition indexes.
fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/scoreboard")
}

/// Enables the original HUD and form documents beneath the shipped built-in HUD.
fn presentation() -> UiPresentationRuntime {
    let mut files: Vec<_> = pack_harness::pack_files(&fixtures().join("base"))
        .into_iter()
        .map(|(path, bytes)| UiFile {
            path: path.into(),
            bytes: bytes.into(),
        })
        .collect();
    files.push(UiFile {
        path: json_ui::GLOBAL_VARIABLES_PATH.into(),
        bytes: b"{}".as_slice().into(),
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let bytes = encode_ui_catalog(
        [2; 32],
        &[UiAtlasPage {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        &[],
        &[],
        &files,
    )
    .unwrap();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation
        .enable_json_ui(Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    presentation
}

/// Supplies tiny original artwork for both replacement styles.
fn pack(patched: bool) -> ServerUiPack {
    let root = fixtures();
    let mut pack = if patched {
        pack_harness::dir_pack([root.join("replacement"), root.join("patch")])
    } else {
        pack_harness::dir_pack([root.join("replacement")])
    };
    pack.textures
        .extend(
            [("sidebar_green", GREEN), ("sidebar_blue", BLUE)].map(|(name, color)| {
                let mut png = std::io::Cursor::new(Vec::new());
                image::RgbaImage::from_pixel(2, 2, image::Rgba(color))
                    .write_to(&mut png, image::ImageFormat::Png)
                    .unwrap();
                (format!("textures/fixture/{name}.png"), png.into_inner())
            }),
        );
    pack
}

/// Admits a server UI event through the same session sequencing as live input.
fn apply(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    seq: u64,
    event: UiEvent,
) {
    runtime
        .apply(
            player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: seq,
                local_millis: 0,
                server_tick: None,
                event,
            },
        )
        .unwrap();
}

/// Populates real scoreboard collections and the HUD title through server events.
fn session(player: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    apply(
        player,
        &mut runtime,
        1,
        UiEvent::Objective(ObjectiveEvent::Display {
            display_slot: "sidebar".into(),
            objective_name: "round".into(),
            display_name: "Standings".into(),
            criteria_name: "dummy".into(),
            sort_order: 1,
        }),
    );
    apply(
        player,
        &mut runtime,
        2,
        UiEvent::Score(ScoreEvent {
            entries: [("Amber", 791), ("Birch", 263)]
                .into_iter()
                .enumerate()
                .map(|(index, (label, score))| ScoreEntry {
                    action: ScoreAction::Change,
                    scoreboard_id: index as i64,
                    objective_name: "round".into(),
                    score,
                    identity: ScoreIdentity::FakePlayer(label.into()),
                })
                .collect(),
        }),
    );
    apply(
        player,
        &mut runtime,
        3,
        UiEvent::Title(protocol::TitleEvent {
            action: protocol::TitleAction::SetTitle,
            text: "Next round".into(),
            document: None,
            fade_in_ticks: 0,
            stay_ticks: 100,
            fade_out_ticks: 0,
            xuid: "".into(),
            platform_online_id: "".into(),
            filtered_message: "".into(),
        }),
    );
    runtime
}

/// Renders final client draw input, retaining its text and click projections.
fn render(
    presentation: &mut UiPresentationRuntime,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> render_model::UiRenderInput {
    let input = presentation
        .build(player, runtime, 500, VIEWPORT, DpiScale::new(1.0).unwrap())
        .unwrap();
    input.validate().unwrap();
    input
}

/// Checks a small interior patch in authored GUI coordinates after final rasterization.
fn region(pixels: &image::RgbaImage, scale: u8, origin: [u32; 2], color: [u8; 4]) {
    for y in origin[1]..origin[1] + 3 {
        for x in origin[0]..origin[0] + 3 {
            assert_eq!(
                pixels
                    .get_pixel(x * u32::from(scale), y * u32::from(scale))
                    .0,
                color,
                "GUI scale {scale}, region {origin:?}, pixel {x},{y}"
            );
        }
    }
}

/// Clicks the second displayed button and checks the exact outgoing form answer.
fn submit(presentation: &UiPresentationRuntime, runtime: &mut UiRuntime) {
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation
        .form_engine_frame(identity)
        .expect("form displayed");
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection_index == Some(1))
        .expect("second button displayed");
    let cursor = UiPoint::new(
        frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
    )
    .unwrap();
    assert!(hit.contains(frame.to_virtual(cursor)));
    for down in [true, false] {
        drive(
            runtime,
            frame,
            EngineInput {
                cursor: Some(cursor),
                keys: &[],
                pointer: PointerButtons {
                    pressed: down,
                    released: !down,
                    held: down,
                },
                pointer_edges: vec![down],
                wheel: Vec::new(),
                typed: Vec::new(),
                now: if down { 0.5 } else { 0.51 },
                animator: None,
            },
        );
    }
    let mut sent = Vec::new();
    assert!(
        flush_form_response(
            runtime,
            |_| false,
            |packet| {
                sent.push(packet);
                Ok(())
            }
        )
        .unwrap()
    );
    assert_eq!(
        sent,
        [protocol::modal_form_submit_response(
            identity.form_id,
            protocol::ModalFormResponseSelection::ButtonIndex(1),
        )]
    );
}

#[test]
fn indexed_scoreboard_pack_survives_apply_switch_remove_at_two_gui_scales() {
    for scale in [1, 2] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = session(&mut player);
        let mut presentation = presentation();
        presentation.set_gui_scale_preference(Some(scale));
        for (stage, color) in [None, Some(GREEN), Some(BLUE), None, Some(GREEN)]
            .into_iter()
            .enumerate()
        {
            runtime.set_server_ui(color.map(|color| Arc::new(pack(color == BLUE))));
            let protocol::WorldEvent::Ui(event) = protocol::into_world_event(
                protocol::test_support::packets::modal_form_request_packet(
                    10 + stage as u32,
                    r#"{"type":"form","title":"Round menu","content":"","buttons":[{"text":"Stay"},{"text":"Continue"}]}"#,
                ),
                0,
            )
            .unwrap()
            .expect("decoded form event") else {
                panic!("form request must decode as UI input");
            };
            apply(&mut player, &mut runtime, 10 + stage as u64, event);
            let input = render(&mut presentation, &player, &runtime);
            let labels: Vec<_> = presentation
                .hud_draw_nodes()
                .iter()
                .filter_map(|node| match &node.draw {
                    Draw::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            for expected in ["Standings", "Amber", "Birch", "Next round"] {
                assert!(
                    labels.contains(&expected),
                    "stage {stage}, scale {scale}: {labels:?}"
                );
            }
            for score in ["791", "263"] {
                assert_eq!(
                    labels.contains(&score),
                    color.is_none(),
                    "score column at stage {stage}, scale {scale}: {labels:?}"
                );
            }
            let form_text = pack_harness::drawn_texts(
                &presentation
                    .last_frame
                    .as_ref()
                    .expect("published frame")
                    .nodes,
            );
            for expected in ["Round menu", "Stay", "Continue"] {
                assert!(
                    form_text.iter().any(|text| text == expected),
                    "{form_text:?}"
                );
            }
            let pixels = snapshot::rasterize(&input);
            // The replacement owns this rectangle; an ordinary patch must retain its
            // inherited size, placement, children and bindings while changing its art.
            if stage > 0 {
                region(&pixels, scale, [10, 10], color.unwrap_or(BACKDROP));
                region(&pixels, scale, [140, 80], color.unwrap_or(BACKDROP));
            }
            if color.is_some() {
                region(&pixels, scale, [150, 10], BACKDROP);
            }
            submit(&presentation, &mut runtime);
        }
    }
}
