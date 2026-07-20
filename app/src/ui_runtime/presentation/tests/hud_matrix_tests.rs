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

/// Applies authoritative full 20/20 health and hunger; stats are never
/// fabricated from game modes, so tests provide them explicitly.
fn apply_full_stats(runtime: &mut UiRuntime, sequence: u64) {
    let attribute = |name: &str| protocol::ActorAttribute {
        name: std::sync::Arc::from(name),
        min: 0.0,
        max: 20.0,
        current: 20.0,
        default: None,
        modifiers: std::sync::Arc::from([]),
    };
    runtime
        .apply_local_attributes(crate::ui_runtime::SequencedLocalAttributes {
            session_id: 1,
            fifo_sequence: sequence,
            local_millis: sequence * 10,
            server_tick: sequence,
            attributes: vec![
                attribute("minecraft:health"),
                attribute("minecraft:player.hunger"),
            ]
            .into(),
        })
        .unwrap();
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

fn quad_bounds(quad: &[render::UiRenderVertex]) -> [f32; 4] {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for vertex in quad {
        bounds[0] = bounds[0].min(vertex.position[0]);
        bounds[1] = bounds[1].min(vertex.position[1]);
        bounds[2] = bounds[2].max(vertex.position[0]);
        bounds[3] = bounds[3].max(vertex.position[1]);
    }
    bounds
}

#[test]
fn crosshair_centers_exactly_on_the_framebuffer_across_scales_and_dpi() {
    // (physical, dpi, fixed preference, expected GUI scale k). Auto follows
    // the Java rule; fixed preferences clamp into the auto range. The quad
    // must center exactly at physical/2 on both axes, including viewports
    // that do not divide by k, and span exactly 15*k physical px.
    for (physical, dpi, preference, k) in [
        ([1280u32, 720u32], 1.0f32, None, 3.0f32),
        ([1920, 1080], 1.0, None, 4.0),
        ([2560, 1440], 1.0, None, 6.0),
        ([1366, 768], 1.0, None, 3.0),
        ([1280, 720], 1.5, None, 3.0),
        ([1280, 720], 1.0, Some(2), 2.0),
        ([2560, 1440], 2.0, Some(4), 4.0),
    ] {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        presentation.set_gui_scale_preference(preference);
        let mut runtime = UiRuntime::new(1);
        runtime.publish_player_game_mode(PlayerGameMode::Survival);
        *presentation.hud_frame_mut() = first_person_frame();
        let input = presentation
            .build(&runtime, 0, physical, ui::DpiScale::new(dpi).unwrap())
            .unwrap();
        assert_eq!(
            invert_batches(&input),
            1,
            "one invert batch at {physical:?}"
        );
        let [left, top, right, bottom] = quad_bounds(&input.vertices[..4]);
        assert_eq!(right - left, 15.0 * k, "width at {physical:?} scale {k}");
        assert_eq!(bottom - top, 15.0 * k, "height at {physical:?} scale {k}");
        assert_eq!(
            (left + right) / 2.0,
            physical[0] as f32 / 2.0,
            "exact horizontal center at {physical:?} dpi {dpi} scale {k}"
        );
        assert_eq!(
            (top + bottom) / 2.0,
            physical[1] as f32 / 2.0,
            "exact vertical center at {physical:?} dpi {dpi} scale {k}"
        );
    }
}

#[test]
fn crosshair_is_invert_blended_first_person_only_and_mode_gated() {
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

    // Focused chat keeps the crosshair, exactly like the reference.
    runtime.open_chat();
    let chatting = build(&mut presentation, &runtime, 0);
    assert_eq!(invert_batches(&chatting), 1);
    runtime.close_chat();

    // Spectator hides the crosshair entirely (no interaction targeting yet).
    runtime.publish_player_game_mode(PlayerGameMode::Spectator);
    let spectator = build(&mut presentation, &runtime, 0);
    assert_eq!(invert_batches(&spectator), 0);
}

#[test]
fn live_spectator_switch_drops_hotbar_and_crosshair_despite_retained_slot() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    runtime.set_local_selected_slot(2);
    *presentation.hud_frame_mut() = first_person_frame();

    let survival = build(&mut presentation, &runtime, 0);
    assert!(
        survival.vertices.len() / 4 >= 13,
        "hotbar and crosshair render"
    );

    // The authoritative mode change arrives mid-session while the local slot
    // prediction is still retained; visibility must follow the mode.
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: protocol::UiEvent::GameMode(protocol::GameModeEvent {
                update: protocol::GameModeUpdate::Explicit(PlayerGameMode::Spectator),
            }),
        })
        .unwrap();
    assert_eq!(runtime.selected_hotbar_slot(), Some(2), "slot retained");
    let spectator = build(&mut presentation, &runtime, 0);
    assert!(
        spectator.vertices.is_empty(),
        "no hotbar, crosshair, or stats"
    );
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
        apply_full_stats(&mut runtime, 1);
        runtime
            .apply_local_effect(1, 2, effect(1, -1), 0)
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
    apply_full_stats(&mut runtime, 1);
    *presentation.hud_frame_mut() = first_person_frame();
    let baseline = build(&mut presentation, &runtime, 0).vertices.len() / 4;

    // Poison recolors hearts without changing the sprite budget.
    runtime.apply_local_effect(1, 2, effect(19, -1), 0).unwrap();
    let poisoned = build(&mut presentation, &runtime, 0).vertices.len() / 4;
    assert_eq!(poisoned, baseline + 2, "one effect entry adds two sprites");

    // Submerged air: 150/300 ticks shows the bubble row.
    runtime
        .apply_local_metadata(
            1,
            3,
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
            4,
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

#[test]
fn the_renderable_effect_id_gate_matches_the_pinned_icon_table_exactly() {
    use crate::ui_runtime::gameplay_hud::is_renderable_effect_id;
    use crate::ui_runtime::presentation::hud_layout::effect_icon_role;
    for effect_id in -8..=64 {
        assert_eq!(
            is_renderable_effect_id(effect_id),
            effect_icon_role(effect_id).is_some(),
            "state gate and pinned icon table agree on effect id {effect_id}"
        );
    }
}

#[test]
fn mount_jump_bar_replaces_the_experience_row_while_riding() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    apply_full_stats(&mut runtime, 1);

    let baseline = build(&mut presentation, &runtime, 0);

    // Riding at zero charge draws the empty jump strip only.
    presentation.hud_frame_mut().mount_jump = Some(0.0);
    let empty_bar = build(&mut presentation, &runtime, 0);
    assert_eq!(empty_bar.vertices.len(), baseline.vertices.len() + 4);

    // A half charge adds the clipped progress strip on top.
    presentation.hud_frame_mut().mount_jump = Some(0.5);
    let charging = build(&mut presentation, &runtime, 0);
    assert_eq!(charging.vertices.len(), baseline.vertices.len() + 8);
}

