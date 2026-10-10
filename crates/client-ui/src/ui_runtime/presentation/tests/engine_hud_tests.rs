//! The gameplay HUD through the JSON-UI engine with the built-in Java pack:
//! surface gating, Java geometry, native renderer state, fades, and caching.
//! Needs the gitignored UI carrier; each test skips when it is absent.

use json_ui::{Draw, DrawNode};
use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorMetadata, ActorMetadataValue, ContainerIdentity,
    InventoryContentEvent, InventoryEvent, NetworkItemStack, PlayerGameMode,
};

use super::*;
use crate::ui_runtime::presentation::{HudFrame, hud_layout};

mod absorption;
mod boss_removal_tests;
mod crosshair_options;
mod effects;

pub use crate::test_support::{engine_presentation, engine_presentation_with};

fn texts(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn text<'a>(nodes: &'a [DrawNode], wanted: &str) -> Option<&'a DrawNode> {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
}

fn customs<'a>(nodes: &'a [DrawNode], renderer: &str) -> Vec<&'a DrawNode> {
    nodes
        .iter()
        .filter(
            |node| matches!(&node.draw, Draw::Custom { renderer: drawn, .. } if drawn == renderer),
        )
        .collect()
}

fn named<'a>(nodes: &'a [DrawNode], name: &str) -> Vec<&'a DrawNode> {
    nodes.iter().filter(|node| node.name == name).collect()
}

fn item(network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: -1,
        count,
        nbt_digest: <sha2::Sha256 as sha2::Digest>::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

fn effect(effect_id: i32) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action: ActorEffectAction::Add,
        effect_id,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks: -1,
        tick: 0,
    }
}

/// Authoritative full 20/20 health and hunger; stats are never fabricated.
fn full_stats(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    sequence: u64,
) {
    let attribute = |name: &str| protocol::ActorAttribute {
        name: Arc::from(name),
        min: 0.0,
        max: 20.0,
        current: 20.0,
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
                    attribute("minecraft:health"),
                    attribute("minecraft:player.hunger"),
                ]
                .into(),
            },
        )
        .unwrap();
}

fn first_person() -> HudFrame {
    HudFrame {
        first_person: true,
        ..HudFrame::default()
    }
}

fn build_at(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
    physical: [u32; 2],
    dpi: f32,
) -> render_model::UiRenderInput {
    presentation
        .build(
            player_runtime,
            runtime,
            now,
            physical,
            DpiScale::new(dpi).unwrap(),
        )
        .unwrap()
}

fn build(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) -> render_model::UiRenderInput {
    build_at(player_runtime, presentation, runtime, now, [1280, 720], 1.0)
}

/// The `[min_x, min_y, max_x, max_y]` bounds of each quad in the frame.
fn quad_bounds(input: &render_model::UiRenderInput) -> impl Iterator<Item = [f32; 4]> + '_ {
    input.vertices.as_chunks::<4>().0.iter().map(|quad| {
        quad.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |bounds, vertex| {
                [
                    bounds[0].min(vertex.position[0]),
                    bounds[1].min(vertex.position[1]),
                    bounds[2].max(vertex.position[0]),
                    bounds[3].max(vertex.position[1]),
                ]
            },
        )
    })
}

/// The crosshair: the one `side`-square quad in the frame, if any.
fn crosshair(input: &render_model::UiRenderInput, side: f32) -> Option<[f32; 4]> {
    quad_bounds(input).find(|bounds| {
        (bounds[2] - bounds[0] - side).abs() < 1e-3 && (bounds[3] - bounds[1] - side).abs() < 1e-3
    })
}

fn select_slot(player_runtime: &mut player_state::PlayerState, runtime: &mut UiRuntime) {
    runtime.retain_local_selected_equipment(
        player_runtime,
        7,
        protocol::EquipmentEvent {
            actor_runtime_id: 42,
            stack: NetworkItemStack::empty(),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: None,
        },
    );
}

