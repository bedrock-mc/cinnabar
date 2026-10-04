//! Local-only: the engine HUD under real server resource packs, unpacked into
//! the `:`-separated directories `CINNABAR_HUD_PACK_DIRS` names (each is its
//! own session). These checks name missing fixtures and skip when absent.

use json_ui::{Draw, DrawNode};
use protocol::{PlayerGameMode, ScoreIdentity as ProtocolScoreIdentity};

use super::engine_hud_tests::engine_presentation;
use super::*;
use crate::ui_runtime::presentation::forms::pack_harness::dir_pack;

const PACK_ENV: &str = "CINNABAR_HUD_PACK_DIRS";

/// A populated session: stats, hotbar, sidebar, boss bar, title, and chat.
fn session(player_runtime: &mut player_state::PlayerState, objective: &str) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player_runtime.inventory.set_local_selected_slot(0);
    runtime.hud.set_stats(
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        None,
    );
    runtime.hud.set_experience(12, 0.3);
    super::retained_hud_tests::install_mixed_scoreboard_slot(
        player_runtime,
        &mut runtime,
        "sidebar",
        &[
            (
                3,
                ProtocolScoreIdentity::FakePlayer(Arc::from("Kills: 4")),
                3,
            ),
            (
                4,
                ProtocolScoreIdentity::FakePlayer(Arc::from("zeqa.net")),
                2,
            ),
        ],
    );
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 10,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Objective(ObjectiveEvent::Display {
                    display_slot: Arc::from("sidebar"),
                    objective_name: Arc::from("objective"),
                    display_name: Arc::from(objective),
                    criteria_name: Arc::from("dummy"),
                    sort_order: 1,
                }),
            },
        )
        .unwrap();
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 11,
                local_millis: 0,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::Show,
                    9,
                    "Dragon",
                    0.6,
                    ProtocolBossColor::Pink,
                    ProtocolBossOverlay::Progress,
                ),
            },
        )
        .unwrap();
    for (sequence, line) in [(12, "hello"), (13, "toast.Welcome back")] {
        runtime
            .apply(
                player_runtime,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: 0,
                    server_tick: None,
                    event: chat_event(line),
                },
            )
            .unwrap();
    }
    runtime.hud.set_title(Arc::from("Round 1"), 20, 0);
    runtime
}

fn textures(nodes: &[DrawNode]) -> std::collections::BTreeSet<&str> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } => Some(texture.as_str()),
            _ => None,
        })
        .collect()
}

fn drawn_contains(nodes: &[DrawNode], wanted: &str) -> bool {
    textures(nodes).contains(wanted)
}

fn texts(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } if !text.is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn server_packs_restyle_the_engine_hud() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Ok(dirs) = std::env::var(PACK_ENV) else {
        eprintln!(
            "skipping server_packs_restyle_the_engine_hud: fixture unavailable; requires installed UI carrier (make assets) and CINNABAR_HUD_PACK_DIRS"
        );
        return;
    };
    for dir in dirs.split(':').filter(|dir| !dir.is_empty()) {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping server_packs_restyle_the_engine_hud: fixture unavailable; requires installed UI carrier (make assets) and CINNABAR_HUD_PACK_DIRS"
            );
            return;
        };
        presentation.set_server_ui_pack(&dir_pack([dir]));
        let name = std::path::Path::new(dir)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let objective = if name.starts_with("eba25239") {
            "support.playhive.com/ui"
        } else {
            "Objective"
        };
        let runtime = session(&mut player_runtime, objective);
        let started = std::time::Instant::now();
        presentation
            .build(
                &player_runtime,
                &runtime,
                500,
                [1920, 1080],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let first = started.elapsed();
        // The next frame draws what the first asked the server atlas for.
        presentation
            .build(
                &player_runtime,
                &runtime,
                516,
                [1920, 1080],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        // Oversized pack art (a 5142x706 watermark) stays out of the atlas.
        let missing = presentation.hud_unresolved_sprites();
        let nodes = presentation.hud_draw_nodes();
        eprintln!(
            "== {name}: {} nodes, first frame {first:?}\n   textures {:?}\n   texts {:?}\n   unresolved {missing:?}",
            nodes.len(),
            textures(nodes),
            texts(nodes)
        );
        let resolved = |texture: &str| {
            drawn_contains(nodes, texture) && !missing.iter().any(|gap| gap == texture)
        };
        let written = texts(nodes);
        match name.get(..8).unwrap_or_default() {
            // Zeqa: its own sidebar art, no score column or title band, and
            // `toast.` chat lines drawn as its toasts instead of chat.
            "52e0000e" => {
                assert!(resolved("textures/ui/zeqa/scoreboard/Black_sb"));
                assert!(!written.contains(&"3") && !written.contains(&"2"));
                assert!(written.contains(&"Kills: 4"));
                assert!(resolved("textures/ui/zeqa/common/toastBorder"));
                assert!(resolved("textures/ui/zeqa/common/scrollbar"));
            }
            // Hive: the flagged objective turns the sidebar into its entries.
            "eba25239" => {
                assert!(resolved("textures/ui/hive/hive_scoreboard_entry"));
                assert!(written.contains(&"Kills: 4"));
            }
            // CubeCraft restyles its sidebar; NetherGames its boss bar and sidebar.
            "ac72a01d" => assert!(resolved("textures/ui/Black_sb")),
            "051c1187" => {
                assert!(resolved("textures/ui/ng/bossbar/filled_progress_bar"));
                assert!(resolved("textures/ui/ng/scoreboard/scoreboard"));
            }
            // Galaxite replaces the boss bar with its own art.
            "5e2431e9" => assert!(resolved("textures/ui/galaxite/boss/left")),
            _ => assert!(!nodes.is_empty()),
        }
    }
}

