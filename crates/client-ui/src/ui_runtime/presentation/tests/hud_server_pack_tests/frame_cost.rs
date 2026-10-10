//! Offline timings through the installed font, server packs, HUD painter and scene publisher.

use super::*;
use std::time::{Duration, Instant};

const SAMPLES: usize = 500;

#[test]
#[ignore = "release benchmark; needs local carriers and CINNABAR_FORM_PACK_DIR"]
fn offline_server_hud_publication_cost() {
    use super::super::super::forms::pack_harness;
    let Some(pack) = pack_harness::env_pack() else {
        eprintln!("HUD_PUBLICATION skipped: no offline pack directories");
        return;
    };
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!("HUD_PUBLICATION skipped: no installed carriers");
        return;
    };
    presentation.set_server_ui_pack(&pack);
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player, "Zeqa lobby");
    runtime.set_session_glyphs(pack_harness::env_glyphs());
    let mut scene = render_model::UiRenderScene::default();
    let stats = render_model::UiRenderStats::default();
    for changing in [false, true] {
        let mut samples = Vec::with_capacity(SAMPLES);
        let before = presentation.hud_passes();
        for sample in 0..=SAMPLES {
            let index = sample as u64;
            let now = index * 8;
            if changing {
                runtime.hud.set_actionbar(
                    Arc::from(format!("Online: {} | Ping: {}ms", index % 7, index % 13)),
                    index + 100,
                    now,
                );
            }
            let started = Instant::now();
            let input = presentation
                .build(
                    &player,
                    &runtime,
                    now,
                    [2560, 1440],
                    DpiScale::new(2.0).unwrap(),
                )
                .unwrap();
            assert!(
                !input.vertices.is_empty(),
                "benchmark must publish visible HUD nodes"
            );
            scene.publish(input, &stats).unwrap();
            if index > 0 {
                samples.push(started.elapsed());
            }
        }
        report(changing, &mut samples, presentation.hud_passes() - before);
    }
}

/// Reports warm percentiles separately for idle paint and changing action-bar data.
fn report(changing: bool, samples: &mut [Duration], passes: usize) {
    samples.sort_unstable();
    eprintln!(
        "HUD_PUBLICATION changing={changing} n={} median_ms={:.3} p99_ms={:.3} bind_layout_passes={passes}",
        samples.len(),
        samples[(samples.len() - 1) / 2].as_secs_f64() * 1e3,
        samples[(samples.len() - 1) * 99 / 100].as_secs_f64() * 1e3
    );
}