// The crosshair spans 15 GUI px and centres exactly on the (safe) viewport.
#[test]
fn crosshair_centres_exactly_across_scales_dpi_and_insets() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for (physical, dpi, preference, k, safe) in [
        ([1280u32, 720u32], 1.0f32, None, 2.0f32, SafeArea::ZERO),
        ([1920, 1080], 1.0, None, 4.0, SafeArea::ZERO),
        ([1366, 768], 1.0, None, 3.0, SafeArea::ZERO),
        ([1280, 720], 1.5, None, 2.0, SafeArea::ZERO),
        ([1280, 720], 1.0, Some(2), 2.0, SafeArea::ZERO),
        (
            [1280, 720],
            1.0,
            None,
            2.0,
            SafeArea::new(20.0, 10.0, 40.0, 30.0).unwrap(),
        ),
        (
            [2560, 1440],
            2.0,
            Some(4),
            4.0,
            SafeArea::new(30.0, 20.0, 10.0, 0.0).unwrap(),
        ),
    ] {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping crosshair_centres_exactly_across_scales_dpi_and_insets: fixture unavailable; requires installed local carriers (make assets)"
            );
            return;
        };
        presentation.set_gui_scale_preference(preference);
        presentation.set_safe_area(safe);
        *presentation.hud_frame_mut() = first_person();
        let runtime = UiRuntime::new(1);
        player_runtime
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        let input = build_at(
            &player_runtime,
            &mut presentation,
            &runtime,
            0,
            physical,
            dpi,
        );
        let bounds =
            crosshair(&input, 15.0 * k).unwrap_or_else(|| panic!("crosshair at {physical:?}"));
        let logical = [physical[0] as f32 / dpi, physical[1] as f32 / dpi];
        let centre = [
            (safe.left() + (logical[0] - safe.left() - safe.right()) / 2.0) * dpi,
            (safe.top() + (logical[1] - safe.top() - safe.bottom()) / 2.0) * dpi,
        ];
        assert!(
            ((bounds[0] + bounds[2]) / 2.0 - centre[0]).abs() < 1e-3,
            "{physical:?}"
        );
        assert!(
            ((bounds[1] + bounds[3]) / 2.0 - centre[1]).abs() < 1e-3,
            "{physical:?}"
        );
    }
}

// First person only, kept while chatting, and gone in spectator.
#[test]
fn crosshair_is_first_person_only_and_mode_gated() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping crosshair_is_first_person_only_and_mode_gated: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let paint = |player_runtime: &player_state::PlayerState,
                 presentation: &UiPresentationRuntime,
                 runtime: &UiRuntime| {
        hud_layout::capture_hud_paint(
            player_runtime,
            runtime,
            presentation.hud_frame(),
            presentation.hud_textures.as_ref(),
            &Default::default(),
        )
    };
    assert!(
        paint(&player_runtime, &presentation, &runtime)
            .crosshair
            .is_none(),
        "third person"
    );
    *presentation.hud_frame_mut() = first_person();
    assert!(
        crosshair(
            &build(&player_runtime, &mut presentation, &runtime, 0),
            30.0
        )
        .is_some()
    );
    runtime.open_chat(&mut player_runtime);
    assert!(
        crosshair(
            &build(&player_runtime, &mut presentation, &runtime, 0),
            30.0
        )
        .is_some()
    );
    runtime.close_chat();
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Spectator);
    assert!(
        paint(&player_runtime, &presentation, &runtime)
            .crosshair
            .is_none(),
        "spectator"
    );
}

