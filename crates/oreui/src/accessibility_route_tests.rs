//! Input and non-interactive status remain usable when the new UI is enlarged.

use super::*;

#[test]
fn extra_large_servers_keeps_scrolled_group_drag_inside_small_windows() {
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::Servers;
                view.servers = vec![launcher::menu::SavedServer {
                    name: "Fixture server".into(),
                    address: "fixture.test:19132".into(),
                    favorite: false,
                    last_joined_unix: 0,
                }]
                .into();
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let action = MenuAction::ServerList(ServerListAction::Toggle(ServerGroup::Saved));
                let mut header = None;
                for _ in 0..64 {
                    let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                    header = frame
                        .hits
                        .iter()
                        .find_map(|(at, bounds)| (*at == action).then_some(*bounds));
                    if header.is_some() {
                        break;
                    }
                    let areas = screen.take_paint().unwrap().scrolls;
                    let area = areas
                        .iter()
                        .find(|area| area.key == "servers.side_menu")
                        .unwrap();
                    let point = crate::test_support::centre(area.viewport);
                    let delta = -area.viewport.height() * 0.5;
                    scrolls.set_areas(areas);
                    assert!(scrolls.wheel(point, delta, true));
                }
                let header =
                    header.expect("a server group can be reached through actual scrolling");
                let point = crate::test_support::centre(header);
                assert!(
                    screen
                        .server_list_pointer(
                            Some(MenuScreen::Servers),
                            Some(point),
                            (true, true),
                            0.0
                        )
                        .0
                );
                let moved = UiPoint::new(point.x() + 8.0, point.y()).unwrap();
                assert!(
                    screen
                        .server_list_pointer(
                            Some(MenuScreen::Servers),
                            Some(moved),
                            (true, false),
                            0.02
                        )
                        .0
                );
                paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                let preview = screen
                    .transitions
                    .server_list
                    .preview()
                    .expect("held group has a drag preview");
                let areas = screen.take_paint().unwrap().scrolls;
                let area = areas
                    .iter()
                    .find(|area| area.key == "servers.side_menu")
                    .unwrap();
                assert!(preview.bounds[1] >= area.viewport.min().y() - 0.01);
                assert!(preview.bounds[3] <= area.viewport.max().y() + 0.01);
                assert!(
                    screen
                        .server_list_pointer(
                            Some(MenuScreen::Servers),
                            Some(moved),
                            (false, false),
                            0.04
                        )
                        .0
                );
                assert!(!screen.transitions.server_list.captured());
            }
        }
    }
}

#[test]
fn extra_large_realm_invite_keeps_edit_submit_and_back_reachable() {
    use launcher::menu::realm_membership::{Action, State};
    for size in [
        [400.0, 150.0],
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [400.0, 250.0],
        [752.0, 500.0],
    ] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::Social;
                view.realm_membership = Some(State {
                    code: "fixture-code".into(),
                    ..Default::default()
                });
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let frame = paint(&mut OreUiScreen::default(), &view, metrics, size);
                for action in [Action::EditCode, Action::Verify, Action::Back] {
                    let bounds = frame
                        .hits
                        .iter()
                        .find_map(|(at, bounds)| {
                            (*at == MenuAction::RealmMembership(action)).then_some(*bounds)
                        })
                        .expect("invite navigation remains available");
                    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                    assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                    assert!(
                        bounds.max().x() <= size[0] && bounds.max().y() <= size[1],
                        "{action:?}: {bounds:?} at {size:?}, DPI {dpi}, enabled {enabled}"
                    );
                }
            }
        }
    }
}