/// Local diagnosis: every painted node of the HUD under the pack stack
/// `CINNABAR_HUD_PACK_STACK` names (`:`-separated layers, lowest first).
#[test]
fn server_pack_stack_hud_dump() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Ok(stack) = std::env::var("CINNABAR_HUD_PACK_STACK") else {
        eprintln!(
            "skipping server_pack_stack_hud_dump: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_HUD_PACK_STACK"
        );
        return;
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping server_pack_stack_hud_dump: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_HUD_PACK_STACK"
        );
        return;
    };
    presentation.set_server_ui_pack(&dir_pack(stack.split(':').filter(|dir| !dir.is_empty())));
    let runtime = session(&mut player_runtime, "Objective");
    for now in [500, 516] {
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                now,
                [1920, 1080],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        super::super::forms::snapshot::write(&input, "hud_pack_stack");
    }
    for node in presentation.hud_draw_nodes() {
        if node.alpha <= 0.0 {
            continue;
        }
        eprintln!(
            "{:40} {:34} {:7.1} {:7.1} {:6.1} {:6.1} {:?}",
            node.name,
            node.key
                .chars()
                .rev()
                .take(34)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
            node.dest.x,
            node.dest.y,
            node.dest.w,
            node.dest.h,
            match &node.draw {
                Draw::Sprite { texture, .. } => texture.clone(),
                Draw::Text { text, .. } => format!("text {text:?}"),
                Draw::Custom { renderer, .. } => format!("custom {renderer}"),
                Draw::Solid { color } => format!("solid {color:?}"),
            }
        );
    }
}

/// Local diagnosis: Zeqa's glyph-built sidebar entries under `CINNABAR_HUD_PACK_STACK`
/// with that pack's `font/glyph_XX.png` sheets, painted to `zeqa_top_bar.png`.
#[test]
fn zeqa_top_bar_snapshot() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Ok(stack) = std::env::var("CINNABAR_HUD_PACK_STACK") else {
        eprintln!(
            "skipping zeqa_top_bar_snapshot: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_HUD_PACK_STACK"
        );
        return;
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping zeqa_top_bar_snapshot: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_HUD_PACK_STACK"
        );
        return;
    };
    presentation.set_server_ui_pack(&dir_pack(stack.split(':').filter(|dir| !dir.is_empty())));
    let mut cells = Vec::new();
    for high_byte in 0..=u8::MAX {
        let path = std::path::Path::new(&stack).join(format!("font/glyph_{high_byte:02X}.png"));
        let Ok(image) = image::open(&path) else {
            continue;
        };
        let image = image.into_rgba8();
        cells.extend(assets::extract_cells(&assets::GlyphSheet {
            high_byte,
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into_boxed_slice(),
        }));
    }
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_glyphs(Some(Arc::new(
        crate::ui_runtime::presentation::SessionGlyphSheets {
            cells,
            ..Default::default()
        },
    )));
    let names = [
        "\u{e15e}\u{e700}\u{e38e}\u{e391}\u{e384}\u{e381}\u{e388}\u{e393}\u{e392}\u{ea39}\u{e700}\u{eaa3}",
        "\u{e107}\u{e700}\u{e38a}\u{e383}\u{e391}\u{ea39}\u{e700}\u{eaa3}\u{eabe}\u{ea9d}\u{eaa2}",
        "\u{e149}\u{e700}\u{e38b}\u{e384}\u{e395}\u{e384}\u{e38b}\u{ea39}\u{e700}\u{ea9a}",
        "\u{e143}\u{e700}\u{e38b}\u{e38e}\u{e381}\u{e381}\u{e398}\u{ea39}\u{e700}\u{ea84}\u{ea94}\u{ea9e}",
        "\u{e00f}\u{e700}",
    ];
    let rows: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                index as i64,
                ProtocolScoreIdentity::FakePlayer(Arc::from(*name)),
                index as i32,
            )
        })
        .collect();
    super::retained_hud_tests::install_mixed_scoreboard_slot(
        &mut player_runtime,
        &mut runtime,
        "sidebar",
        &rows,
    );
    for now in [500, 516] {
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                now,
                [1920, 1080],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        super::super::forms::snapshot::write(&input, "zeqa_top_bar");
    }
    for node in presentation.hud_draw_nodes() {
        if node.dest.y < 40.0 && node.alpha > 0.0 {
            eprintln!(
                "{:40} {:7.1} {:7.1} {:6.1} {:6.1} a={:.2} {:?}",
                node.name,
                node.dest.x,
                node.dest.y,
                node.dest.w,
                node.dest.h,
                node.alpha,
                node.draw
            );
        }
    }
}

mod frame_cost;