// Hotbar, status rows, and effects follow the authoritative game mode.
#[test]
fn game_mode_matrix_gates_each_surface_exactly() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for (mode, hotbar, stats) in [
        (PlayerGameMode::Survival, true, true),
        (PlayerGameMode::Adventure, true, true),
        (PlayerGameMode::Creative, true, false),
        (PlayerGameMode::Spectator, false, false),
    ] {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping game_mode_matrix_gates_each_surface_exactly: fixture unavailable; requires installed local carriers (make assets)"
            );
            return;
        };
        let mut runtime = UiRuntime::new(1);
        player_runtime.facts.publish_player_game_mode(mode);
        player_runtime.inventory.set_local_selected_slot(2);
        full_stats(&mut player_runtime, &mut runtime, 1);
        runtime.apply_local_effect(1, 2, effect(1), 0).unwrap();
        *presentation.hud_frame_mut() = first_person();
        build(&player_runtime, &mut presentation, &runtime, 0);
        let nodes = presentation.hud_draw_nodes();
        assert_eq!(
            customs(nodes, "hotbar_renderer").len(),
            if hotbar { 9 } else { 0 },
            "{mode:?}"
        );
        assert_eq!(
            !customs(nodes, "heart_renderer").is_empty(),
            stats,
            "{mode:?}"
        );
        assert_eq!(
            !customs(nodes, "hunger_renderer").is_empty(),
            stats,
            "{mode:?}"
        );
        assert_eq!(customs(nodes, "mob_effects_renderer").len(), 1, "{mode:?}");
        let paint = hud_layout::capture_hud_paint(
            &player_runtime,
            &runtime,
            presentation.hud_frame(),
            None,
            &Default::default(),
        );
        assert_eq!(paint.effects.icons.len(), 1, "{mode:?}");
        assert_eq!(paint.hearts.len(), if stats { 20 } else { 0 }, "{mode:?}");
    }
}

// A retained slot prediction never keeps the hotbar once the mode is spectator.
#[test]
fn live_spectator_switch_drops_the_hotbar_despite_a_retained_slot() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping live_spectator_switch_drops_the_hotbar_despite_a_retained_slot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player_runtime.inventory.set_local_selected_slot(2);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert_eq!(
        customs(presentation.hud_draw_nodes(), "hotbar_renderer").len(),
        9
    );
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::GameMode(protocol::GameModeEvent {
                    update: protocol::GameModeUpdate::Explicit(PlayerGameMode::Spectator),
                }),
            },
        )
        .unwrap();
    assert_eq!(
        player_runtime.selected_hotbar_slot(),
        Some(2),
        "slot retained"
    );
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(customs(presentation.hud_draw_nodes(), "hotbar_renderer").is_empty());
}

// Java Gui geometry: hotbar flush at the bottom centre, selection 1 px out,
// status rows from H-39, the XP bar at H-29 with its green level at H-35.
#[test]
fn java_pack_geometry_on_a_real_viewport() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping java_pack_geometry_on_a_real_viewport: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(&mut player_runtime, &mut runtime);
    full_stats(&mut player_runtime, &mut runtime, 1);
    runtime.hud.set_experience(7, 0.4);
    let input = build_at(
        &player_runtime,
        &mut presentation,
        &runtime,
        0,
        [1280, 750],
        1.5,
    );
    let nodes = presentation.hud_draw_nodes();
    // 1280x750 at scale 3: a 426.67x250 GUI-px screen.
    let centre = 1280.0 / 3.0 / 2.0;
    let slots = customs(nodes, "hotbar_renderer");
    assert!(
        (slots[0].dest.x - (centre - 90.0)).abs() < 1e-3,
        "{:?} vs centre {centre}",
        slots[0].dest
    );
    assert_eq!(slots[0].dest.y, 250.0 - 22.0);
    let selected = named(nodes, "hotbar_slot_selected_image");
    assert!((selected[0].dest.x - (centre - 92.0)).abs() < 1e-3);
    assert_eq!(selected[0].dest.y, 250.0 - 23.0);
    let hearts = customs(nodes, "heart_renderer");
    assert!((hearts[0].dest.x - (centre - 91.0)).abs() < 1e-3);
    assert_eq!(hearts[0].dest.y, 250.0 - 39.0);
    let hunger = customs(nodes, "hunger_renderer");
    assert!((hunger[0].dest.x - (centre + 90.0)).abs() < 1e-3);
    let bar = named(nodes, "empty_progress_bar")
        .into_iter()
        .map(|node| node.dest.y)
        .fold(f64::INFINITY, f64::min);
    assert_eq!(bar, 250.0 - 29.0);
    let level: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(&node.draw, Draw::Text { text, .. } if text == "7"))
        .collect();
    assert_eq!(level.len(), 5, "the level and its four outline copies");
    assert_eq!(level[4].dest.y, 250.0 - 35.0);
    // The selection frame lands where the Java HUD drew it, in physical px.
    assert!(
        quad_bounds(&input).any(|bounds| bounds == [364.0, 681.0, 436.0, 753.0]),
        "selection frame at Java geometry"
    );
}