/// Paints the loading adapter without scroll input, as the client does outside menus.
fn loading_nodes(size: [f32; 2], dpi: f32, enabled: i32) -> Vec<UiNode> {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(64, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let artwork = HashMap::new();
    let metrics = TextMetrics::for_viewport(
        size.map(|value| (value * dpi) as u32),
        DpiScale::new(dpi).unwrap(),
        Some(2),
    );
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: size,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    let mut settings = SettingsOptions::default();
    settings.set(option_index(EXTRA_LARGE_UI), enabled);
    let mut screen = OreUiScreen::default();
    screen.configure_accessibility(&settings);
    screen
        .paint_loading(
            crate::loading_screen::LoadingInput {
                stage: ui::LoadingStage::Connecting,
                words: ["Connecting", "Loading world"],
                size,
                title_artwork: None,
                destination_icon: None,
                artwork: &artwork,
                dimension: 0,
            },
            &mut frame,
        )
        .unwrap();
    assert_eq!(frame.metrics.gui_scale, metrics.gui_scale);
    nodes
}

#[test]
fn extra_large_noninteractive_loading_keeps_title_and_details_visible() {
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                let nodes = loading_nodes(size, dpi, enabled);
                for text in ["CONNECTING", "Loading world"] {
                    let node = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. } if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == text)).expect("loading status is painted");
                    let mut bounds = [
                        node.bounds().min().x(),
                        node.bounds().min().y(),
                        node.bounds().max().x(),
                        node.bounds().max().y(),
                    ];
                    let mut parent = node.parent();
                    while let Some(ancestor) =
                        parent.and_then(|id| nodes.iter().find(|node| node.id() == id))
                    {
                        if ancestor.clips_children() {
                            assert!(
                                bounds[0] >= -0.01
                                    && bounds[1] >= -0.01
                                    && bounds[2] <= ancestor.bounds().width() + 0.01
                                    && bounds[3] <= ancestor.bounds().height() + 0.01,
                                "{text} is clipped at {size:?}, DPI {dpi}, enabled {enabled}: {bounds:?} in {:?}",
                                ancestor.bounds()
                            );
                        }
                        let min = ancestor.bounds().min();
                        bounds = [
                            bounds[0] + min.x(),
                            bounds[1] + min.y(),
                            bounds[2] + min.x(),
                            bounds[3] + min.y(),
                        ];
                        parent = ancestor.parent();
                    }
                    assert!(
                        bounds[0] >= -0.01
                            && bounds[1] >= -0.01
                            && bounds[2] <= size[0] + 0.01
                            && bounds[3] <= size[1] + 0.01
                    );
                }
            }
        }
    }
}

/// Narrow, enlarged settings must reflow the selected shader card instead of pushing
/// its controls past the panel, where scrolling can't reach them.
#[test]
fn extra_large_shader_library_keeps_every_control_inside_the_settings_panel() {
    use launcher::menu::{
        RendererChoice,
        settings_sections::SettingSection,
        settings_storage::{StorageAction, StorageFolder},
    };
    if !render_model::SHADER_PACKS_ENABLED {
        return;
    }
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [240.0, 720.0], [400.0, 250.0]] {
        for enabled in [0, 1] {
            let mut view =
                crate::test_support::settings_view(SettingSection::Shaders.definition().name);
            view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
            view.render_mode = ui::RenderMode::ShaderPack;
            view.shader_pack = Some("Dusk.zip".into());
            let selected = RendererChoice::ShaderPack(0, view.shader_packs_revision);
            view.renderer = Some(selected);
            view.shader_options_available = true;
            view.renderer_failure = Some("Pack failed: unknown bloomTint".into());
            Arc::make_mut(&mut view.storage).opens_folders = true;
            Arc::make_mut(&mut view.settings_options).set(option_index(EXTRA_LARGE_UI), enabled);
            let metrics = TextMetrics::for_viewport(
                size.map(|value| value as u32),
                DpiScale::new(1.0).unwrap(),
                Some(2),
            );
            let mut screen = OreUiScreen::default();
            paint_with_scrolls(
                &mut screen,
                &view,
                metrics,
                size,
                &mut MenuScrolls::default(),
            );
            let paint = screen.take_paint().unwrap();
            let panel = paint
                .scrolls
                .iter()
                .find(|area| area.key.starts_with("oreui_settings/"))
                .expect("settings content scroll view")
                .viewport;
            for action in [
                MenuAction::SelectRenderer(RendererChoice::Vanilla),
                MenuAction::SelectRenderer(selected),
                MenuAction::SelectRenderer(RendererChoice::ShaderPack(
                    1,
                    view.shader_packs_revision,
                )),
                MenuAction::RemoveShaderPackDialog(0, view.shader_packs_revision),
                MenuAction::ShaderOptions(launcher::shader_options::Action::Open),
                MenuAction::ReloadShaders,
                MenuAction::SettingsStorage(StorageAction::OpenFolder(StorageFolder::ShaderPacks)),
            ] {
                let bounds = paint
                    .geometry
                    .focus_targets
                    .iter()
                    .find(|target| target.action == action)
                    .unwrap_or_else(|| panic!("{size:?} xl={enabled}: {action:?} is focusable"))
                    .bounds;
                assert!(bounds.width() > 0.0, "{size:?} xl={enabled}: {action:?}");
                assert!(
                    bounds.min().x() >= panel.min().x() && bounds.max().x() <= panel.max().x(),
                    "{size:?} xl={enabled}: {action:?} at {bounds:?} leaves {panel:?}"
                );
            }
        }
    }
}
