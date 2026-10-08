//! The OreUI bed screen: shown while asleep, Leave bed (and Open chat with
//! other players present) taking input once the screen settles.

use super::*;
use crate::ui_runtime::presentation::BedHit;

fn asleep(presentation: &mut UiPresentationRuntime, runtime: &mut UiRuntime) {
    runtime.set_local_sleeping(true);
    presentation.hud_frame_mut().sleep.observe(true, 1_000);
}

fn build(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) -> render_model::UiRenderInput {
    presentation
        .build(
            player_runtime,
            runtime,
            now,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn leave_bed_takes_input_once_the_screen_settles() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(presentation.bed_hits().is_empty(), "awake: no bed screen");
    asleep(&mut presentation, &mut runtime);
    build(&player_runtime, &mut presentation, &runtime, 1_000 + 1_000);
    assert!(presentation.bed_hits().is_empty(), "not yet interactive");
    build(&player_runtime, &mut presentation, &runtime, 1_000 + 2_000);
    let hits: Vec<BedHit> = presentation
        .bed_hits()
        .iter()
        .map(|(hit, _)| *hit)
        .collect();
    assert_eq!(hits, [BedHit::LeaveBed], "alone: no chat button");
    let (_, bounds) = presentation.bed_hits()[0];
    let centre = UiPoint::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    )
    .unwrap();
    assert_eq!(presentation.hit_test_bed(centre), Some(BedHit::LeaveBed));
    // The block sits 17.2% above the bottom edge.
    assert!((bounds.max().y() - 720.0 * (1.0 - 0.172)).abs() < 1.0);
}

#[test]
fn other_players_add_the_open_chat_button() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Me"), Arc::from("Alex")]);
    asleep(&mut presentation, &mut runtime);
    build(&player_runtime, &mut presentation, &runtime, 1_000 + 2_000);
    let hits: Vec<BedHit> = presentation
        .bed_hits()
        .iter()
        .map(|(hit, _)| *hit)
        .collect();
    assert_eq!(hits, [BedHit::LeaveBed, BedHit::OpenChat]);
}

/// Local-only: writes `bed_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is set.
#[test]
fn bed_screen_snapshot() {
    let player_runtime = player_state::PlayerState::new(1);

    let font = super::super::forms::pack_harness::font();
    let mut presentation = UiPresentationRuntime::with_hud(font, fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Me"), Arc::from("Alex")]);
    asleep(&mut presentation, &mut runtime);
    let input = build(&player_runtime, &mut presentation, &runtime, 1_000 + 3_000);
    super::super::forms::snapshot::write(&input, "bed_screen");
}