// The hotbar keeps 182 GUI px at every auto scale, centred.
#[test]
fn hotbar_stays_bottom_centred_and_tracks_the_auto_scale() {
    let mut player_runtime = player_state::PlayerState::new(1);

    for (physical, scale) in [([1280u32, 720u32], 2.0f64), ([2560, 1344], 5.0)] {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping hotbar_stays_bottom_centred_and_tracks_the_auto_scale: fixture unavailable; requires installed local carriers (make assets)"
            );
            return;
        };
        let mut runtime = UiRuntime::new(1);
        select_slot(&mut player_runtime, &mut runtime);
        build_at(
            &player_runtime,
            &mut presentation,
            &runtime,
            0,
            physical,
            1.5,
        );
        let nodes = presentation.hud_draw_nodes();
        let caps = named(nodes, "start_cap_image");
        let ends = named(nodes, "end_cap_image");
        let left = caps
            .iter()
            .map(|node| node.dest.x)
            .fold(f64::INFINITY, f64::min);
        let right = ends
            .iter()
            .map(|node| node.dest.x + node.dest.w)
            .fold(f64::NEG_INFINITY, f64::max);
        let width = f64::from(physical[0]) / scale;
        assert!((right - left - 182.0).abs() < 1e-3);
        assert!(((left + right) / 2.0 - width / 2.0).abs() < 1e-3);
    }
}

// Poison recolours hearts, air shows bubbles, armor shows ten icons once
// points are derived, and mount hearts replace hunger.
#[test]
fn heart_variants_mount_rows_air_and_armor_follow_authoritative_state() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    full_stats(&mut player_runtime, &mut runtime, 1);
    let frame = first_person();
    let paint = |runtime: &UiRuntime, frame: &HudFrame| {
        hud_layout::capture_hud_paint(&player_runtime, runtime, frame, None, &Default::default())
    };
    let baseline = paint(&runtime, &frame);
    assert!(
        baseline
            .hearts
            .iter()
            .any(|cell| cell.texture == "textures/ui/heart")
    );
    runtime.apply_local_effect(1, 2, effect(19), 0).unwrap();
    let poisoned = paint(&runtime, &frame);
    assert!(
        poisoned
            .hearts
            .iter()
            .any(|cell| cell.texture == "textures/ui/poison_heart")
    );
    assert_eq!(poisoned.hearts.len(), baseline.hearts.len());
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
    assert!(
        !paint(&runtime, &frame).bubbles.is_empty(),
        "bubbles while submerged"
    );
    runtime.set_derived_armor(Some(0));
    assert!(
        paint(&runtime, &frame).armor.is_empty(),
        "zero armor stays hidden"
    );
    runtime.set_derived_armor(Some(15));
    let armored = paint(&runtime, &frame);
    assert_eq!(armored.armor.len(), 10);
    // One row of hearts: armor sits one row (10 px) above it.
    assert!(armored.armor.iter().all(|cell| cell.at[1] == -10.0));
    let mut mounted = frame.clone();
    mounted.mount_health = Some((7.0, 30.0));
    let riding = paint(&runtime, &mounted);
    assert!(riding.hunger.is_empty());
    // Fifteen containers plus four hearts (7 half-hearts: three full, one half).
    assert_eq!(riding.mount_hearts.len(), 15 + 4);
}

// 30/30 half-hearts stack fifteen hearts over two rows 10 px apart.
#[test]
fn nonstandard_health_maximum_renders_stacked_rows_like_the_reference() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.hud.set_health(BoundedStat::new(30, 30));
    let paint = hud_layout::capture_hud_paint(
        &player_runtime,
        &runtime,
        &HudFrame::default(),
        None,
        &Default::default(),
    );
    let rows: std::collections::BTreeSet<i64> =
        paint.hearts.iter().map(|cell| cell.at[1] as i64).collect();
    assert_eq!(paint.hearts.len(), 30);
    assert_eq!(rows.into_iter().collect::<Vec<_>>(), [-10, 0]);
}

