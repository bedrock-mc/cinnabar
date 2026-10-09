use super::*;
use crate::menu::split_address;
use json_ui::RectOut;

#[test]
fn reconnect_regions_require_a_retryable_failure() {
    let mut shown = view(MenuScreen::Play);
    let button = region(HitKind::Button, Some("button.cinnabar_reconnect"));
    assert_eq!(action_for(&shown, &button), None);
    shown.disconnect_message = Some("network session failed: closed".into());
    assert_eq!(action_for(&shown, &button), None);
    shown.can_reconnect = true;
    assert_eq!(action_for(&shown, &button), Some(MenuAction::Reconnect));
}

fn view(screen: MenuScreen) -> MenuView {
    let mut view = crate::menu::MenuView::new(true, "Steve".to_owned());
    view.screen = screen;
    view.auth_state = AuthState::SignedOut;
    view
}

fn region(kind: HitKind, pressed: Option<&str>) -> HitRegion {
    let rect = RectOut {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    HitRegion {
        key: "/screen/button".to_owned(),
        name: "button".to_owned(),
        kind,
        rect,
        clip: rect,
        layer: 0,
        order: 0,
        pressed: pressed.map(str::to_owned),
        control_name: None,
        collection_index: None,
        collection: None,
        enabled: true,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        input: Default::default(),
        focus: None,
        widget: Default::default(),
        collections: Vec::new(),
        modal_root: None,
    }
}

fn reference(view: &MenuView) -> Option<&'static str> {
    screen_data(view, &|_| None).map(|screen| screen.reference)
}

#[test]
fn the_pause_store_button_names_the_server_store() {
    assert_eq!(server_store_text(&|_| None), "Server Store");
}

#[test]
fn dressing_room_buttons_open_the_skin_library_and_preserve_profile_routes() {
    for screen in [MenuScreen::Home, MenuScreen::Pause] {
        let shown = view(screen);
        assert_eq!(
            action_for(
                &shown,
                &region(HitKind::Button, Some("button.to_profile_screen"))
            ),
            Some(MenuAction::Navigate(MenuScreen::DressingRoom))
        );
        assert_eq!(
            action_for(
                &shown,
                &region(HitKind::Button, Some("button.manage_account"))
            ),
            Some(MenuAction::Navigate(MenuScreen::Profile))
        );
    }
}

#[test]
fn the_version_reads_as_the_release_client_shows_it() {
    assert_eq!(version_label("1.26.50"), "v26.50");
    assert_eq!(version_label("26.60"), "v26.60");
}

#[test]
fn menu_states_open_their_vanilla_screens() {
    assert_eq!(
        reference(&view(MenuScreen::Pause)),
        Some("pause.pause_screen")
    );
    assert_eq!(
        reference(&view(MenuScreen::Home)),
        Some("start.start_screen")
    );
    assert_eq!(
        reference(&view(MenuScreen::Death)),
        Some("death.death_screen")
    );
    assert_eq!(
        reference(&view(MenuScreen::Servers)),
        Some("play.play_screen")
    );
    let mut connecting = view(MenuScreen::Play);
    connecting.connecting = true;
    assert_eq!(
        reference(&connecting),
        Some("progress.world_loading_progress_screen")
    );
    connecting.feeds.join = crate::menu::JoinProgress::new(crate::menu::JoinKind::Realm);
    assert_eq!(
        reference(&connecting),
        Some("progress.realms_stories_loading_progress_screen")
    );
    let mut dropped = view(MenuScreen::Play);
    dropped.disconnect_message = Some("Kicked".into());
    assert_eq!(reference(&dropped), Some("disconnect.disconnect_screen"));
    let mut code = view(MenuScreen::Home);
    code.auth_state = AuthState::AwaitingCode {
        uri: "https://x".into(),
        code: "ABC".into(),
    };
    assert_eq!(reference(&code), Some("start.start_screen"));
}

/// A local world's loading screen wins over the plain connecting screen and cancels the open.
#[test]
fn local_world_progress_opens_the_loading_screen_with_cancel() {
    let mut opening = view(MenuScreen::Play);
    opening.connecting = true;
    opening.local.progress = Some(crate::local_worlds::Progress::connecting("Home"));
    assert_eq!(reference(&opening), Some(LOCAL_WORLD_PROGRESS_SCREEN));
    let cancel = action_for(&opening, &region(HitKind::Button, Some("button.menu_exit")));
    assert_eq!(
        cancel,
        Some(MenuAction::LocalWorld(crate::menu::LocalWorldAction::Back))
    );
}

