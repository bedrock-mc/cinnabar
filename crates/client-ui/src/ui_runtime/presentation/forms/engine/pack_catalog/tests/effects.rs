use std::sync::Arc;

use json_ui::{Animator, Context, LayoutEnv, TextureSource, render_screen};

use super::{FixedText, vanilla};
use crate::ui_runtime::{
    SequencedUiEvent, UiRuntime,
    presentation::forms::{
        pack_harness,
        server_pack::ServerAtlas,
        snapshot,
        textures::{TextureSet, Textures},
    },
};

#[test]
fn sprite_sidecar_advances_authored_frames_and_loops() {
    let assets = crate::test_support::mini_carrier();
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &vec![255; 24 * 8 * 4],
        24,
        8,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    let files = vec![
        ("textures/ui/effect.png".to_owned(), png),
        (
            "textures/ui/effect.json".to_owned(),
            br#"{"frames":[
            {"frame":{"x":0,"y":0},"duration":40},
            {"frame":{"x":8,"y":0},"duration":70},
            {"frame":{"x":16,"y":0},"duration":20}
        ]}"#
            .to_vec(),
        ),
    ];
    let mut set = TextureSet::new(0);
    set.set_atlas(ServerAtlas::new(&files, None, 1), 0);
    let atlas = set.lock();
    let textures = Textures {
        assets: &assets,
        set: &set,
        atlas: &atlas,
        images: None,
    };
    let catalog=json_ui::Catalog::from_files([
        ("ui/_global_variables.json",b"{}".as_slice()),
        ("ui/_ui_defs.json",br#"{"ui_defs":["ui/effect.json"]}"#.as_slice()),
        ("ui/effect.json",br#"{
        "namespace":"hud",
        "frames":{"anim_type":"aseprite_flip_book","initial_uv":[0,0]},
        "hud_screen":{"type":"image","texture":"textures/ui/effect","size":[8,8],"uv_size":[8,8],"uv":"@hud.frames"}
    }"#.as_slice())]).unwrap();
    let frame = render_screen(
        json_ui::HUD_SCREEN,
        &catalog,
        &Context::desktop(),
        &Default::default(),
        [100.0, 100.0],
        &LayoutEnv {
            text: &FixedText,
            textures: &textures,
        },
        &Default::default(),
    )
    .unwrap();
    let node = frame.nodes.first().unwrap();
    let mut animator = Animator::new();
    for (seconds, column) in [(0.0, 0), (0.04, 1), (0.11, 2), (0.13, 0)] {
        let uv = node
            .animate(&mut animator, seconds, None, Some(&textures))
            .uv
            .unwrap();
        assert!(
            (uv.u0 - column as f32 / 3.0).abs() < 0.00001,
            "sidecar frame did not advance at {seconds}: {uv:?}"
        );
    }
    assert!(textures.aseprite_frames("textures/ui/effect").is_some());
}

fn admitted_pack() -> Option<super::super::super::ServerUiPack> {
    let Some(dirs) = std::env::var_os("CINNABAR_TEST_EFFECT_PACK_DIRS") else {
        eprintln!(
            "skipping admitted_effects_publish_from_raw_messages: missing CINNABAR_TEST_EFFECT_PACK_DIRS fixture"
        );
        return None;
    };
    let roots = std::env::split_paths(&dirs).collect::<Vec<_>>();
    let mut pack = super::super::super::ServerUiPack::default();
    for root in roots {
        if !root.is_dir() {
            eprintln!(
                "skipping admitted_effects_publish_from_raw_messages: missing {}",
                root.display()
            );
            return None;
        }
        let mut files = Vec::new();
        for prefix in ["ui", "textures/ui"] {
            files.extend(
                pack_harness::pack_files(&root.join(prefix))
                    .into_iter()
                    .map(|(path, bytes)| (format!("{prefix}/{path}"), bytes)),
            );
        }
        pack.ui_layers.push(
            files
                .iter()
                .filter(|(path, _)| path.starts_with("ui/") && path.ends_with(".json"))
                .cloned()
                .collect(),
        );
        pack.textures.extend(
            files
                .into_iter()
                .filter(|(path, _)| path.starts_with("textures/")),
        );
    }
    Some(pack)
}

#[test]
fn admitted_effects_publish_from_raw_messages() {
    let Some(pack) = admitted_pack() else { return };
    if vanilla("admitted_effects_publish_from_raw_messages").is_none() {
        return;
    };
    for (trigger, actionbar, expected) in [
        ("ui.static", false, "textures/ui/aseprite/static"),
        ("ui.jumpscare", false, "textures/ui/aseprite/glitch"),
        ("ui.halloween.light", true, "textures/ui/standards/vignette"),
    ] {
        let mut presentation =
            crate::test_support::engine_presentation_with(pack_harness::font()).unwrap();
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        let mut runtime = UiRuntime::new(1);
        runtime.set_server_ui(Some(Arc::new(pack.clone())));
        let event = if actionbar {
            protocol::UiEvent::Title(protocol::TitleEvent {
                action: protocol::TitleAction::ActionBar,
                text: Arc::from(trigger),
                document: None,
                fade_in_ticks: 0,
                stay_ticks: 0,
                fade_out_ticks: 0,
                xuid: Arc::from(""),
                platform_online_id: Arc::from(""),
                filtered_message: Arc::from(""),
            })
        } else {
            protocol::UiEvent::Text(protocol::TextEvent {
                category: protocol::TextCategory::MessageOnly,
                kind: protocol::TextKind::Raw,
                needs_translation: false,
                source: None,
                message: Arc::from(trigger),
                parameters: Arc::from([]),
                xuid: Arc::from(""),
                platform_chat_id: Arc::from(""),
                filtered_message: None,
            })
        };
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 0,
                    server_tick: None,
                    event: event.clone(),
                },
            )
            .unwrap();
        if !actionbar {
            assert_eq!(
                runtime.chat().messages().back().unwrap().message.as_ref(),
                trigger
            );
        }
        presentation
            .build(
                &player,
                &runtime,
                120,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        presentation.finish_menu_artwork();
        let input = presentation
            .build(
                &player,
                &runtime,
                120,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let frame = presentation.last_frame.as_ref().unwrap();
        assert!(!pack_harness::drawn_texts(&frame.nodes).contains(&trigger.to_owned()));
        let drawn = presentation
            .hud_draw_nodes()
            .iter()
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Sprite { texture, .. } => Some(texture.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let missing = presentation.hud_unresolved_sprites();
        assert!(
            drawn.iter().any(|path| *path == expected),
            "{trigger} did not select its effect: {drawn:?}"
        );
        assert!(
            !missing.iter().any(|path| path == expected),
            "{trigger} effect pixels unresolved"
        );
        snapshot::write(&input, trigger);
        let raster = snapshot::rasterize(&input);
        assert_ne!(
            raster.get_pixel(20, 20).0,
            [70, 90, 110, 255],
            "{trigger} published no full-screen effect"
        );
        if !actionbar {
            check_authored_timeline(&presentation, expected);
            let first = expected_sheet_uv(&presentation, expected, 0.12);
            assert_published_sheet_uv(&presentation, first);
            let next = presentation
                .build(
                    &player,
                    &runtime,
                    170,
                    [1280, 720],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            let second = expected_sheet_uv(&presentation, expected, 0.17);
            assert_published_sheet_uv(&presentation, second);
            assert_ne!(first, second, "{trigger} did not advance its published UV");
            snapshot::write(&next, &format!("{trigger}.next"));
            write_frozen_snapshot(&pack, event, trigger);
        }
    }
}

fn write_frozen_snapshot(
    pack: &super::super::super::ServerUiPack,
    event: protocol::UiEvent,
    name: &str,
) {
    if std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR").is_none() {
        return;
    }
    let mut frozen = pack.clone();
    frozen.textures.retain(|(path, bytes)| {
        !path.ends_with(".json")
            || serde_json::from_slice(bytes)
                .ok()
                .and_then(|value| json_ui::parse_aseprite_frames(&value))
                .is_none()
    });
    let mut presentation =
        crate::test_support::engine_presentation_with(pack_harness::font()).unwrap();
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(frozen)));
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event,
            },
        )
        .unwrap();
    presentation
        .build(
            &player,
            &runtime,
            120,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    presentation.finish_menu_artwork();
    let input = presentation
        .build(
            &player,
            &runtime,
            120,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, &format!("{name}.frozen"));
}