// The selected-item name, stack counts, and durability draw; the name leaves
// after its two-second window.
#[test]
fn selected_item_label_counts_and_durability_render_and_fade() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping selected_item_label_counts_and_durability_render_and_fade: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = item(5, 16);
    runtime
        .enqueue_inventory_event(
            &mut player_runtime,
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
    runtime.drain_pending_inventory(&mut player_runtime);
    player_runtime.inventory.set_local_selected_slot(0);
    runtime.observe_selected_item_identity(&player_runtime, 1_000);
    let mut frame = first_person();
    frame.selected_item_name = Some(Arc::from("Emerald"));
    frame.hotbar_stacks[0] = Some(item(5, 16));
    frame.hotbar_durability[0] = Some(0.5);
    *presentation.hud_frame_mut() = frame;

    build(&player_runtime, &mut presentation, &runtime, 1_500);
    let nodes = presentation.hud_draw_nodes();
    assert!(texts(nodes).contains(&"Emerald"));
    assert!(texts(nodes).contains(&"16"));
    let durability = customs(nodes, "progress_bar_renderer");
    assert!(durability.iter().any(|node| matches!(
        &node.draw,
        Draw::Custom { data, .. } if data.get("#touch_progress_bar_visible") == Some(&serde_json::Value::Bool(true))
    )));
    // Java: the name stands 1.5 s, then fades out over 0.5 s.
    let name = text(nodes, "Emerald").unwrap();
    assert_eq!(presentation.hud_fade(name, 1.5), 1.0);
    assert!(presentation.hud_fade(name, 2.75) < 1.0);

    build(&player_runtime, &mut presentation, &runtime, 3_100);
    let nodes = presentation.hud_draw_nodes();
    assert!(!texts(nodes).contains(&"Emerald"));
    assert!(texts(nodes).contains(&"16"));
}

// Boss bars and chat stay visible in spectator.
#[test]
fn spectator_still_presents_boss_bars_and_chat() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping spectator_still_presents_boss_bars_and_chat: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Spectator);
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::Show,
                    9,
                    "Guardian",
                    1.0,
                    ProtocolBossColor::White,
                    ProtocolBossOverlay::Progress,
                ),
            },
        )
        .unwrap();
    runtime
        .apply(&mut player_runtime, chat_line(2, "hello"))
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    assert!(texts(nodes).contains(&"Guardian"));
    assert!(texts(nodes).contains(&"hello"));
}

// Riding hides the XP bar for the jump bar, drawn from the HUD sheet.
#[test]
fn mount_jump_bar_replaces_the_experience_row_while_riding() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping mount_jump_bar_replaces_the_experience_row_while_riding: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    full_stats(&mut player_runtime, &mut runtime, 1);
    runtime.hud.set_experience(3, 0.5);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!named(presentation.hud_draw_nodes(), "full_progress_bar").is_empty());
    presentation.hud_frame_mut().mount_jump = Some(0.5);
    let base = build(&player_runtime, &mut presentation, &runtime, 0)
        .vertices
        .len();
    assert!(named(presentation.hud_draw_nodes(), "full_progress_bar").is_empty());
    let paint = hud_layout::capture_hud_paint(
        &player_runtime,
        &runtime,
        presentation.hud_frame(),
        presentation.hud_textures.as_ref(),
        &Default::default(),
    );
    let (_, _, filled) = paint.mount_jump.expect("jump bar");
    assert_eq!(filled, 91.0);
    presentation.hud_frame_mut().mount_jump = Some(0.0);
    assert_eq!(
        build(&player_runtime, &mut presentation, &runtime, 0)
            .vertices
            .len(),
        base - 4
    );
}

