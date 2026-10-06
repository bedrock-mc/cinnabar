//! Form presentation paths: the fallback without the carrier, and the engine's
//! render caching over a minimal in-memory carrier.
use super::super::{UiPresentationRuntime, tests::fixture_font};
use crate::ui_runtime::UiRuntime;
use protocol::{FormKind, FormRequestEvent, ModalDialogForm, ServerFormModel};
use std::sync::Arc;

pub use crate::test_support::{mini_carrier, mini_engine_presentation};

// A static form resolves and lays out once; hovering a button only repaints.
#[test]
fn static_form_resolves_once_and_hover_only_repaints() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = mini_engine_presentation();
    let mut runtime =
        super::pack_harness::action_form(&mut player_runtime, "Menu", &["A", "B", "C"]);
    let passes = |presentation: &UiPresentationRuntime| {
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .passes
    };
    for _ in 0..3 {
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation
        .form_engine_frame(identity)
        .expect("engine drew it");
    assert_eq!(frame.hits.len(), 3);
    assert_eq!(passes(&presentation), [1, 1]);
    let key = frame.hits[1].key.clone();
    runtime.server_forms_mut().engine_mut().view.hovered = Some(key);
    for _ in 0..2 {
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    assert_eq!(passes(&presentation), [1, 1]);
    let frame = presentation.form_engine_frame(identity).unwrap();
    assert_eq!(
        frame.hits.len(),
        3,
        "gated hover children add no hit regions"
    );
}

#[test]
fn review_open_form_remeasures_after_a_late_font_swap() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = mini_engine_presentation();
    let mut runtime = super::pack_harness::action_form(&mut player_runtime, "Menu", &["AAAA"]);
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let before = presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    super::snapshot::write(&before, "late-font-before");
    runtime.set_session_glyphs(Some(Arc::new(super::super::SessionGlyphSheets {
        cells: vec![assets::CellGlyph {
            codepoint: 'A',
            size: [16, 16],
            rgba8: vec![255; 16 * 16 * 4].into_boxed_slice(),
            bearing: [0, 0],
            advance_64: 16 * 64,
            draw_size_64: [16 * 64, 16 * 64],
        }],
        ..Default::default()
    })));
    let after = presentation
        .build(&player_runtime, &runtime, 1, [1280, 720], dpi)
        .unwrap();
    super::snapshot::write(&after, "late-font-after");
    let engine = presentation.form_presentation.engine.as_ref().unwrap();
    assert_eq!(
        engine.passes,
        [1, 2],
        "new glyph metrics must remeasure an open form"
    );
    for now in 2..7 {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    assert_eq!(
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .passes,
        [1, 2]
    );
    runtime.set_session_glyphs(None);
    presentation
        .build(&player_runtime, &runtime, 7, [1280, 720], dpi)
        .unwrap();
    assert_eq!(
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .passes,
        [1, 3]
    );
}

#[test]
fn selected_edit_box_rebinds_only_when_its_visible_state_changes() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".to_owned(),
            br#"{ "namespace": "server_form", "form_button": {
                "type": "edit_box", "size": [200, 30], "text_control": "display", "max_length": 50,
                "button_mappings": [{ "from_button_id": "button.menu_select",
                    "to_button_id": "button.text_edit_box_selected", "mapping_type": "pressed",
                    "handle_select": true, "handle_deselect": false }],
                "controls": [{ "display": { "type": "label", "text": "A", "size": [200, 30] } }]
            } }"#
                .to_vec(),
        )]],
        ..Default::default()
    });
    let mut runtime = super::pack_harness::action_form(&mut player_runtime, "Menu", &["A"]);
    let render = |presentation: &mut UiPresentationRuntime, runtime: &UiRuntime| {
        presentation
            .build(
                &player_runtime,
                runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .passes
    };
    render(&mut presentation, &runtime);
    let identity = runtime.server_forms().active().unwrap().identity;
    let hits = presentation
        .form_engine_frame(identity)
        .unwrap()
        .hits
        .clone();
    let edit = hits
        .iter()
        .find(|hit| hit.kind == json_ui::HitKind::EditBox)
        .unwrap();
    {
        let engine = runtime.server_forms_mut().engine_mut();
        engine.view.focused = Some(edit.key.clone());
        engine.dispatcher.button(
            &hits,
            &mut engine.view,
            json_ui::ButtonInput {
                id: "button.menu_select",
                down: true,
                point: None,
                mode: json_ui::InputMode::Gamepad,
                now: 0.0,
            },
        );
        assert_eq!(engine.view.components.selected(), Some(edit.key.as_str()));
    }
    let selected = render(&mut presentation, &runtime);
    {
        let engine = runtime.server_forms_mut().engine_mut();
        assert!(!engine.dispatcher.tick(&hits, &mut engine.view, 1.0 / 60.0));
    }
    assert_eq!(
        render(&mut presentation, &runtime),
        selected,
        "advancing the hidden caret timer must not resolve and lay out the form again"
    );
    {
        let engine = runtime.server_forms_mut().engine_mut();
        assert!(engine.dispatcher.tick(&hits, &mut engine.view, 0.31));
    }
    assert_eq!(
        render(&mut presentation, &runtime),
        [selected[0] + 1, selected[1] + 1],
        "the caret becoming hidden must still update the form"
    );
}

