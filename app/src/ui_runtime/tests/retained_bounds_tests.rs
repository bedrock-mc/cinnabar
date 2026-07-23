//! Bounded retained-memory and frame-shape evidence with every UI surface
//! active at once: chat at capacity, scoreboard and boss stores loaded,
//! effects saturated, hotbar mirrored, hearts/hunger/air/XP presented.
//!
//! These are deterministic structural bounds, not wall-clock claims: retained
//! bytes stay inside the documented store budgets, the draw list stays inside
//! the render limits, and a steady-state rebuild reuses the text-layout cache
//! without growing it. Release-profile frame-time measurement against the
//! plan.md budgets remains a separate performance gate.

use std::sync::Arc;

use protocol::{
    ActorEffectAction, ActorEffectEvent, BossAction as ProtocolBossAction,
    BossColor as ProtocolBossColor, BossEvent, BossOverlay as ProtocolBossOverlay,
    BossStyle as ProtocolBossStyle, ContainerIdentity, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, ObjectiveEvent, ScoreAction as ProtocolScoreAction,
    ScoreEntry as ProtocolScoreEntry, ScoreEvent, ScoreIdentity as ProtocolScoreIdentity,
};
use sha2::Digest;
use ui::{
    ChatMessage, ChatMessageKind, MAX_CHAT_MESSAGES, MAX_CHAT_RETAINED_BYTES,
    MAX_SCOREBOARD_RETAINED_TEXT_BYTES,
};

use super::gameplay_hud_tests::RENDERABLE_EFFECT_IDS;
use super::*;
use crate::ui_runtime::presentation::UiPresentationRuntime;

fn saturated_runtime() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut sequence = 0u64;
    let mut next = || {
        sequence += 1;
        sequence
    };

    // Chat at its retention cap.
    for index in 0..(MAX_CHAT_MESSAGES + 32) {
        let _ = runtime.chat.push(ChatMessage {
            fifo_sequence: index as u64,
            received_millis: index as u64,
            kind: ChatMessageKind::Chat,
            source: Some(Arc::from("bench")),
            message: Arc::from(format!("retained chat line {index} §a§lwith codes")),
            parameters: Arc::from([]),
        });
    }

    // A sidebar objective with a full presentation page of rows.
    runtime
        .apply(envelope(
            1,
            next(),
            UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("bench"),
                display_name: Arc::from("Bench Board"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        ))
        .unwrap();
    let entries: Vec<ProtocolScoreEntry> = (0..64)
        .map(|index| ProtocolScoreEntry {
            scoreboard_id: index,
            objective_name: Arc::from("bench"),
            score: index as i32 * 3,
            identity: ProtocolScoreIdentity::FakePlayer(Arc::from(format!("row {index}"))),
        })
        .collect();
    runtime
        .apply(envelope(
            1,
            next(),
            UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: entries.into(),
            }),
        ))
        .unwrap();

    // Eight coexisting boss bars.
    for boss in 0..8i64 {
        runtime
            .apply(envelope(
                1,
                next(),
                UiEvent::Boss(BossEvent {
                    target_entity_id: boss + 10,
                    player_id: 0,
                    action: ProtocolBossAction::Show,
                    title: Arc::from(format!("Boss {boss}")),
                    filtered_title: Arc::from(""),
                    progress: 0.75,
                    style: ProtocolBossStyle {
                        color: ProtocolBossColor::Red,
                        overlay: ProtocolBossOverlay::Progress,
                        darken_sky: None,
                        create_world_fog: None,
                    },
                }),
            ))
            .unwrap();
    }

    // Every renderable effect at once, full stats, a mirrored hotbar, and
    // titles.
    for (index, effect_id) in RENDERABLE_EFFECT_IDS.into_iter().enumerate() {
        runtime
            .apply_local_effect(
                1,
                next(),
                ActorEffectEvent {
                    dimension: 0,
                    actor_runtime_id: 1,
                    action: ActorEffectAction::Add,
                    effect_id,
                    amplifier: 1,
                    particles: true,
                    ambient: index % 2 == 0,
                    duration_ticks: -1,
                    tick: 0,
                },
                0,
            )
            .unwrap();
    }
    runtime.hud.set_stats(
        ui::BoundedStat::new(37, 40),
        ui::BoundedStat::new(13, 20),
        ui::BoundedStat::new(15, 20),
        ui::BoundedStat::new(120, 300),
    );
    runtime.hud.set_absorption(ui::BoundedStat::new(8, 8));
    runtime.hud.set_experience(30, 0.62);
    let stack = |id: i32| NetworkItemStack {
        network_id: id,
        metadata: 0,
        stack_network_id: -1,
        count: 42,
        nbt_digest: sha2::Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    };
    let slots: Vec<NetworkItemStack> = (1..=36).map(stack).collect();
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
    runtime.hud.set_title(Arc::from("§6Title"), next(), 0);
    runtime
        .hud
        .set_actionbar(Arc::from("action bar"), next(), 0);
    runtime
}