// Boss bars stack, tinted by their colour; an emptied bar draws no fill.
#[test]
fn boss_bars_render_titled_tinted_tracks_and_updates() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping boss_bars_render_titled_tinted_tracks_and_updates: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    let show = |sequence, id, title, progress, color| SequencedUiEvent {
        session_id: 1,
        fifo_sequence: sequence,
        local_millis: 0,
        server_tick: None,
        event: boss_event(
            ProtocolBossAction::Show,
            id,
            title,
            progress,
            color,
            ProtocolBossOverlay::Progress,
        ),
    };
    runtime
        .apply(
            &mut player_runtime,
            show(1, 7, "Boss", 0.5, ProtocolBossColor::Purple),
        )
        .unwrap();
    runtime
        .apply(
            &mut player_runtime,
            show(2, 8, "Other", 1.0, ProtocolBossColor::Red),
        )
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    let fills = named(nodes, "filled_progress_bar_for_collections");
    let tint = |color: [u8; 4]| {
        fills
            .iter()
            .filter(
                |node| matches!(&node.draw, Draw::Sprite { color: drawn, .. } if *drawn == color),
            )
            .count()
    };
    assert!(tint([170, 0, 170, 255]) > 0, "purple fill");
    assert!(tint([255, 85, 85, 255]) > 0, "red fill");
    let notches = customs(nodes, "java_boss_notches");
    assert_eq!(notches.len(), 2, "one overlay per bar");
    let names: Vec<f64> = ["Boss", "Other"]
        .iter()
        .map(|name| text(nodes, name).unwrap().dest.y)
        .collect();
    assert_eq!(names, [3.0, 22.0], "Java stacks bars 19 GUI px apart");
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 3,
                local_millis: 0,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::SetProgress,
                    7,
                    "",
                    0.0,
                    ProtocolBossColor::Purple,
                    ProtocolBossOverlay::Progress,
                ),
            },
        )
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    assert!(
        !named(nodes, "filled_progress_bar_for_collections")
            .iter()
            .any(|node| matches!(&node.draw, Draw::Sprite { color, .. } if *color == [170, 0, 170, 255]))
    );
}

// Sidebar rows resolve player, entity, and fake owners, with red Java scores.
#[test]
fn sidebar_resolves_owned_actor_names_and_draws_java_scores() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping sidebar_resolves_owned_actor_names_and_draws_java_scores: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    super::retained_hud_tests::install_mixed_scoreboard_slot(
        &mut player_runtime,
        &mut runtime,
        "sidebar",
        &[
            (3, ProtocolScoreIdentity::Player(17), 3),
            (4, ProtocolScoreIdentity::Entity(23), 2),
            (5, ProtocolScoreIdentity::FakePlayer(Arc::from("Server")), 1),
        ],
    );
    presentation
        .set_scoreboard_owner_names([(17, Arc::from("Alex")), (23, Arc::from("Beeatrice"))]);
    build(&player_runtime, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    for name in ["Objective", "Alex", "Beeatrice", "Server"] {
        assert!(texts(nodes).contains(&name), "{name}");
    }
    for score in ["3", "2", "1"] {
        let node = text(nodes, score).unwrap();
        assert!(matches!(
            node.draw,
            Draw::Text {
                color: [255, 85, 85, 255],
                ..
            }
        ));
    }
    // Rows 9 GUI px apart, box right edge 1 px in from the screen edge.
    let alex = text(nodes, "Alex").unwrap().dest.y;
    let beeatrice = text(nodes, "Beeatrice").unwrap().dest.y;
    assert!((beeatrice - alex - 9.0).abs() < 1e-3);
}

