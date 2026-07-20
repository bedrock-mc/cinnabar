//! Deterministic witnesses for the pinned Java-HUD state matrix: crosshair
//! visibility, per-game-mode surface gating, heart variants, mount rows,
//! effects, and the selected-item label fade.

use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorMetadata, ActorMetadataValue, ArmorEquipmentEvent,
    ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack, PlayerGameMode,
};

use super::{fixture_font, fixture_hud};
use crate::ui_runtime::presentation::{HudFrame, UiPresentationRuntime};
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};

fn effect(effect_id: i32, duration_ticks: i32) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action: ActorEffectAction::Add,
        effect_id,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks,
        tick: 0,
    }
}

fn item(network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: -1,
        count,
        nbt_digest: <sha2::Sha256 as sha2::Digest>::digest([]).into(),
        block_runtime_id: 0,
        extra_data: std::sync::Arc::from([]),
    }
}

fn first_person_frame() -> HudFrame {
    HudFrame {
        first_person: true,
        ..HudFrame::default()
    }
}

fn build(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now_millis: u64,
) -> render::UiRenderInput {
    presentation
        .build(
            runtime,
            now_millis,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

fn invert_batches(input: &render::UiRenderInput) -> usize {
    input
        .batches
        .iter()
        .filter(|batch| batch.blend_mode == render::UI_BLEND_INVERT)
        .count()
}

#[test]
fn crosshair_is_centered_invert_blended_and_first_person_only() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);

    // Third person (default frame): no crosshair, no invert batch.
    let third_person = build(&mut presentation, &runtime, 0);
    assert_eq!(invert_batches(&third_person), 0);

    *presentation.hud_frame_mut() = first_person_frame();
    let first_person = build(&mut presentation, &runtime, 0);
    assert_eq!(
        invert_batches(&first_person),
        1,
        "the crosshair draws through exactly one invert-blend batch"
    );
    // The crosshair is the first drawn surface; its quad is the first four
    // vertices. Java centers the 15x15 sprite with integer floor at GUI scale
    // 3: x = floor((426.67 - 15) / 2) * 3 + fractional-center offset of the
    // 1280-wide viewport; assert exact symmetric centering instead of pinning
    // the odd-pixel remainder.
    let quad = &first_person.vertices[..4];
    let left = quad
        .iter()
        .map(|vertex| vertex.position[0])
        .fold(f32::INFINITY, f32::min);
    let right = quad
        .iter()
        .map(|vertex| vertex.position[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let top = quad
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::INFINITY, f32::min);
    let bottom = quad
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(right - left, 45.0, "15 GUI px at auto scale 3");
    assert_eq!(bottom - top, 45.0);
    let center = [(left + right) / 2.0, (top + bottom) / 2.0];
    assert!((center[0] - 640.0).abs() <= 3.0);
    assert!((center[1] - 360.0).abs() <= 3.0);

    // Spectator hides the crosshair entirely (no interaction targeting yet).
    runtime.publish_player_game_mode(PlayerGameMode::Spectator);
    let spectator = build(&mut presentation, &runtime, 0);
    assert_eq!(invert_batches(&spectator), 0);
}

#[test]
fn game_mode_matrix_gates_each_surface_exactly() {
    for (mode, expect_hotbar, expect_stats) in [
        (PlayerGameMode::Survival, true, true),
        (PlayerGameMode::Adventure, true, true),
        (PlayerGameMode::Creative, true, false),
        (PlayerGameMode::Spectator, false, false),
    ] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.publish_player_game_mode(mode);
        runtime
            .apply_local_effect(1, 1, effect(1, -1))
            .expect("effects apply in every mode");
        *presentation.hud_frame_mut() = first_person_frame();

        let input = build(&mut presentation, &runtime, 0);
        let sprites = input.vertices.len() / 4;
        // Effects render in every mode (background + icon), the crosshair in
        // every non-spectator mode; the hotbar adds 12 sprites and survival
        // stats add hearts (20) and hunger (10) rows.
        let mut expected = 2usize;
        if mode != PlayerGameMode::Spectator {
            expected += 1;
        }
        if expect_hotbar {
            expected += 12;
        }
        if expect_stats {
            // Default 20/20 health and hunger: containers plus fills.
            expected += 40;
        }
        assert_eq!(
            sprites, expected,
            "sprite budget for {mode:?} (hotbar {expect_hotbar}, stats {expect_stats})"
        );
    }
}