#[test]
fn retained_memory_stays_inside_documented_budgets_with_every_surface_active() {
    let runtime = saturated_runtime();

    assert_eq!(runtime.chat().messages().len(), MAX_CHAT_MESSAGES);
    assert!(runtime.chat().retained_bytes() <= MAX_CHAT_RETAINED_BYTES);
    assert!(runtime.scoreboards().retained_text_bytes() <= MAX_SCOREBOARD_RETAINED_TEXT_BYTES);
    assert!(runtime.boss_bars().retained_text_bytes() <= ui::MAX_BOSS_RETAINED_TEXT_BYTES);
    assert_eq!(
        runtime.gameplay_hud().effects().len(),
        RENDERABLE_EFFECT_IDS.len()
    );
    assert_eq!(runtime.boss_bars().stacked().len(), 8);
}

#[test]
fn saturated_frames_stay_inside_render_limits_and_reuse_the_layout_cache() {
    let mut runtime = saturated_runtime();
    // First person with a fresh selected stack: the invert-blend crosshair
    // batch and the selected-item label are part of the saturated frame.
    runtime.retain_local_selected_equipment(
        99,
        protocol::EquipmentEvent {
            actor_runtime_id: 7,
            stack: NetworkItemStack {
                network_id: 1,
                metadata: 0,
                stack_network_id: -1,
                count: 1,
                nbt_digest: sha2::Sha256::digest([]).into(),
                block_runtime_id: 0,
                extra_data: Arc::from([]),
            },
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: None,
        },
    );
    runtime.observe_selected_item_identity(10_000);
    let mut presentation = UiPresentationRuntime::with_hud(
        crate::ui_runtime::presentation::tests::fixture_font(),
        crate::ui_runtime::presentation::tests::fixture_hud(),
    )
    .unwrap();
    presentation.enable_scoreboard_background();
    presentation.hud_frame_mut().first_person = true;
    presentation.hud_frame_mut().selected_item_name = Some(Arc::from("Saturated Blade"));

    let first = presentation
        .build(
            &runtime,
            10_000,
            [1920, 1080],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(!first.vertices.is_empty());
    assert!(first.vertices.len() <= render::MAX_UI_VERTICES / 4);
    assert!(first.indices.len() <= render::MAX_UI_INDICES / 4);
    assert!(first.batches.len() <= render::MAX_UI_BATCHES / 4);
    assert_eq!(
        first
            .batches
            .iter()
            .filter(|batch| batch.blend_mode == render::UI_BLEND_INVERT)
            .count(),
        1,
        "the saturated frame draws the first-person crosshair invert batch"
    );
    // Dropping only the resolved name from the frame removes glyph quads:
    // the selected-item label was live in the saturated frame.
    presentation.hud_frame_mut().selected_item_name = None;
    let unlabeled = presentation
        .build(
            &runtime,
            10_000,
            [1920, 1080],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        unlabeled.vertices.len() < first.vertices.len(),
        "the selected-item label contributes glyphs to the saturated frame"
    );
    presentation.hud_frame_mut().selected_item_name = Some(Arc::from("Saturated Blade"));

    // Steady-state rebuilds settle the text-layout cache: no unbounded growth
    // frame over frame with identical retained content.
    let settled = presentation.layout_cache_len();
    for frame in 0..8u64 {
        presentation
            .build(
                &runtime,
                10_000 + frame,
                [1920, 1080],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    assert_eq!(presentation.layout_cache_len(), settled);
}