#[test]
fn modal_without_the_carrier_uses_the_fallback_with_both_buttons() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    let session = runtime.session_id();
    runtime.server_forms_mut().admit(
        FormRequestEvent {
            form_id: 9,
            kind: FormKind::Modal,
            title: None,
            json: Arc::from("{}"),
            model: ServerFormModel::Modal(ModalDialogForm {
                title: Arc::from("Sure?"),
                content: Arc::from("Body"),
                button1: Arc::from("Yes"),
                button2: Arc::from("No"),
            }),
        },
        1,
        session,
        false,
    );
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.set_server_ui_pack(&super::ServerUiPack {
        catalog: None,
        ui_layers: vec![vec![("ui/x.json".to_owned(), b"{}".to_vec())]],
        ..Default::default()
    });
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    assert!(presentation.form_engine_frame(identity).is_none());
    assert_eq!(presentation.form_button_count(identity), Some(2));
    assert!(presentation.engine_container_frame().is_none());
}

// The fallback dialog keeps every line of a multi-line label, each its own row
// of text inside the button, with format codes hidden.
#[test]
fn fallback_buttons_show_every_label_line() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let runtime = super::pack_harness::action_form(
        &mut player_runtime,
        "Free For All§zfp0;",
        &["Updates In - 2m 24s\n§cKills - 7\nKillstreak - 1", "Duels"],
    );
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let nodes = super::pack_harness::render(&mut presentation, &runtime, [1280, 720], 1.0);
    let texts = super::pack_harness::drawn_texts(&nodes);
    for line in [
        "Free For Allfp0;",
        "Updates In - 2m 24s",
        "Kills - 7",
        "Killstreak - 1",
    ] {
        assert!(texts.iter().any(|text| text == line), "{line}: {texts:?}");
    }
    let rows: Vec<f32> = nodes
        .iter()
        .filter(|node| {
            matches!(node.visual(), ui::UiVisual::Text { layout, .. }
                if layout.glyphs().first().is_some_and(|glyph| "UKk".contains(glyph.codepoint)))
        })
        .map(|node| node.bounds().min().y())
        .collect();
    assert_eq!(rows.len(), 3);
    assert!(rows.windows(2).all(|pair| pair[1] > pair[0]), "{rows:?}");
}

// Installing and removing a server pack's UI keeps every published frame
// acceptable to the renderer, which pins the static texture identity and plan.
#[test]
fn server_pack_install_and_removal_keep_the_renderer_accepting_frames() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use render_model::{UiRenderScene, UiRenderStats};
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 8, image::Rgba([9, 8, 7, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let pack = super::ServerUiPack {
        screen_settings: None,
        catalog: None,
        ui_layers: vec![vec![(
            "ui/server_form.json".to_owned(),
            br#"{ "namespace": "server_form", "form_button": { "modifications": [
                { "array_name": "controls", "operation": "insert_back", "value": [
                    { "art": { "type": "image", "texture": "textures/ui/pack_button", "size": [16, 8] } } ] } ] } }"#
                .to_vec(),
        )]],
        textures: vec![("textures/ui/pack_button.png".to_owned(), png)],
        view: None,
    };
    let mut presentation = mini_engine_presentation();
    let runtime = super::pack_harness::action_form(&mut player_runtime, "Menu", &["A"]);
    let (mut scene, stats) = (UiRenderScene::default(), UiRenderStats::default());
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let mut publish = |presentation: &mut UiPresentationRuntime| {
        let input = presentation
            .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
            .unwrap();
        scene.publish(input, &stats).unwrap();
    };
    publish(&mut presentation);
    presentation.set_server_ui_pack(&pack);
    assert!(
        presentation.server_ui_pages().is_empty(),
        "nothing packs until drawn"
    );
    publish(&mut presentation);
    assert_eq!(
        presentation.server_ui_pages().len(),
        1,
        "the drawn texture packed"
    );
    publish(&mut presentation);
    presentation.set_server_ui_pack(&super::ServerUiPack::default());
    assert!(presentation.server_ui_pages().is_empty());
    publish(&mut presentation);
}

