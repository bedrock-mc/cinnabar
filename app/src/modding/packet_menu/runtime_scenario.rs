//! Optional end-to-end packet reactor, retained app state and real HUD painter scenario.

use super::*;
use client_ui::ui_runtime::{UiRuntime, presentation::ServerUiPack};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::PathBuf,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    size: [u32; 2],
    #[serde(default)]
    player_block: Option<[i32; 3]>,
    #[serde(default)]
    coordinates_visible: bool,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    name: String,
    millis: u64,
    #[serde(default)]
    event: Option<mod_api::native_menu::Event>,
    #[serde(default)]
    key: Option<mod_api::native_menu::Key>,
    #[serde(default)]
    toggle: bool,
    #[serde(default)]
    command: Option<PathBuf>,
    #[serde(default)]
    pointer: Option<Pointer>,
    #[serde(default)]
    hud_title: Option<String>,
    #[serde(default)]
    hud_subtitle: Option<String>,
    #[serde(default)]
    expect_open: Option<bool>,
    #[serde(default)]
    expect_geometry: Vec<String>,
    #[serde(default)]
    expect_preview_sizes: Vec<String>,
    #[serde(default)]
    expect_rect_change: Vec<String>,
    #[serde(default)]
    expect_scale_grow: Vec<String>,
    #[serde(default)]
    expect_panel_document_unchanged: bool,
    #[serde(default)]
    expect_paint_change: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    root: String,
    fraction: [f32; 2],
    #[serde(default)]
    delta: [f32; 2],
    pressed: bool,
    held: bool,
}

/// Collects the real pack definitions and original textures returned by guest startup.
fn pack(
    packs: Vec<protocol::RelayPack>,
) -> (
    ServerUiPack,
    Option<Arc<client_ui::ui_runtime::presentation::SessionGlyphSheets>>,
) {
    let mut result = ServerUiPack::default();
    let base = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
    let (stack, errors) = resource_pack::compose_added_packs(
        &base,
        packs
            .iter()
            .map(|pack| resource_pack::AddedPack {
                archive: pack.archive.clone(),
                above_server: pack.above_server,
            })
            .collect(),
        8 << 30,
    );
    assert!(
        errors.is_empty(),
        "scenario packs must pass normal admission: {errors:?}"
    );
    let view = resource_pack::LayeredPackView::new(Arc::new(stack));
    let glyphs = crate::runtime::network::glyph_sheets::compile_session_glyphs(&view);
    result.view = Some(view);
    for pack in packs {
        let mut archive = zip::ZipArchive::new(Cursor::new(pack.archive)).unwrap();
        let mut layer = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().replace('\\', "/");
            let ui = name.starts_with("ui/") && name.ends_with(".json");
            let image = ServerUiPack::is_image_path(&name);
            if !ui && !image {
                continue;
            }
            assert!(file.size() <= 16 * 1024 * 1024, "oversized scenario asset");
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            if ui {
                layer.push((name, bytes));
            } else {
                result.textures.push((name, bytes));
            }
        }
        result.ui_layers.push(layer);
    }
    (result, glyphs)
}

/// Publishes opaque title fixtures through the normal UI event owner.
fn title(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    text: String,
    subtitle: bool,
    sequence: u64,
    millis: u64,
) {
    runtime
        .apply(
            player,
            client_ui::ui_runtime::SequencedUiEvent {
                session_id: 1,
                fifo_sequence: sequence,
                local_millis: millis,
                server_tick: None,
                event: protocol::UiEvent::Title(protocol::TitleEvent {
                    action: if subtitle {
                        protocol::TitleAction::SetSubtitle
                    } else {
                        protocol::TitleAction::SetTitle
                    },
                    text: text.into(),
                    document: None,
                    fade_in_ticks: 0,
                    stay_ticks: 10_000,
                    fade_out_ticks: 0,
                    xuid: "".into(),
                    platform_online_id: "".into(),
                    filtered_message: "".into(),
                }),
            },
        )
        .unwrap();
}