/// Per-frame publication on a busy lobby: a full sidebar, chat, a hotbar and thirty name tags.
#[test]
fn lobby_ui_publication_cost() {
    use super::super::super::forms::pack_harness;
    use crate::ui_runtime::presentation::{
        PendingUiPublication, PreviewCapture, nametags::NametagAnchor, render_prepared_ui,
    };
    if std::env::var_os("CINNABAR_LOBBY_BENCH").is_none() {
        eprintln!("LOBBY_PUBLICATION skipped: set CINNABAR_LOBBY_BENCH=1");
        return;
    }
    let compiled =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let read = |name: &str| std::fs::read(compiled.join(name)).ok();
    let (Some(hud), Some(icons), Some(carrier)) = (
        read("vanilla-v1.mcbehud"),
        read("vanilla-v1.mcbeico"),
        pack_harness::carrier(),
    ) else {
        eprintln!("LOBBY_PUBLICATION skipped: no installed carriers");
        return;
    };
    let icons = Arc::new(assets::RuntimeIconCatalog::decode(&icons).unwrap());
    let hotbar: Vec<_> = icons.entries()[..9]
        .iter()
        .map(|entry| entry.identifier.to_string())
        .collect();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        pack_harness::font(),
        Arc::new(RuntimeHudCatalog::decode(&hud).unwrap()),
        icons,
    )
    .unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    if let Some(pack) = pack_harness::env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    // The client always installs GUI models, which compare skins instead of hashing them.
    presentation.gui_models.enabled = true;
    let hotbar_icons: Vec<_> = hotbar
        .iter()
        .map(|identifier| presentation.item_icon(identifier, 0))
        .collect();
    let frame = presentation.hud_frame_mut();
    frame.first_person = true;
    for (slot, icon) in hotbar_icons.into_iter().enumerate() {
        frame.hotbar_icons[slot] = icon;
        frame.hotbar_stacks[slot] = Some(protocol::NetworkItemStack {
            network_id: 1 + slot as i32,
            metadata: 0,
            stack_network_id: -1,
            count: 1,
            nbt_digest: [0; 32],
            block_runtime_id: 0,
            extra_data: Arc::from([]),
        });
    }
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player, "§l§bZEQA §fLOBBY");
    runtime.set_session_glyphs(pack_harness::env_glyphs());
    let entries = (0..15)
        .map(|row| ProtocolScoreEntry {
            action: ProtocolScoreAction::Change,
            scoreboard_id: i64::from(row) + 100,
            objective_name: Arc::from("objective"),
            score: row,
            identity: ProtocolScoreIdentity::FakePlayer(Arc::from(format!(
                "§7» §fStat {row}: §b{}§r",
                row * 37
            ))),
        })
        .collect();
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 100,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Score(ScoreEvent { entries }),
            },
        )
        .unwrap();
    for line in 0..40u64 {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 200 + line,
                    local_millis: 0,
                    server_tick: None,
                    event: chat_event(&format!(
                        "§7[§bMember§7] §fPlayer{line}§7: hello lobby {line}"
                    )),
                },
            )
            .unwrap();
    }
    let anchors: Vec<_> = (0..30u64)
        .map(|index| NametagAnchor {
            runtime_id: index + 10,
            position: bevy::math::Vec3::new(index as f32 * 0.7, 66.0, 4.0 + index as f32),
            lines: vec![
                Arc::from(format!("§a[Member] §fPlayer{index}")),
                Arc::from(format!("§c{} ❤", 20 - index % 20)),
            ],
            depth_tested: false,
            text_alpha: 1.0,
            distance: 4.0 + index as f32,
        })
        .collect();
    let mut scene = render_model::UiRenderScene::default();
    let stats = render_model::UiRenderStats::default();
    let samples = std::env::var("CINNABAR_LOBBY_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(SAMPLES);
    // Lobby chat arrives about twice a second.
    let chatty = std::env::var_os("CINNABAR_LOBBY_CHAT").is_some();
    let mut stages = [const { Vec::new() }; 4];
    for sample in 0..=samples {
        let now = 1_000 + sample as u64 * 16;
        if chatty && sample % 30 == 0 {
            runtime
                .apply(
                    &mut player,
                    SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: 1_000 + sample as u64,
                        local_millis: now,
                        server_tick: None,
                        event: chat_event(&format!("§7[§bMember§7] §fNew{sample}§7: gg")),
                    },
                )
                .unwrap();
        }
        let started = Instant::now();
        let prepared = PendingUiPublication {
            inventory: runtime.capture_presentation_inventory(&player),
            preview: PreviewCapture {
                ready: true,
                skin: None,
                pose: Default::default(),
                shown: false,
                hands: false,
            },
            item_icons: (None, None),
            now_millis: now,
            physical_size: [2560, 1440],
            dpi_scale: DpiScale::new(2.0).unwrap(),
        };
        let captured = started.elapsed();
        let input = render_prepared_ui(&player, &mut runtime, &mut presentation, prepared).unwrap();
        let built = started.elapsed();
        presentation.set_nametag_anchors(anchors.clone());
        let _tags = presentation.nametag_scene();
        let tagged = started.elapsed();
        scene.publish(input, &stats).unwrap();
        let published = started.elapsed();
        if sample > 0 {
            for (stage, elapsed) in stages.iter_mut().zip([
                captured,
                built - captured,
                tagged - built,
                published - tagged,
            ]) {
                stage.push(elapsed);
            }
        }
    }
    for (name, samples) in ["capture", "build", "nametags", "publish"]
        .into_iter()
        .zip(&mut stages)
    {
        report_stage(name, samples);
    }
    // One retained-state change per frame: the UI build is the cost a change pays.
    let mut sequence = 10_000u64;
    let updates = std::env::var("CINNABAR_LOBBY_UPDATES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let kinds = std::env::var("CINNABAR_LOBBY_UPDATE_KIND").ok();
    for kind in ["chat_line", "score_row", "title"]
        .into_iter()
        .filter(|kind| kinds.as_deref().is_none_or(|only| only == *kind))
    {
        let mut builds = Vec::with_capacity(updates);
        for update in 0..=updates {
            sequence += 1;
            let now = 20_000 + sequence * 16;
            let event = match kind {
                "chat_line" => chat_event(&format!("§7[§bMember§7] §fUpd{update}§7: gg")),
                "score_row" => UiEvent::Score(ScoreEvent {
                    entries: vec![ProtocolScoreEntry {
                        action: ProtocolScoreAction::Change,
                        scoreboard_id: 100 + (update % 15) as i64,
                        objective_name: Arc::from("objective"),
                        score: (update % 15) as i32,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from(format!(
                            "§7» §fStat {}: §b{update}§r",
                            update % 15
                        ))),
                    }]
                    .into(),
                }),
                _ => title_event(&format!("§6Round {update}")),
            };
            runtime
                .apply(
                    &mut player,
                    SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: sequence,
                        local_millis: now,
                        server_tick: None,
                        event,
                    },
                )
                .unwrap();
            let prepared = PendingUiPublication {
                inventory: runtime.capture_presentation_inventory(&player),
                preview: PreviewCapture {
                    ready: true,
                    skin: None,
                    pose: Default::default(),
                    shown: false,
                    hands: false,
                },
                item_icons: (None, None),
                now_millis: now,
                physical_size: [2560, 1440],
                dpi_scale: DpiScale::new(2.0).unwrap(),
            };
            let started = Instant::now();
            let input =
                render_prepared_ui(&player, &mut runtime, &mut presentation, prepared).unwrap();
            if update > 0 {
                builds.push(started.elapsed());
            }
            scene.publish(input, &stats).unwrap();
        }
        report_stage(&format!("update_{kind}"), &mut builds);
    }
}

fn title_event(text: &str) -> UiEvent {
    UiEvent::Title(protocol::TitleEvent {
        action: protocol::TitleAction::SetTitle,
        text: Arc::from(text),
        document: None,
        fade_in_ticks: 10,
        stay_ticks: 70,
        fade_out_ticks: 20,
        xuid: Arc::from(""),
        platform_online_id: Arc::from(""),
        filtered_message: Arc::from(""),
    })
}

fn report_stage(name: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    let mean = samples.iter().sum::<Duration>() / samples.len() as u32;
    eprintln!(
        "LOBBY_PUBLICATION stage={name} n={} mean_ms={:.3} median_ms={:.3} p99_ms={:.3}",
        samples.len(),
        mean.as_secs_f64() * 1e3,
        samples[(samples.len() - 1) / 2].as_secs_f64() * 1e3,
        samples[(samples.len() - 1) * 99 / 100].as_secs_f64() * 1e3
    );
}