// The bar binds `#loading_bar_percentage` as its `#clip_ratio`, so the share it keeps is the share done.
#[test]
fn local_world_download_bar_fills_with_the_bytes_done() {
    let files = [
        ("ui/_global_variables.json", "{}"),
        (
            "ui/_ui_defs.json",
            r#"{"ui_defs":["ui/progress_screen.json"]}"#,
        ),
        (
            "ui/progress_screen.json",
            r##"{"namespace":"progress","world_convert_modal_progress_screen":{"type":"panel","size":[100,5],"controls":[{"fill":{"type":"image","texture":"textures/ui/experiencebarfull","clip_direction":"left","clip_pixelperfect":false,"bindings":[{"binding_name":"#loading_bar_percentage","binding_name_override":"#clip_ratio"}]}}]}}"##,
        ),
    ];
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|(path, text)| (*path, text.as_bytes())))
            .unwrap();
    let mut opening = view(MenuScreen::Play);
    opening.local.progress = Some(crate::local_worlds::Progress {
        stage: crate::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.882),
        detail: "72.2 / 81.8 MB".to_owned(),
    });
    let screen = screen_data(&opening, &|_| None).unwrap();
    let resolved = json_ui::resolve(&catalog, screen.reference, &screen.context)
        .control
        .unwrap();
    let library = json_ui::CatalogLibrary {
        catalog: &catalog,
        context: &screen.context,
    };
    let bound = json_ui::bind(&resolved, &screen.data, &library);
    let env = json_ui::LayoutEnv {
        text: &super::super::tests::FixedText,
        textures: &super::super::tests::NoTextures,
    };
    let laid = json_ui::layout(&bound, [200.0, 100.0], &env);
    let clipped = laid.children[0].clip_ratio.unwrap_or(0.0);
    assert!((1.0 - clipped - 0.882).abs() < 1e-3, "clipped {clipped}");
}

#[test]
fn pressed_buttons_map_to_menu_actions() {
    let pause = view(MenuScreen::Pause);
    let press = |view: &MenuView, id: &str| action_for(view, &region(HitKind::Button, Some(id)));
    assert_eq!(
        press(&pause, "button.menu_continue"),
        Some(MenuAction::PauseResume)
    );
    assert_eq!(
        press(&pause, "button.menu_quit"),
        Some(MenuAction::PauseDisconnect)
    );
    let death = view(MenuScreen::Death);
    assert_eq!(
        press(&death, "button.main_menu_button"),
        Some(MenuAction::OpenDeathQuit)
    );
    assert_eq!(
        press(&death, "button.respawn_button"),
        Some(MenuAction::Respawn)
    );
    let home = view(MenuScreen::Home);
    assert_eq!(
        press(&home, "button.menu_exit"),
        Some(MenuAction::OpenExitDialog)
    );
    let mut edit = region(
        HitKind::Button,
        Some("button.menu_network_server_world_edit"),
    );
    edit.collection_index = Some(3);
    assert_eq!(
        action_for(&view(MenuScreen::Servers), &edit),
        Some(MenuAction::EditSaved(3))
    );
}

#[test]
fn the_pause_friends_drawer_invites_only_while_hosting() {
    let press = |view: &MenuView| {
        action_for(
            view,
            &region(HitKind::Button, Some("button.friends_drawer")),
        )
    };
    let mut pause = view(MenuScreen::Pause);
    assert_eq!(
        press(&pause),
        Some(MenuAction::Navigate(MenuScreen::Friends))
    );
    pause.hosting = true;
    assert_eq!(
        press(&pause),
        Some(MenuAction::Invite(launcher::menu::invite::Action::Open))
    );
    let mut home = view(MenuScreen::Home);
    home.hosting = true;
    assert_eq!(
        press(&home),
        Some(MenuAction::Navigate(MenuScreen::Friends))
    );
}

#[test]
fn the_start_screen_marketplace_button_opens_the_store_and_its_presses_route_to_it() {
    let home = view(MenuScreen::Home);
    assert_eq!(
        action_for(&home, &region(HitKind::Button, Some("button.menu_store"))),
        Some(MenuAction::Store(crate::store::StoreAction::Open))
    );
    let mut store = view(MenuScreen::Store);
    assert!(
        reference(&store).is_none(),
        "no engine screen until the store publishes"
    );
    store.store = Some(std::sync::Arc::new(crate::store::StoreSnapshot::empty()));
    assert_eq!(
        reference(&store),
        Some("store_layout.store_data_driven_screen")
    );
    assert_eq!(
        action_for(&store, &region(HitKind::Button, Some("button.menu_exit"))),
        Some(MenuAction::Store(crate::store::StoreAction::Back))
    );
}

#[test]
fn radio_tabs_pick_play_tabs_and_settings_sections() {
    let mut tab = region(HitKind::Toggle, None);
    tab.control_name = Some("navigation_tab".into());
    tab.group_index = Some(1);
    assert_eq!(
        action_for(&view(MenuScreen::Play), &tab),
        Some(MenuAction::Navigate(MenuScreen::Social))
    );
    tab.group_index = Some(8);
    assert_eq!(
        action_for(&view(MenuScreen::Settings), &tab),
        Some(MenuAction::SettingsSection(8))
    );
}