#[test]
fn attack_indicator_draws_only_below_full_charge_in_first_person() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    *presentation.hud_frame_mut() = first_person_frame();

    // The production authority reports exactly full: nothing draws.
    presentation.hud_frame_mut().attack_indicator_charge = Some(1.0);
    let ready = build(&mut presentation, &runtime, 0);

    // A sub-full charge draws the background and fill bars below center.
    presentation.hud_frame_mut().attack_indicator_charge = Some(0.5);
    let charging = build(&mut presentation, &runtime, 0);
    assert_eq!(charging.vertices.len(), ready.vertices.len() + 8);

    // The indicator is crosshair-attached: never in third person.
    presentation.hud_frame_mut().first_person = false;
    let third_person = build(&mut presentation, &runtime, 0);
    assert!(third_person.vertices.len() < ready.vertices.len());
}

#[test]
fn notched_boss_overlays_draw_their_exact_dividers() {
    use protocol::{
        BossAction as ProtocolBossAction, BossColor as ProtocolBossColor, BossEvent,
        BossOverlay as ProtocolBossOverlay, BossStyle as ProtocolBossStyle, UiEvent,
    };
    let boss = |overlay| {
        UiEvent::Boss(BossEvent {
            target_entity_id: 9,
            player_id: 0,
            action: ProtocolBossAction::Show,
            title: std::sync::Arc::from(""),
            filtered_title: std::sync::Arc::from(""),
            progress: 1.0,
            style: ProtocolBossStyle {
                color: ProtocolBossColor::Red,
                overlay,
                darken_sky: None,
                create_world_fog: None,
            },
        })
    };
    let build_with = |overlay| {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(crate::ui_runtime::SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: boss(overlay),
            })
            .unwrap();
        build(&mut presentation, &runtime, 0).vertices.len()
    };
    let progress = build_with(ProtocolBossOverlay::Progress);
    // N-notch overlays add exactly N-1 divider quads over the bar.
    assert_eq!(build_with(ProtocolBossOverlay::Notched6), progress + 5 * 4);
    assert_eq!(build_with(ProtocolBossOverlay::Notched10), progress + 9 * 4);
    assert_eq!(
        build_with(ProtocolBossOverlay::Notched12),
        progress + 11 * 4
    );
    assert_eq!(
        build_with(ProtocolBossOverlay::Notched20),
        progress + 19 * 4
    );
}

#[test]
fn the_tab_overlay_lists_known_players_with_list_scores_while_held() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(
        |_| None,
        vec![std::sync::Arc::from("Alex"), std::sync::Arc::from("Steve")],
    );

    let closed = build(&mut presentation, &runtime, 0);
    assert!(closed.vertices.is_empty(), "no overlay while released");

    presentation.hud_frame_mut().tab_list_open = true;
    let open = build(&mut presentation, &runtime, 0);
    assert!(
        !open.vertices.is_empty(),
        "held player-list action presents the known players"
    );

    // Releasing the action removes the overlay again.
    presentation.hud_frame_mut().tab_list_open = false;
    let released = build(&mut presentation, &runtime, 0);
    assert!(released.vertices.is_empty());
}
