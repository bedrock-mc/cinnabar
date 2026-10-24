//! Animated HUD models retain surrounding controls and match a full publication.

use super::*;
use crate::ui_runtime::UiRuntime;
use render_model::{UiRenderInput, UiRenderScene, UiRenderStats, hats};
use std::sync::Arc;

/// An original two-bone player with an animated attachment and the installed HUD layout.
fn fixture() -> Option<(UiPresentationRuntime, chunk_pipeline::WorldStream)> {
    let mut presentation = crate::test_support::engine_presentation()?;
    let catalog = tests::player_catalog_geometry(br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},
        "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]}]}"#.to_vec());
    let stream = tests::player_stream(Arc::clone(&catalog), catalog);
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(None, Default::default());
    presentation.hud_frame.player_preview = presentation.player_preview_icon();
    presentation.hud_frame.paper_doll_visible = true;
    presentation.hud_frame.first_person = true;
    presentation.set_preview_cosmetics(&[hats::fixture::bunny(), hats::fixture::dragon_wings()]);
    presentation.menu_artwork_loader.wait();
    Some((presentation, stream))
}

/// Builds a fixed survival HUD while its equipped wing animation advances.
fn frame(
    presentation: &mut UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    seconds: f32,
) -> UiRenderInput {
    presentation.set_cosmetic_motion(&[(
        hats::fixture::dragon_wings(),
        hats::Motion {
            seconds,
            ..Default::default()
        },
    )]);
    presentation.capture_hud_player(Some(stream), false);
    presentation
        .build(
            player,
            runtime,
            10_000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn animated_hud_cosmetics_reuse_topology_and_match_full_frames() {
    let Some((mut presentation, stream)) = fixture() else {
        eprintln!("missing-fixture: installed UI carrier for animated HUD retention; make assets");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    frame(&mut presentation, &stream, &player, &runtime, 0.0);
    let mut previous = frame(&mut presentation, &stream, &player, &runtime, 0.1);
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    scene.publish(previous.clone(), &stats).unwrap();
    for seconds in [0.2, 0.4, 0.7] {
        let next = frame(&mut presentation, &stream, &player, &runtime, seconds);
        assert_ne!(
            previous.vertices, next.vertices,
            "the equipped wings must keep moving"
        );
        assert!(
            Arc::ptr_eq(&previous.indices, &next.indices),
            "animation rebuilt HUD topology"
        );
        assert!(Arc::ptr_eq(&previous.batches, &next.batches));
        scene.publish(next.clone(), &stats).unwrap();
        presentation.last_frame = None;
        let full = frame(&mut presentation, &stream, &player, &runtime, seconds);
        assert_eq!(next.vertices, full.vertices);
        assert_eq!(next.indices, full.indices);
        assert_eq!(next.batches, full.batches);
        previous = full;
    }
}

#[test]
fn hud_cosmetic_publication_cost() {
    if std::env::var_os("CINNABAR_HUD_MOTION_BENCH").is_none() {
        eprintln!("missing-fixture: opt-in HUD motion benchmark; set CINNABAR_HUD_MOTION_BENCH=1");
        return;
    }
    let Some((mut presentation, stream)) = fixture() else {
        eprintln!("missing-fixture: installed UI carrier for HUD motion benchmark; make assets");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    let mut samples = Vec::new();
    for step in 0..700 {
        let started = std::time::Instant::now();
        let input = frame(
            &mut presentation,
            &stream,
            &player,
            &runtime,
            step as f32 / 60.0,
        );
        scene.publish(input, &stats).unwrap();
        if step >= 200 {
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "HUD_MOTION median_ms={:.4} p99_ms={:.4} vertices={} builds={}",
        samples[samples.len() / 2],
        samples[samples.len() * 99 / 100],
        scene.input.as_ref().unwrap().vertices.len(),
        presentation.tree_builds
    );
}