fn check_authored_timeline(
    presentation: &crate::ui_runtime::presentation::UiPresentationRuntime,
    path: &str,
) {
    let engine = presentation.form_presentation.engine.as_deref().unwrap();
    let atlas = engine.textures.lock();
    let textures = Textures {
        assets: engine.assets(),
        set: &engine.textures,
        atlas: &atlas,
        images: None,
    };
    let frames = textures.aseprite_frames(path).unwrap();
    assert!(frames.len() >= 3);
    let node = presentation
        .hud_draw_nodes()
        .iter()
        .find(|node| matches!(&node.draw, json_ui::Draw::Sprite {texture, ..} if texture == path))
        .unwrap();
    let mut animator = Animator::new();
    let first = node
        .animate(&mut animator, 0.0, None, Some(&textures))
        .uv
        .unwrap();
    let middle_millis = frames[0].duration_ms + frames[1].duration_ms + frames[2].duration_ms / 2;
    let middle = node
        .animate(
            &mut animator,
            middle_millis as f64 / 1000.0,
            None,
            Some(&textures),
        )
        .uv
        .unwrap();
    let total: i64 = frames.iter().map(|frame| frame.duration_ms).sum();
    let repeated = node
        .animate(
            &mut animator,
            (total + middle_millis) as f64 / 1000.0,
            None,
            Some(&textures),
        )
        .uv
        .unwrap();
    let pixels = textures.texture(path).unwrap().pixels;
    assert_ne!(first, middle, "{path} remains frozen at its first frame");
    assert!((middle.u0 * pixels[0] as f32 - frames[2].x as f32).abs() < 0.001);
    assert_eq!(
        middle, repeated,
        "{path} ignores its authored loop duration"
    );
}