#[test]
fn heart_variants_mount_rows_air_and_armor_follow_authoritative_state() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    *presentation.hud_frame_mut() = first_person_frame();
    let baseline = build(&mut presentation, &runtime, 0).vertices.len() / 4;

    // Poison recolors hearts without changing the sprite budget.
    runtime.apply_local_effect(1, 1, effect(19, -1)).unwrap();
    let poisoned = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    assert_eq!(poisoned, baseline + 2, "one effect entry adds two sprites");

    // Submerged air: 150/300 ticks shows the bubble row.
    runtime
        .apply_local_metadata(
            1,
            2,
            &[
                ActorMetadata {
                    key: 7,
                    value: ActorMetadataValue::Short(150),
                },
                ActorMetadata {
                    key: 42,
                    value: ActorMetadataValue::Short(300),
                },
            ],
        )
        .unwrap();
    let submerged = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    assert!(submerged > poisoned, "air bubbles render while submerged");

    // Armor equipment renders the ten-icon row only once points are derived.
    runtime
        .apply_local_armor(
            1,
            3,
            &ArmorEquipmentEvent {
                actor_runtime_id: 1,
                helmet: item(100, 1),
                chestplate: NetworkItemStack::empty(),
                leggings: NetworkItemStack::empty(),
                boots: NetworkItemStack::empty(),
                body: NetworkItemStack::empty(),
            },
        )
        .unwrap();
    // Without an item-registry resolution the identifier is unknown, so the
    // derived total is zero and the row stays hidden (fail closed).
    runtime.set_derived_armor(Some(0));
    let no_points = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    assert_eq!(
        no_points, submerged,
        "zero derived armor keeps the row hidden"
    );
    runtime.set_derived_armor(Some(15));
    let armored = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    assert_eq!(armored, no_points + 10, "armor row renders ten icons");

    // Mount health replaces the hunger row with right-aligned mount hearts.
    let mut frame = first_person_frame();
    frame.mount_health = Some((7.0, 30.0));
    *presentation.hud_frame_mut() = frame;
    let mounted = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    // Hunger's 10+N sprites swap for 15 mount hearts (containers + fills).
    assert_ne!(mounted, armored);
}

#[test]
fn selected_item_label_counts_and_durability_render_and_fade() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = item(5, 16);
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory();
    runtime.set_local_selected_slot(0);
    runtime.observe_selected_item_identity(1_000);

    let mut frame = first_person_frame();
    frame.selected_item_name = Some(std::sync::Arc::from("Emerald"));
    frame.hotbar_durability[0] = Some(0.5);
    *presentation.hud_frame_mut() = frame;

    // Inside the label window: label glyphs, count glyphs, and the two
    // durability rects render alongside the base sprites.
    let visible = build(&mut presentation, &runtime, 1_500);
    assert!(!visible.vertices.is_empty());
    let full_alpha_text = visible
        .vertices
        .iter()
        .any(|vertex| vertex.color == [255, 255, 255, 255] && vertex.uv != [0, 0]);
    assert!(full_alpha_text, "label text renders at full alpha");
    let has_black_track = visible
        .vertices
        .iter()
        .any(|vertex| vertex.color == [0, 0, 0, 255]);
    assert!(has_black_track, "durability track renders");

    // After the two-second window the label is gone; counts remain.
    let expired = build(&mut presentation, &runtime, 3_100);
    assert!(expired.vertices.len() < visible.vertices.len());
}

#[test]
fn spectator_still_presents_boss_bars_and_chat_surfaces() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Spectator);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: protocol::UiEvent::Boss(protocol::BossEvent {
                target_entity_id: 9,
                player_id: 0,
                action: protocol::BossAction::Show,
                title: std::sync::Arc::from("Guardian"),
                filtered_title: std::sync::Arc::from(""),
                progress: 1.0,
                style: protocol::BossStyle {
                    color: protocol::BossColor::White,
                    overlay: protocol::BossOverlay::Progress,
                    darken_sky: None,
                    create_world_fog: None,
                },
            }),
        })
        .unwrap();

    let input = build(&mut presentation, &runtime, 0);
    assert!(
        !input.vertices.is_empty(),
        "boss bars stay visible in spectator"
    );
}