fn png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn sprite_pages(nodes: &[ui::UiNode]) -> Vec<(u16, [u16; 4])> {
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Sprite {
                texture_page, uv, ..
            } => Some((*texture_page, *uv)),
            _ => None,
        })
        .collect()
}

// Button images by vanilla path draw from the item icon atlas when it holds the
// texture, else from the local vanilla pack; a URL image shows once downloaded.
#[test]
fn path_and_url_button_images_resolve_like_vanilla() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use super::super::IconRef;
    use protocol::FormButtonImage::{Path, Url};
    let vanilla = std::env::temp_dir().join(format!("forms-vanilla-{}", std::process::id()));
    std::fs::create_dir_all(vanilla.join("textures/blocks")).unwrap();
    std::fs::write(
        vanilla.join("textures/blocks/stone.png"),
        png([9, 9, 9, 255]),
    )
    .unwrap();
    let url = format!(
        "{}/remote.png",
        super::remote_images::tests::serve(png([1, 2, 3, 255]))
    );
    let mut presentation = mini_engine_presentation();
    let apple = IconRef {
        page: 0,
        uv: [0, 0, 1, 1],
        glint: false,
    };
    let icons = [("textures/items/apple".to_owned(), apple)].into();
    let engine = presentation.form_presentation.engine.as_mut().unwrap();
    engine.textures.set_fallbacks(icons, vanilla.clone());
    let remote = engine.textures.remote.clone();
    let runtime = super::pack_harness::image_form(
        &mut player_runtime,
        "Images",
        &["Item", "Block", "Remote"],
        vec![
            Some(Path("textures/items/apple".into())),
            Some(Path("textures/blocks/stone".into())),
            Some(Url(url.as_str().into())),
        ],
    );
    let server_page =
        presentation.textures.dynamic_start() + super::super::dynamic_textures::SERVER_UI_PAGE;
    let frame = |presentation: &mut UiPresentationRuntime| {
        let nodes = super::pack_harness::render(presentation, &runtime, [1280, 720], 1.0);
        sprite_pages(&nodes)
    };
    let first = frame(&mut presentation);
    assert!(
        first.contains(&(0, [0, 0, 1, 1])),
        "the icon atlas draws the item"
    );
    let on_server = |sprites: &[(u16, [u16; 4])]| {
        sprites
            .iter()
            .filter(|(page, _)| usize::from(*page) == server_page)
            .count()
    };
    assert_eq!(
        on_server(&first),
        1,
        "the block decodes from the vanilla pack"
    );
    super::remote_images::tests::settle(&remote, &url);
    let loaded = frame(&mut presentation);
    assert_eq!(on_server(&loaded), 2, "the downloaded image joins it");
    assert_eq!(presentation.server_ui_pages().len(), 1);
    let _ = std::fs::remove_dir_all(vanilla);
}

/// Six virtual px per character, nine per line.
pub(super) struct FixedText;
impl json_ui::TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

pub(super) struct NoTextures;
impl json_ui::TextureSource for NoTextures {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        Some(json_ui::TextureMeta {
            base_size: [16.0, 16.0],
            pixels: [16.0, 16.0],
            nineslice: None,
        })
    }
}

fn pause_texts() -> Option<Vec<String>> {
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.screen = crate::menu::MenuScreen::Pause;
    screen_texts(&view)
}