#[test]
fn external_reactor_runs_retained_menu_and_actual_pack_hud_scenario() {
    let (Some(wasm), Some(capture), Some(scenario)) = (
        std::env::var_os("CINNABAR_NATIVE_WASM"),
        std::env::var_os("CINNABAR_NATIVE_CAPTURE"),
        std::env::var_os("CINNABAR_NATIVE_SCENARIO"),
    ) else {
        eprintln!(
            "missing fixture: CINNABAR_NATIVE_WASM, CINNABAR_NATIVE_CAPTURE, CINNABAR_NATIVE_SCENARIO; skipping external reactor painter scenario"
        );
        return;
    };
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.local/assets/compiled");
    let font = std::env::var_os("CINNABAR_NATIVE_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiled.join("ui-cinnangles-sans-v1.mcbefont"));
    let Ok(bytes) = std::fs::read(font) else {
        eprintln!(
            "missing fixture: installed font carrier or CINNABAR_NATIVE_FONT; skipping external reactor painter scenario"
        );
        return;
    };
    let manifest = assets::canonical_source_manifest_sha256(
        &std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/cinnangles-sans-source.json"),
        )
        .unwrap(),
    );
    let font = assets::RuntimeFontCatalog::decode(&bytes, manifest)
        .unwrap()
        .with_coverage_pages();
    let font = mod_ui::fonts::install(Arc::new(font)).unwrap();
    let Some(presentation) = client_ui::test_support::engine_presentation_with(font) else {
        eprintln!(
            "missing fixture: installed UI carriers (make assets); skipping external reactor painter scenario"
        );
        return;
    };
    let scenario: Scenario = serde_json::from_slice(&std::fs::read(scenario).unwrap()).unwrap();
    assert!(
        scenario.steps.len() <= 256 && scenario.size.iter().all(|size| *size > 0 && *size <= 4096)
    );
    let records = mod_host::packet_mod::capture::parse(&std::fs::read(capture).unwrap()).unwrap();
    let (identity, client_data, startup) = records
        .iter()
        .find_map(|record| match record {
            mod_host::packet_mod::capture::Record::Start {
                identity,
                client_data,
                startup,
            } => Some((identity, client_data, startup)),
            _ => None,
        })
        .expect("synthetic capture needs startup");
    let mut packets = PacketMods::default();
    packets.select_environment(
        client_extensions::modding::packet_relay::PacketModSelection {
            path: wasm.into(),
            grants: mod_host::ModGrants {
                packets: true,
                wasi: true,
                controls: true,
                ..Default::default()
            },
        },
    );
    let relay = packets.session_relay().unwrap();
    let packs = relay.start(
        &protocol::session_wire::SessionIdentity {
            display_name: identity.display_name.clone(),
            xuid: identity.xuid.clone(),
            uuid: identity.uuid.clone(),
        },
        client_data,
        startup,
    );
    assert!(
        packets.native_menu_identity().is_some(),
        "guest startup must negotiate native controls"
    );
    assert!(
        !packets.native_frame_available(),
        "camera grants remain independent"
    );
    let mut presentation = UiPresentationResource(presentation);
    let (pack, glyphs) = pack(packs);
    presentation.set_server_ui_pack(&pack);
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_glyphs(glyphs);
    runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(scenario.coordinates_visible),
        ..Default::default()
    });
    presentation.hud_frame_mut().player_block = scenario.player_block;
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let viewport = Some(
        scenario
            .size
            .map(|size| size as f32 / ui::FONT_DESIGN_PIXEL_TEXELS as f32),
    );
    let mut sequence = 1;
    let mut geometry = BTreeMap::<String, ui::mod_hud::HudGeometry>::new();
    let mut document = None;
    let mut previous_vertices = None;
    let mut grab = None;
    let mut pointer_held = false;
    let mut report = Vec::new();
    for step in scenario.steps {
        presentation.set_player_preview_skin(
            Some(render_model::default_actor_skin_rgba8().as_ref()),
            Default::default(),
        );
        presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
        assert!(
            !step.name.is_empty()
                && step
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        );
        for (text, subtitle) in [(step.hud_title, false), (step.hud_subtitle, true)] {
            if let Some(text) = text {
                title(
                    &mut player,
                    &mut runtime,
                    text,
                    subtitle,
                    sequence,
                    step.millis,
                );
                sequence += 1;
            }
        }
        if let Some(command) = step.command {
            let _ = relay.outbound(&std::fs::read(command).unwrap());
        }
        if step.toggle {
            toggle(&mut packets, &mut presentation);
        }
        if let Some(event) = step.event {
            packets.menu.pending.events.push(event);
        }
        if let Some(key) = step.key {
            packets.menu.pending.keys.push(key);
        }
        let mut edits = Vec::new();
        if let Some(pointer) = step.pointer {
            if pointer.pressed {
                let root = geometry
                    .get(&pointer.root)
                    .expect("pointer root needs actual painted HUD geometry");
                grab = Some([
                    root.rect[0] + root.rect[2] * pointer.fraction[0],
                    root.rect[1] + root.rect[3] * pointer.fraction[1],
                ]);
            }
            let start = grab.expect("pointer samples require a preceding press");
            let point = std::array::from_fn(|axis| {
                (start[axis] + pointer.delta[axis]) * ui::FONT_DESIGN_PIXEL_TEXELS as f32
            });
            edits =
                presentation
                    .mod_ui_mut()
                    .mod_panel_events(point, pointer.pressed, pointer.held);
            packets
                .menu
                .pending
                .events
                .extend(edits.iter().map(|event| mod_api::native_menu::Event {
                    id: event.id.clone(),
                    value: event.value,
                }));
            pointer_held = pointer.held;
            if !pointer.held {
                grab = None;
            }
        }
        // The synchronous relay tick is a deterministic serial-worker barrier, not a sleep.
        for _ in 0..3 {
            packets.menu.pending.pointer_held = pointer_held;
            poll(&mut packets, &mut presentation, 1, viewport);
            let _ = relay.tick();
        }
        packets.menu.pending.pointer_held = pointer_held;
        poll(&mut packets, &mut presentation, 1, viewport);
        let frame = presentation
            .build(
                &player,
                &runtime,
                step.millis,
                scenario.size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let requests = packets
            .menu
            .retained
            .as_ref()
            .and_then(|output| output.output.hud_geometry.as_ref())
            .cloned()
            .unwrap_or_default();
        let next = presentation.mod_hud_geometry(&requests, 1);
        let preview_sizes = presentation
            .mod_control_preview_sizes(PanelUiOwner::PacketSession(packets.menu.relay.unwrap()), 1);
        for name in &step.expect_preview_sizes {
            assert!(
                preview_sizes.contains_key(name),
                "{}: missing pack-control preview {name}; measured {:?}",
                step.name,
                preview_sizes
            );
        }
        for name in &step.expect_geometry {
            assert!(
                next.contains_key(name),
                "{}: missing actual HUD root {name}; observed {:?}",
                step.name,
                next.keys()
            );
        }
        for name in &step.expect_rect_change {
            assert_ne!(
                geometry.get(name).unwrap().rect,
                next.get(name).unwrap().rect,
                "{}: HUD did not move",
                step.name
            );
        }
        for name in &step.expect_scale_grow {
            assert!(
                next[name].rect[2] > geometry[name].rect[2],
                "{}: HUD did not grow",
                step.name
            );
        }
        let next_document = packets
            .menu
            .retained
            .as_ref()
            .and_then(|output| output.output.panel.as_ref())
            .and_then(|panel| panel.surface.as_ref())
            .map(|surface| surface.document.clone());
        if step.expect_panel_document_unchanged {
            assert_eq!(
                document, next_document,
                "{}: measurement changed document/capture identity",
                step.name
            );
        }
        if step.expect_paint_change {
            assert_ne!(
                previous_vertices.as_ref(),
                Some(&frame.vertices),
                "{}: clock did not change painted vertices",
                step.name
            );
        }
        if let Some(open) = step.expect_open {
            assert_eq!(
                presentation.mod_ui().mod_panel_open(),
                open,
                "{}",
                step.name
            );
        }
        client_ui::test_support::write_snapshot(
            &frame,
            &format!(
                "external-{}-{}x{}",
                step.name, scenario.size[0], scenario.size[1]
            ),
        );
        report.push(serde_json::json!({"step":step.name,"millis":step.millis,"open":presentation.mod_ui().mod_panel_open(),"geometry":next,"preview_sizes":preview_sizes,"edits":edits,"panel":packets.menu.retained.as_ref().map(|output|&output.output)}));
        geometry = next;
        document = next_document;
        previous_vertices = Some(frame.vertices);
    }
    if let Some(directory) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") {
        std::fs::write(
            PathBuf::from(directory).join(format!(
                "external-scenario-{}x{}.json",
                scenario.size[0], scenario.size[1]
            )),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    relay.stop();
    poll(&mut packets, &mut presentation, 1, viewport);
    assert!(packets.menu.retained.is_none());
}