// Titles fade in, hold, and out on the server's timing; chat fades after 10 s.
#[test]
fn titles_and_chat_carry_their_fades() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping titles_and_chat_carry_their_fades: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime
        .hud
        .set_durations(ui::TitleDurations::from_wire(10, 70, 20).unwrap());
    runtime.hud.set_title(Arc::from("Victory"), 1, 1_000);
    runtime
        .apply(&mut player_runtime, chat_line(2, "gg"))
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 1_200);
    let nodes = presentation.hud_draw_nodes();
    let title = text(nodes, "Victory").unwrap();
    // 0.5 s fade in from its start at 1 s, 3.5 s hold, 1 s out.
    assert!((presentation.hud_fade(title, 1.25) - 0.5).abs() < 1e-3);
    assert_eq!(presentation.hud_fade(title, 3.0), 1.0);
    assert!((presentation.hud_fade(title, 5.5) - 0.5).abs() < 1e-3);
    let chat = text(nodes, "gg").unwrap();
    assert_eq!(presentation.hud_fade(chat, 9.0), 1.0);
    assert!(presentation.hud_fade(chat, 10.5) < 1.0);
    assert_eq!(presentation.hud_fade(chat, 11.0), 0.0);
    // A re-sent title starts a fresh title control while preserving the chat fade.
    let passes = presentation.hud_passes();
    runtime.hud.set_title(Arc::from("Victory"), 3, 2_000);
    build(&player_runtime, &mut presentation, &runtime, 2_100);
    assert_eq!(presentation.hud_passes(), passes + 1);
    let title = text(presentation.hud_draw_nodes(), "Victory").unwrap();
    assert!((presentation.hud_fade(title, 2.25) - 0.5).abs() < 1e-3);
    let chat = text(presentation.hud_draw_nodes(), "gg").unwrap();
    assert_eq!(presentation.hud_fade(chat, 11.0), 0.0);
}

// A steady HUD repaints from cache; a changed binding re-binds once.
#[test]
fn unchanged_hud_reuses_its_layout_across_frames() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping unchanged_hud_reuses_its_layout_across_frames: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(&mut player_runtime, &mut runtime);
    full_stats(&mut player_runtime, &mut runtime, 1);
    build(&player_runtime, &mut presentation, &runtime, 0);
    let first = presentation.hud_passes();
    for now in [16, 33, 50, 66] {
        build(&player_runtime, &mut presentation, &runtime, now);
    }
    assert_eq!(
        presentation.hud_passes(),
        first,
        "no re-bind while nothing changed"
    );
    runtime.hud.set_experience(4, 0.1);
    build(&player_runtime, &mut presentation, &runtime, 80);
    assert_eq!(presentation.hud_passes(), first + 1);
}

// The position line follows the world rule or a held map; days follow theirs.
#[test]
fn world_rules_raise_the_position_and_days_lines() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping world_rules_raise_the_position_and_days_lines: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    presentation.hud_frame_mut().player_block = Some([12, 64, -7]);
    presentation.hud_frame_mut().world_time = Some(24_000.0 * 3.0 + 5.0);
    build(&player_runtime, &mut presentation, &runtime, 0);
    let shown = |presentation: &UiPresentationRuntime, wanted: &str| {
        text(presentation.hud_draw_nodes(), wanted).is_some()
    };
    assert!(
        !shown(&presentation, "Position: 12, 64, -7"),
        "off by default"
    );
    runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(true),
        show_days_played: Some(true),
    });
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(shown(&presentation, "Position: 12, 64, -7"));
    assert!(shown(&presentation, "Days played: 3"));
    runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(false),
        show_days_played: None,
    });
    presentation.hud_frame_mut().holding_filled_map = true;
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(
        shown(&presentation, "Position: 12, 64, -7"),
        "a held map shows it"
    );
    assert!(
        shown(&presentation, "Days played: 3"),
        "an absent rule keeps its value"
    );
    if let Some(mut real) = engine_presentation_with(super::super::forms::pack_harness::font()) {
        *real.hud_frame_mut() = presentation.hud_frame().clone();
        let input = build(&player_runtime, &mut real, &runtime, 0);
        super::super::forms::snapshot::write(&input, "hud_coordinates");
    }
}

fn chat_line(sequence: u64, message: &str) -> SequencedUiEvent {
    SequencedUiEvent {
        session_id: 1,
        fifo_sequence: sequence,
        local_millis: 0,
        server_tick: None,
        event: chat_event(message),
    }
}