pub(super) fn screen_texts(view: &crate::menu::MenuView) -> Option<Vec<String>> {
    let carrier = super::pack_harness::carrier()?;
    let catalog = json_ui::Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .ok()?;
    let screen = super::menu_screens::screen_data(view, &|_| None)?;
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    let render = json_ui::render_screen(
        screen.reference,
        &catalog,
        &screen.context,
        &screen.data,
        [480.0, 270.0],
        &env,
        &json_ui::ViewState::default(),
    )?;
    Some(
        render
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect(),
    )
}

// A retail client's pause screen draws the retail content, not edu_pause's.
#[test]
fn pause_screen_draws_the_retail_buttons() {
    let Some(texts) = pause_texts() else {
        return;
    };
    for wanted in ["menu.returnToGame", "menu.settings", "pauseScreen.quit"] {
        assert!(
            texts.iter().any(|text| text == wanted),
            "{wanted}: {texts:?}"
        );
    }
}

// A pack download shows vanilla's "Downloading packs" title with the pack count and sizes.
#[test]
fn connecting_screen_reports_the_pack_download() {
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.connecting = true;
    view.feeds.join.observe(Some(crate::menu::JoinStage::Packs {
        done: 1,
        total: 3,
        received_bytes: 5 * 1024 * 1024,
        total_bytes: 20 * 1024 * 1024,
    }));
    let Some(texts) = screen_texts(&view) else {
        eprintln!(
            "skipping connecting_screen_reports_the_pack_download: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    for wanted in ["Downloading packs [1 / 3]", "[5.0MB / 20.0MB]", "Cancel"] {
        assert!(
            texts.iter().any(|text| text == wanted),
            "{wanted}: {texts:?}"
        );
    }
}

// A Realm join lays out vanilla's Realms loading screen with the lookup's words.
#[test]
fn realm_join_screen_reports_the_realm_lookup() {
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.connecting = true;
    view.feeds.join = crate::menu::JoinProgress::new(crate::menu::JoinKind::Realm);
    view.feeds.join.observe(Some(crate::menu::JoinStage::Realm));
    let Some(texts) = screen_texts(&view) else {
        eprintln!(
            "skipping realm_join_screen_reports_the_realm_lookup: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    for wanted in ["Joining Realm...", "This may take a few moments"] {
        assert!(
            texts.iter().any(|text| text == wanted),
            "{wanted}: {texts:?}"
        );
    }
}

// Opening a local world shows vanilla's loading screen with the current stage and its bytes.
#[test]
fn local_world_loading_screen_names_the_stage() {
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.local.progress = Some(crate::local_worlds::Progress {
        stage: crate::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.5),
        detail: "50.0 / 100.0 MB".to_owned(),
    });
    let Some(texts) = screen_texts(&view) else {
        eprintln!(
            "skipping local_world_loading_screen_names_the_stage: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    assert!(
        texts.iter().any(|text| text == "Starting World"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Downloading Bedrock Dedicated Server")
                && text.contains("50.0 / 100.0 MB")),
        "{texts:?}"
    );
}

// Retail desktop settings show vanilla's section set; debug, edu, touch and
// automation sections stay hidden.
#[test]
fn retail_settings_hide_debug_and_automation_sections() {
    let Some(carrier) = super::pack_harness::carrier() else {
        eprintln!(
            "skipping retail_settings_hide_debug_and_automation_sections: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.screen = crate::menu::MenuScreen::Settings;
    let settings = super::menu_screens::screen_data(&view, &|_| None).unwrap();
    let tree = json_ui::resolve(&catalog, settings.reference, &settings.context)
        .control
        .unwrap();
    let mut names = Vec::new();
    let mut stack = vec![&tree];
    while let Some(control) = stack.pop() {
        names.push(control.name.as_str());
        stack.extend(&control.children);
    }
    for shown in [
        "accessibility_button",
        "keyboard_and_mouse_button",
        "controller_button",
        "general_button",
        "video_button",
        "sound_button",
        "account_button",
        "view_subscriptions_button",
        "global_texture_pack_button",
        "storage_management_button",
        "language_button",
        "creator_button",
    ] {
        assert!(names.contains(&shown), "{shown} missing");
    }
    for hidden in [
        "touch_button",
        "switch_controller_button",
        "party_button",
        "preview_button",
        "debug_button",
        "ui_debug_button",
        "edu_debug_button",
        "edu_cloud_storage_button",
        "automation_button",
    ] {
        assert!(!names.contains(&hidden), "{hidden} shown");
    }
}

// Exercise the actual pinned templates: the desktop selector rows are adjacent,
// and hidden advanced video options leave only the panel's normal bottom inset.
#[test]
fn retail_settings_keep_navigation_and_video_options_compact() {
    let Some(carrier) = super::pack_harness::carrier() else {
        eprintln!(
            "skipping retail_settings_keep_navigation_and_video_options_compact: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let catalog = json_ui::Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.screen = crate::menu::MenuScreen::Settings;
    let screen = super::menu_screens::screen_data(&view, &|_| None).unwrap();
    let resolved = json_ui::resolve(&catalog, screen.reference, &screen.context)
        .control
        .unwrap();
    let bound = json_ui::bind(
        &resolved,
        &screen.data,
        &json_ui::CatalogLibrary {
            catalog: &catalog,
            context: &screen.context,
        },
    );
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    fn rect(node: &json_ui::LaidOut<'_>, name: &str) -> Option<json_ui::Rect> {
        if !node.visible {
            return None;
        }
        if node.control.name == name {
            return Some(node.rect);
        }
        node.children.iter().find_map(|child| rect(child, name))
    }
    for size in [[480.0, 270.0], [3840.0 / 7.0, 2560.0 / 7.0]] {
        let layout = json_ui::layout(&bound, size, &env);
        for (above, below) in [
            ("keyboard_and_mouse_button", "controller_button"),
            ("general_button", "video_button"),
            ("video_button", "sound_button"),
            ("sound_button", "account_button"),
        ] {
            let upper = rect(&layout, above).expect(above);
            let lower = rect(&layout, below).expect(below);
            let gap = lower.y - upper.y - upper.h;
            assert!((0.0..=2.0).contains(&gap), "{above} to {below}: {gap}");
        }
        let video = layout_section(&layout, "video_section").unwrap();
        for (above, below) in [
            ("graphics_mode", "advanced_graphics_options_button"),
            ("advanced_graphics_options_button", "render_distance_slider"),
            ("render_distance_slider", "brightness_slider"),
        ] {
            let upper = rect(video, above).expect(above);
            let lower = rect(video, below).expect(below);
            let gap = lower.y - upper.y - upper.h;
            assert!((0.0..=8.0).contains(&gap), "{above} to {below}: {gap}");
        }
    }
}

fn layout_section<'a, 'b>(
    node: &'b json_ui::LaidOut<'a>,
    name: &str,
) -> Option<&'b json_ui::LaidOut<'a>> {
    if node.control.name == name {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| layout_section(child, name))
}

// The Servers tab builds only the saved rows its list shows, however long the list.
#[test]
fn the_server_list_builds_only_visible_rows() {
    let player_runtime = player_state::PlayerState::new(1);

    let drawn = |count: usize| {
        let mut presentation = mini_engine_presentation();
        let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
        view.screen = crate::menu::MenuScreen::Servers;
        view.servers = (0..count)
            .map(|index| crate::menu::SavedServer {
                name: format!("Server {index}"),
                address: format!("10.0.0.{}:19132", index % 250),
                favorite: false,
                last_joined_unix: 0,
            })
            .collect();
        presentation.set_menu_view(Some(view));
        let input = presentation
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let saved = presentation
            .menu_hit_targets
            .iter()
            .filter(|(action, _)| matches!(action, crate::menu::MenuAction::SelectSaved(_)))
            .count();
        (input.vertices.len(), saved)
    };
    let (short, short_rows) = drawn(40);
    let (long, long_rows) = drawn(300);
    assert!(short_rows > 0 && short_rows < 40, "{short_rows}");
    assert_eq!(long_rows, short_rows);
    // Only the list's count label grows a digit.
    assert!(long < short + 64, "{long} vs {short}");
}

/// The OreUI screen `view` draws, as its text runs.
fn oreui_texts(view: &crate::menu::MenuView) -> Vec<String> {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let metrics = super::super::TextMetrics::for_viewport([1600, 900], dpi, None);
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation
        .append_oreui_screen(view, &mut nodes, &mut next, metrics, [1600.0, 900.0], None)
        .unwrap()
        .expect("an OreUI screen");
    super::pack_harness::drawn_texts(&nodes)
}

// The owner's world type labels show on the create form, the edit screen, the worlds list
// and the no-Docker dialog's built-in option.
#[test]
fn world_types_carry_the_owner_labels_everywhere_they_show() {
    use crate::local_worlds::{
        Event, FLAT_WORLD_LABEL, Input, NORMAL_WORLD_LABEL, Tab, WorldsMenu,
    };
    use protocol::world_control::{
        Backend, Difficulty, GameMode, Generator, Prefs, Setup, SetupState, UnavailableReason,
        World, WorldState, WorldStatus,
    };
    let mut runtime = crate::menu::MenuView::new(true, "Steve".to_owned());
    runtime.screen = crate::menu::MenuScreen::Play;
    let flat = World {
        id: "0123456789abcdef".to_owned(),
        name: "Plains".to_owned(),
        game_mode: GameMode::Creative,
        generator: Generator::Flat,
        difficulty: Difficulty::Easy,
        backend: Backend::Dragonfly,
        seed: 1,
        created_unix: 1,
        last_played_unix: 1,
        size_bytes: 0,
    };
    let base = |menu: &WorldsMenu| {
        let mut view = runtime.clone();
        view.local = menu.view();
        view.local_worlds = vec![crate::menu::LocalWorldCard {
            name: flat.name.clone(),
            game_mode: "Creative".to_owned(),
            world_type: crate::local_worlds::world_type_label(flat.generator).to_owned(),
            date: String::new(),
            size: String::new(),
        }];
        view
    };
    let has = |texts: &[String], wanted: &str| texts.iter().any(|text| text.contains(wanted));
    let mut menu = WorldsMenu::default();
    menu.update(Input::Refresh);
    menu.apply(Event::Listed(vec![flat.clone()]));
    let list = oreui_texts(&base(&menu));
    assert!(has(&list, FLAT_WORLD_LABEL), "worlds list: {list:?}");

    menu.update(Input::BeginCreate);
    menu.update(Input::SelectTab(Tab::Advanced));
    let create = oreui_texts(&base(&menu));
    for label in [NORMAL_WORLD_LABEL, FLAT_WORLD_LABEL] {
        assert!(has(&create, label), "create form {label}: {create:?}");
    }
    menu.update(Input::Back);

    menu.update(Input::BeginEdit(0));
    let edit = oreui_texts(&base(&menu));
    assert!(has(&edit, FLAT_WORLD_LABEL), "edit screen: {edit:?}");
    menu.update(Input::Back);

    let setup = Setup {
        state: SetupState::Unsupported,
        version: None,
        bytes_done: 0,
        bytes_total: 0,
        layers_done: 0,
        layers_total: 0,
        eula_accepted: false,
        error: None,
        runtime: "none".to_owned(),
        reason: None,
    };
    menu.apply(Event::Prefs(
        Prefs::default(),
        WorldStatus {
            state: WorldState::Idle,
            world_id: None,
            backend: None,
            paused: false,
            pause_supported: true,
            error: None,
            setup: Some(setup),
            backend_unavailable_reason: Some(UnavailableReason::DockerMissing),
        },
    ));
    menu.update(Input::BeginCreate);
    let dialog = oreui_texts(&base(&menu));
    assert!(
        has(&dialog, &format!("Create {FLAT_WORLD_LABEL} world")),
        "no-Docker dialog: {dialog:?}"
    );
}

#[test]
fn fallback_form_preserves_host_owned_presentation_state() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let runtime = super::pack_harness::action_form(&mut player_runtime, "Menu", &["A"]);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .set_experience_chrome(Some("Trusted status"), false)
        .unwrap();
    presentation.set_mod_label(Some("Extension label")).unwrap();
    super::pack_harness::render(&mut presentation, &runtime, [1280, 720], 1.0);
    assert!(presentation.form_presentation.experience.is_some());
    assert!(presentation.form_presentation.mod_hud.is_some());
}

#[test]
fn review_failed_menu_modal_does_not_expose_underlying_actions() {
    use crate::menu::{MenuDialog, MenuScreen};
    let player_runtime = player_state::PlayerState::new(1);
    let mut presentation = mini_engine_presentation();
    let mut view = crate::menu::MenuView::new(true, "Test".into());
    view.screen = MenuScreen::Play;
    presentation.set_menu_view(Some(view.clone()));
    presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(!presentation.menu_hit_targets.is_empty());
    view.dialog = Some(MenuDialog::Exit);
    presentation.set_menu_view(Some(view));
    presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(presentation.menu_hit_targets.is_empty());
}

#[test]
fn paper_doll_keeps_vanilla_placement_under_the_java_hud_overlay() {
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let mut catalog = json_ui::Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let context = json_ui::hud_context(&json_ui::Context::default());
    let env = json_ui::LayoutEnv {
        text: &FixedText,
        textures: &NoTextures,
    };
    for java in [false, true] {
        if java {
            for (path, _, bytes) in super::hud::JAVA_HUD_PACK {
                let text = std::str::from_utf8(bytes).unwrap();
                if path.ends_with("_global_variables.json") {
                    catalog.overlay_globals_text(text);
                } else {
                    catalog.overlay_text(path, text);
                }
            }
        }
        for visible in [true, false] {
            let data = json_ui::hud_data_source(&json_ui::HudModel {
                paper_doll: visible,
                hotbar_visible: true,
                ..Default::default()
            });
            let render = json_ui::render_screen(
                json_ui::HUD_SCREEN,
                &catalog,
                &context,
                &data,
                [640., 360.],
                &env,
                &Default::default(),
            )
            .unwrap();
            let dolls: Vec<_> = render.nodes.iter().filter(|node| matches!(&node.draw, json_ui::Draw::Custom { renderer, .. } if renderer == "hud_player_renderer")).collect();
            assert_eq!(dolls.len(), usize::from(visible), "java={java}");
            if visible {
                assert_eq!(
                    dolls[0].dest,
                    json_ui::RectOut {
                        x: 15.,
                        y: 15.,
                        w: 15.,
                        h: 15.
                    }
                );
            }
        }
    }
}

// The join's trust question draws vanilla's modal popup over everything, and only its answers take
// presses, even with a launcher dialog open beneath it.
#[test]
fn server_trust_question_draws_the_vanilla_popup_and_owns_the_input() {
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping server_trust_question_draws_the_vanilla_popup_and_owns_the_input: missing local UI carrier; make assets"
        );
        return;
    };
    let player_runtime = player_state::PlayerState::new(1);
    let mut view = crate::menu::MenuView::new(true, "Player".into());
    view.connecting = true;
    view.dialog = Some(crate::menu::MenuDialog::Exit);
    view.feeds.server_trust = Some(crate::menu::ServerTrustPrompt {
        id: 1,
        url: "http://127.0.0.1:19132".into(),
        from_session_core: false,
    });
    let actions = super::test_support::draw_menu_actions(&player_runtime, &mut presentation, &view);
    let texts =
        super::pack_harness::drawn_texts(super::pack_harness::menu_nodes(&presentation)).join(" ");
    for expected in [
        "Trust this server?",
        "You are connecting to",
        "http://127.0.0.1:19132",
        "Trust and Join",
        "Don't Trust",
    ] {
        assert!(
            texts.contains(expected),
            "missing {expected:?} in {texts:?}"
        );
    }
    for answer in [true, false] {
        assert!(
            actions.contains(&crate::menu::MenuAction::ServerTrust(answer)),
            "{actions:?}"
        );
    }
    assert!(
        actions
            .iter()
            .all(|action| matches!(action, crate::menu::MenuAction::ServerTrust(_))),
        "{actions:?}"
    );
}

// A language's translation of the question wins over vanilla's English and names the URL.
#[test]
fn server_trust_question_reads_the_active_language() {
    let translate = |key: &str| {
        (key == "permissions.servertrust.message").then(|| Arc::<str>::from("Vertrauen %1$s?"))
    };
    let json_ui::FormModel::Modal(modal) =
        super::menu_screens::server_trust_model("http://a:1", &translate)
    else {
        panic!("the trust question is a modal popup");
    };
    assert_eq!(
        (
            modal.title.as_str(),
            modal.body.as_str(),
            modal.button1.as_str()
        ),
        (
            "Trust this server?",
            "Vertrauen http://a:1?",
            "Trust and Join"
        )
    );
}