#[test]
fn fullscreen_toggle_binds_and_changes_the_current_window_mode() {
    let mut view = view(MenuScreen::Settings);
    view.fullscreen = false;
    let mut toggle = region(HitKind::Toggle, None);
    toggle.control_name = Some("#full_screen".into());
    assert_eq!(
        action_for(&view, &toggle),
        Some(MenuAction::SettingsFullscreen(true))
    );
    view.fullscreen = true;
    assert_eq!(
        action_for(&view, &toggle),
        Some(MenuAction::SettingsFullscreen(false))
    );
    let data = screen_data(&view, &|_| None).unwrap().data;
    let toggle = json_ui::ResolvedControl {
        name: "full_screen".into(),
        control_type: Some("toggle".into()),
        base: None,
        unresolved_base: None,
        properties: serde_json::json!({"bindings": [
            {"binding_name": "#full_screen", "binding_name_override": "#toggle_state"},
            {"binding_name": "#full_screen_enabled", "binding_name_override": "#enabled"}
        ]})
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect(),
        children: Vec::new(),
        factory: None,
    };
    let bound = json_ui::bind(&toggle, &data, &json_ui::EmptyLibrary);
    assert_eq!(
        bound.properties.get("#toggle_state"),
        Some(&Value::Bool(true))
    );
    assert_eq!(bound.properties.get("#enabled"), Some(&Value::Bool(true)));
}

#[test]
fn sliders_split_into_their_settings_values() {
    let mut view = crate::menu::MenuView::new(true, "Player".to_owned());
    view.gui_scale_choices = ui::DesktopGuiScale::for_window([1920, 1080])
        .choices()
        .collect();
    let mut slider = region(HitKind::Slider, None);
    slider.control_name = Some("gui_scale".to_owned());
    let scale = slider_actions(&view, &slider).unwrap();
    assert_eq!(
        scale,
        vec![
            MenuAction::SettingsScale(-2),
            MenuAction::SettingsScale(-1),
            MenuAction::SettingsScale(0)
        ]
    );
    for (index, option) in crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .enumerate()
    {
        if !matches!(
            option.kind,
            crate::menu::settings_options::SettingKind::Slider
        ) {
            continue;
        }
        slider.control_name = Some(option.name.to_owned());
        let actions = slider_actions(&view, &slider).unwrap();
        assert_eq!(
            actions.first(),
            Some(&MenuAction::SettingsOption(index as u16, option.min))
        );
        assert_eq!(
            actions.last(),
            Some(&MenuAction::SettingsOption(index as u16, option.max))
        );
    }
}

#[test]
fn addresses_split_into_the_ip_and_port_boxes() {
    assert_eq!(
        split_address("play.example:19133"),
        ("play.example".into(), "19133".into())
    );
    assert_eq!(split_address("[::1]:19132"), ("::1".into(), "19132".into()));
    assert_eq!(split_address("host"), ("host".into(), "19132".into()));
}

#[test]
fn antialiasing_slider_uses_device_sample_stops_and_numeric_labels() {
    let mut view = view(MenuScreen::Settings);
    let options = Arc::make_mut(&mut view.settings_options);
    options.set_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4, 8]));
    let index = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "msaa")
        .unwrap();
    options.set(index, 4);
    let mut slider = region(HitKind::Slider, None);
    slider.control_name = Some("msaa".into());
    assert_eq!(
        slider_actions(&view, &slider).unwrap(),
        [1, 4, 8].map(|samples| MenuAction::SettingsOption(index as u16, samples))
    );
    let mut actual = DataSource::default();
    super::super::settings_controls::bind(&view, &mut actual, &str::to_owned);
    let mut expected = actual.clone();
    expected.set_global("#msaa", Scalar::Num(1.0));
    expected.set_global("#msaa_steps", Scalar::Num(3.0));
    expected.set_global("#msaa_text_value", Scalar::Text("4".into()));
    expected.set_global("#msaa_slider_label", Scalar::Text("options.msaa: 4".into()));
    expected.set_global("#show_msaa", Scalar::Bool(true));
    assert_eq!(actual, expected);
}

#[test]
fn motion_blur_dropdown_binds_saved_presets_and_edits_the_registered_option() {
    use crate::menu::settings_options::{
        MOTION_BLUR_CHOICES, MOTION_BLUR_OPTION, SETTINGS_OPTIONS,
    };
    let mut view = view(MenuScreen::Settings);
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == MOTION_BLUR_OPTION.name)
        .unwrap();
    for preset in ui::MotionBlurQuality::ALL {
        Arc::make_mut(&mut view.settings_options).set(index, preset.index());
        let mut actual = DataSource::default();
        super::super::settings_controls::bind(&view, &mut actual, &str::to_owned);
        let mut expected = actual.clone();
        expected.set_global(
            format!("#{}_dropdown_toggle_label", MOTION_BLUR_OPTION.name),
            Scalar::Text(preset.label().into()),
        );
        for (choice_index, choice) in MOTION_BLUR_CHOICES.iter().enumerate() {
            expected.set_global(
                format!("#{}", choice.name),
                Scalar::Bool(choice_index == preset.index() as usize),
            );
            let mut radio = region(HitKind::Toggle, None);
            radio.control_name = Some(choice.name.into());
            assert_eq!(
                super::super::settings_controls::action(&view, &radio),
                Some(MenuAction::SettingsOption(
                    index as u16,
                    choice_index as i32
                ))
            );
        }
        assert_eq!(actual, expected);
    }
}