/// A busy survival session: stats, XP, hotbar, a ten-row sidebar, a boss bar,
/// five chat lines, and a title.
fn busy_session(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(player_runtime, &mut runtime);
    runtime.hud.set_experience(12, 0.4);
    let rows: Vec<_> = (0..10)
        .map(|row| {
            (
                i64::from(row) + 10,
                ProtocolScoreIdentity::FakePlayer(Arc::from(format!("Line {row}"))),
                row,
            )
        })
        .collect();
    super::retained_hud_tests::install_mixed_scoreboard_slot(
        player_runtime,
        &mut runtime,
        "sidebar",
        &rows,
    );
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 30,
                local_millis: 0,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::Show,
                    9,
                    "Boss",
                    0.5,
                    ProtocolBossColor::Red,
                    ProtocolBossOverlay::Progress,
                ),
            },
        )
        .unwrap();
    for line in 0..5 {
        runtime
            .apply(
                player_runtime,
                chat_line(40 + line, &format!("chat line {line}")),
            )
            .unwrap();
    }
    runtime.hud.set_title(Arc::from("Round 1"), 50, 0);
    full_stats(player_runtime, &mut runtime, 60);
    runtime
}

// Steady and changing-data frame costs, printed when the ignored benchmark is selected.
#[test]
#[ignore = "manual HUD frame-cost benchmark; requires installed local carriers (make assets)"]
fn hud_frame_timing() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation =
        engine_presentation().expect("required offline fixture; see the ignore reason");
    *presentation.hud_frame_mut() = first_person();
    let mut runtime = busy_session(&mut player_runtime);
    let frames: u32 = std::env::var("HUD_FRAMES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(240);
    build_at(
        &player_runtime,
        &mut presentation,
        &runtime,
        100,
        [1920, 1080],
        1.0,
    );
    let started = std::time::Instant::now();
    for frame in 0..frames {
        build_at(
            &player_runtime,
            &mut presentation,
            &runtime,
            100 + u64::from(frame),
            [1920, 1080],
            1.0,
        );
    }
    let steady = started.elapsed() / frames;
    let started = std::time::Instant::now();
    for frame in 0..frames {
        runtime.hud.set_experience(12, frame as f32 / frames as f32);
        build_at(
            &player_runtime,
            &mut presentation,
            &runtime,
            100 + u64::from(frame),
            [1920, 1080],
            1.0,
        );
    }
    let changing = started.elapsed() / frames;
    eprintln!("engine HUD frame: steady {steady:?}, re-bound every frame {changing:?}");
}

// An N-notch overlay adds exactly N-1 dividers over the bar.
#[test]
fn notched_boss_overlays_draw_their_dividers() {
    let quads = |overlay| {
        let mut player_runtime = player_state::PlayerState::new(1);
        let mut presentation = engine_presentation()?;
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(
                &mut player_runtime,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 0,
                    server_tick: None,
                    event: boss_event(
                        ProtocolBossAction::Show,
                        9,
                        "",
                        1.0,
                        ProtocolBossColor::Red,
                        overlay,
                    ),
                },
            )
            .unwrap();
        Some(
            build(&player_runtime, &mut presentation, &runtime, 0)
                .vertices
                .len()
                / 4,
        )
    };
    let Some(plain) = quads(ProtocolBossOverlay::Progress) else {
        eprintln!(
            "skipping notched_boss_overlays_draw_their_dividers: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    assert_eq!(quads(ProtocolBossOverlay::Notched6), Some(plain + 5));
    assert_eq!(quads(ProtocolBossOverlay::Notched20), Some(plain + 19));
}

/// A saved visibility edit changes the emitted HUD and restores it without a reload.
#[test]
fn settings_hide_hud_suppresses_the_rendered_overlay() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping settings_hide_hud_suppresses_the_rendered_overlay: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player_runtime.inventory.set_local_selected_slot(0);
    let shown = build(&player_runtime, &mut presentation, &runtime, 0);
    let mut options = SettingsOptions::default();
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "hide_hud")
        .unwrap();
    options.set(index, 1);
    presentation.set_chat_settings_snapshot((Arc::new(options.clone()), None));
    let hidden = build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(hidden.vertices.len() < shown.vertices.len());
    options.set(index, 0);
    presentation.set_chat_settings_snapshot((Arc::new(options), None));
    let restored = build(&player_runtime, &mut presentation, &runtime, 0);
    assert_eq!(restored.vertices.len(), shown.vertices.len());
}
