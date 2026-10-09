//! Combat-frame publication work: a HUD change redraws only the nodes it changed, and turning
//! with the paper doll shown rebuilds no texture page, while every published frame equals the
//! one a full build of the same frame makes.

use super::*;

fn survival(player_runtime: &mut player_state::PlayerState, health: u16) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    player_runtime.inventory.set_local_selected_slot(0);
    runtime.hud.set_stats(
        BoundedStat::new(health, 20),
        BoundedStat::new(20, 20),
        BoundedStat::new(0, 20),
        None,
    );
    runtime
}

fn build(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) -> UiRenderInput {
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

/// A presentation stepped in lockstep with one that builds every frame in full.
struct Twin {
    redrawn: UiPresentationRuntime,
    full: UiPresentationRuntime,
}

impl Twin {
    /// Builds both and asserts they publish the same frame; returns the redraw's nodes emitted
    /// and whether it built in full.
    fn step(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        now: u64,
    ) -> (usize, bool) {
        let (redrawn, builds) = (self.redrawn.redrawn_nodes, self.redrawn.tree_builds);
        let drawn = build(player_runtime, &mut self.redrawn, runtime, now);
        self.full.last_frame = None;
        let expected = build(player_runtime, &mut self.full, runtime, now);
        assert_eq!(drawn.vertices, expected.vertices, "frame at {now}");
        assert_eq!(drawn.indices, expected.indices, "frame at {now}");
        assert_eq!(drawn.batches, expected.batches, "frame at {now}");
        (
            self.redrawn.redrawn_nodes - redrawn,
            self.redrawn.tree_builds != builds,
        )
    }
}

/// Health as the server's attribute update sets it.
fn set_health(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    health: f32,
    sequence: u64,
) {
    let attribute = |name: &str, current: f32| protocol::ActorAttribute {
        name: Arc::from(name),
        min: 0.0,
        max: 20.0,
        current,
        default: None,
        modifiers: Arc::from([]),
    };
    runtime
        .apply_local_attributes(
            player_runtime,
            crate::ui_runtime::SequencedLocalAttributes {
                session_id: 1,
                fifo_sequence: sequence,
                local_millis: sequence * 10,
                server_tick: sequence,
                attributes: vec![
                    attribute("minecraft:health", health),
                    attribute("minecraft:player.hunger", 20.0),
                ]
                .into(),
            },
        )
        .unwrap();
}

/// Hits that blink the hearts change the HUD's shape and build in full; every frame publishes
/// what a full build would.
#[test]
fn health_changes_publish_what_a_full_build_does() {
    let (Some(redrawn), Some(full)) = (
        crate::test_support::engine_presentation(),
        crate::test_support::engine_presentation(),
    ) else {
        eprintln!(
            "skipping health_changes_publish_what_a_full_build_does: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut twin = Twin { redrawn, full };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = survival(&mut player, 20);
    set_health(&mut player, &mut runtime, 20.0, 1);
    for now in 0..3 {
        twin.step(&player, &runtime, now);
    }
    for (index, health) in [19.0, 17.0, 12.0, 12.0, 5.0, 20.0].into_iter().enumerate() {
        let sequence = 2 + index as u64;
        set_health(&mut player, &mut runtime, health, sequence);
        for frame in 0..3 {
            twin.step(&player, &runtime, 100 * sequence + frame * 16);
        }
    }
}

/// The paper doll turns with the player as geometry; its hand rasters follow only while shown.
#[test]
fn turning_with_the_paper_doll_shown_rebuilds_no_texture_page() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.gui_models.enabled = true;
    let pose =
        |yaw: f32, pitch: f32| player_preview::PlayerPreviewPose::new(yaw, yaw, pitch, false);
    presentation.sync_player_preview(None, pose(0.0, 0.0), true, false, 0.0);
    let textures = Arc::clone(&presentation.textures);
    for step in 1..20 {
        let turned = pose(step as f32 * 7.0, step as f32 * 2.0);
        presentation.sync_player_preview(None, turned, true, false, f64::from(step) * 0.01);
        assert!(
            Arc::ptr_eq(&textures, &presentation.textures),
            "turn {step} rebuilt a texture page"
        );
        assert_eq!(presentation.player_preview_pose, Some(turned));
    }
    // Shown hands follow the pitch they draw.
    let looked = pose(40.0, 30.0);
    presentation.sync_player_preview(None, looked, true, true, 1.0);
    assert!(!Arc::ptr_eq(&textures, &presentation.textures));
    let skin = render_model::default_actor_skin_rgba8();
    let rasters = presentation.player_preview_pixels.as_ref().unwrap();
    assert_eq!(
        rasters.right_hand,
        player_preview::render_hand(skin.as_ref(), looked, false)
    );
    assert_eq!(
        rasters.left_hand,
        player_preview::render_hand(skin.as_ref(), looked, true)
    );
}

/// A swing's action-bar update re-emits only the action bar's nodes through the engine HUD.
#[test]
fn an_action_bar_update_redraws_only_its_nodes() {
    let (Some(redrawn), Some(full)) = (
        crate::test_support::engine_presentation(),
        crate::test_support::engine_presentation(),
    ) else {
        eprintln!(
            "skipping an_action_bar_update_redraws_only_its_nodes: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut twin = Twin { redrawn, full };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = survival(&mut player, 20);
    runtime.hud.set_actionbar(Arc::from("CPS: 5"), 1, 0);
    for now in 0..3 {
        twin.step(&player, &runtime, now);
    }
    for (index, cps) in (6..12).enumerate() {
        let now = 10 + index as u64;
        runtime
            .hud
            .set_actionbar(Arc::from(format!("CPS: {cps}")), 2 + index as u64, now);
        let (emitted, rebuilt) = twin.step(&player, &runtime, now);
        assert!(!rebuilt, "the HUD keeps its shape");
        assert!(
            (1..=4).contains(&emitted),
            "CPS {cps} redrew {emitted} nodes"
        );
    }
}