fn expected_sheet_uv(
    presentation: &crate::ui_runtime::presentation::UiPresentationRuntime,
    path: &str,
    now: f64,
) -> [u16; 4] {
    let engine = presentation.form_presentation.engine.as_deref().unwrap();
    let atlas = engine.textures.lock();
    let textures = Textures {
        assets: engine.assets(),
        set: &engine.textures,
        atlas: &atlas,
        images: None,
    };
    let frames = textures.aseprite_frames(path).unwrap();
    let source = textures.texture(path).unwrap().pixels;
    let (_, placement) = textures.animation_sprite(path).expect("artwork is ready");
    let ms = (now * 1000.0) as i64;
    let total: i64 = frames.iter().map(|frame| frame.duration_ms).sum();
    let mut phase = ms % total;
    let frame = frames
        .iter()
        .find(|frame| {
            if phase < frame.duration_ms {
                true
            } else {
                phase -= frame.duration_ms;
                false
            }
        })
        .unwrap();
    let node = presentation
        .hud_draw_nodes()
        .iter()
        .find(|node| matches!(&node.draw, json_ui::Draw::Sprite{texture, ..} if texture == path))
        .unwrap();
    let anim = node.anim.as_ref().unwrap();
    let size = anim.uv_size_rest.unwrap();
    let frame_rect = [
        frame.x as f64,
        frame.y as f64,
        frame.x as f64 + f64::from(size[0]),
        frame.y as f64 + f64::from(size[1]),
    ];
    std::array::from_fn(|index| {
        let axis = index % 2;
        (f64::from(placement[axis])
            + frame_rect[index] / source[axis] * f64::from(placement[axis + 2]))
        .round() as u16
    })
}

fn assert_published_sheet_uv(
    presentation: &crate::ui_runtime::presentation::UiPresentationRuntime,
    expected: [u16; 4],
) {
    assert!(
        presentation.last_frame.as_ref().unwrap().nodes.iter().any(
            |node| matches!(node.visual(), ui::UiVisual::Sprite { uv, .. } if *uv == expected)
        ),
        "authored sheet frame was not mapped to its atlas placement: {expected:?}"
    );
}
