use super::*;
use launcher::dressing_room::SkinModel;

fn wait(
    menu: &mut MenuRuntime,
    local: &mut crate::player_skin::LocalPlayerSkin,
    world: &mut ClientWorld,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while menu.dressing_room.busy {
        menu.poll_dressing_room(Some(&mut *local), world, None, 0);
        assert!(
            std::time::Instant::now() < deadline,
            "skin worker did not publish"
        );
        std::thread::yield_now();
    }
}

#[test]
fn dressing_room_keeps_pause_history_and_selected_skin_login_state() {
    let mut menu = MenuRuntime::new(false, 2, "fixture".to_owned());
    let mut local = menu.player_skin.clone();
    let mut world = ClientWorld::default();
    menu.open_pause();
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
    assert_eq!(menu.screen(), MenuScreen::DressingRoom);
    assert!(!menu.uses_panorama());
    wait(&mut menu, &mut local, &mut world);
    let source = menu.layout.user_data_root.join("import.png");
    fs::create_dir_all(&menu.layout.user_data_root).unwrap();
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba([5, 150, 220, 255]),
    )
    .save(&source)
    .unwrap();
    menu.import_skin_path(source);
    wait(&mut menu, &mut local, &mut world);
    let view = menu.view();
    let entry = view.dressing_room.selected_skin().unwrap();
    assert!(entry.imported);
    assert_eq!(entry.model, SkinModel::Classic);
    assert_eq!(view.player_skin.as_ref(), Some(&local.standard_skin()));
    assert_eq!(
        menu.player_skin().to_client_skin().rgba8,
        local.rgba8.as_ref()
    );
    assert!(
        menu.focus_actions()
            .contains(&MenuAction::DressingRoom(Action::SetModel(SkinModel::Slim)))
    );
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.screen(), MenuScreen::Pause);
    assert_eq!(
        crate::player_skin::LocalPlayerSkin::load(&menu.layout, "fixture").standard_skin(),
        local.standard_skin()
    );
}

#[test]
fn skin_editor_traps_input_and_saves_the_same_bounded_draft() {
    let mut menu = MenuRuntime::new(false, 2, "skin editor".to_owned());
    let mut local = menu.player_skin.clone();
    let mut world = ClientWorld::default();
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
    wait(&mut menu, &mut local, &mut world);
    let source = menu.layout.user_data_root.join("rename.png");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba([50, 190, 240, 255]),
    )
    .save(&source)
    .unwrap();
    menu.import_skin_path(source.clone());
    wait(&mut menu, &mut local, &mut world);
    let index = menu.dressing_room.selected.unwrap();
    menu.activate(MenuAction::DressingRoom(Action::BeginRename(index)));
    assert!(menu.view().popup_open());
    assert_eq!(menu.field, Some(MenuField::SkinName));
    assert_eq!(
        menu.focus_actions(),
        vec![
            MenuAction::DressingRoom(Action::Cancel),
            MenuAction::EditSkinName,
            MenuAction::DressingRoom(Action::SaveRename)
        ]
    );
    menu.skin_name.move_home(false);
    menu.skin_name.move_end(true);
    menu.edit_text(&"é".repeat(launcher::dressing_room::MAX_SKIN_NAME_BYTES));
    let draft = menu.dressing_room.editor.as_ref().unwrap().draft.clone();
    assert!(!draft.is_empty() && draft.len() <= launcher::dressing_room::MAX_SKIN_NAME_BYTES);
    assert_eq!(draft, menu.skin_name.as_str());
    menu.activate(MenuAction::DressingRoom(Action::Select(0)));
    menu.activate(MenuAction::Navigate(MenuScreen::Home));
    assert_eq!(menu.dressing_room.selected, Some(index));
    assert_eq!(menu.screen(), MenuScreen::DressingRoom);
    let pixels = local.standard_skin();
    menu.activate(MenuAction::DressingRoom(Action::SaveRename));
    wait(&mut menu, &mut local, &mut world);
    assert!(!menu.view().popup_open());
    assert_eq!(menu.field, None);
    assert_eq!(menu.dressing_room.selected_skin().unwrap().name, draft);
    assert_eq!(local.standard_skin(), pixels);
    menu.activate(MenuAction::DressingRoom(Action::BeginDelete(index)));
    assert_eq!(
        menu.focus_actions(),
        vec![
            MenuAction::DressingRoom(Action::Cancel),
            MenuAction::DressingRoom(Action::ConfirmDelete)
        ]
    );
    menu.activate(MenuAction::AddBack);
    assert!(!menu.view().popup_open());
    assert_eq!(menu.screen(), MenuScreen::DressingRoom);
    assert_eq!(menu.dressing_room.selected, Some(index));
    menu.activate(MenuAction::DressingRoom(Action::Select(0)));
    wait(&mut menu, &mut local, &mut world);
    menu.activate(MenuAction::DressingRoom(Action::BeginDelete(index)));
    menu.activate(MenuAction::DressingRoom(Action::ConfirmDelete));
    wait(&mut menu, &mut local, &mut world);
    assert!(menu.dressing_room.skins.iter().all(|entry| !entry.imported));
    assert!(source.is_file());
}

#[test]
fn cape_section_selection_and_editor_target_preserve_active_skin() {
    use launcher::dressing_room::{DressingRoomSection, SkinEditorTarget};
    let mut menu = MenuRuntime::new(false, 2, "cape editor".to_owned());
    let mut local = menu.player_skin.clone();
    let mut world = ClientWorld::default();
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
    wait(&mut menu, &mut local, &mut world);
    let body = local.rgba8.clone();
    let source = menu.layout.user_data_root.join("cape.png");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let size = protocol::CAPE_DIMENSIONS[0];
    image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([50, 90, 240, 255]))
        .save(&source)
        .unwrap();
    menu.activate(MenuAction::DressingRoom(Action::SetSection(
        DressingRoomSection::Capes,
    )));
    assert!(
        menu.focus_actions()
            .contains(&MenuAction::DressingRoom(Action::ImportCape))
    );
    assert!(
        !menu
            .focus_actions()
            .contains(&MenuAction::DressingRoom(Action::Import))
    );
    menu.import_cape_path(source);
    wait(&mut menu, &mut local, &mut world);
    assert_eq!(menu.dressing_room.section, DressingRoomSection::Capes);
    let index = menu.dressing_room.selected_cape.unwrap();
    assert_eq!(menu.view().player_skin.as_ref().unwrap().cape, local.cape);
    assert_eq!(local.rgba8, body);
    assert!(local.to_client_skin().cape.is_some());
    menu.activate(MenuAction::DressingRoom(Action::BeginRenameCape(index)));
    assert_eq!(
        menu.dressing_room.editor.as_ref().unwrap().target,
        SkinEditorTarget::Cape
    );
    menu.skin_name.move_home(false);
    menu.skin_name.move_end(true);
    menu.edit_text("Renamed cape");
    menu.activate(MenuAction::DressingRoom(Action::SaveRename));
    wait(&mut menu, &mut local, &mut world);
    assert_eq!(
        menu.dressing_room.selected_cape().unwrap().name,
        "Renamed cape"
    );
    menu.activate(MenuAction::DressingRoom(Action::BeginDeleteCape(index)));
    menu.activate(MenuAction::DressingRoom(Action::ConfirmDelete));
    wait(&mut menu, &mut local, &mut world);
    assert!(local.cape.is_none());
    assert!(menu.dressing_room.selected_cape().is_none());
    assert_eq!(local.rgba8, body);
}

#[test]
fn cached_roster_and_skin_commands_publish_while_cape_refresh_is_blocked() {
    let layout = crate::install_layout::scratch("dressing-room-refresh");
    let local = crate::player_skin::LocalPlayerSkin::generated_default("refresh fixture");
    let (entered, started) = crossbeam_channel::bounded(1);
    let (release, continue_refresh) = crossbeam_channel::bounded(1);
    let worker = Worker::start_with_refresh(layout.clone(), local, move |_| {
        entered.send(()).unwrap();
        continue_refresh.recv().unwrap();
    })
    .unwrap();
    let timeout = std::time::Duration::from_secs(5);
    let cached = worker
        .results
        .recv_timeout(timeout)
        .expect("cached skin roster must not await texture acquisition");
    assert!(cached.completed_command);
    assert!(!cached.view.skins.is_empty());
    started.recv_timeout(timeout).unwrap();
    let source = layout.user_data_root.join("during-refresh.png");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(
        protocol::CLASSIC_SKIN_SIDE as u32,
        protocol::CLASSIC_SKIN_SIDE as u32,
        image::Rgba([210, 90, 50, 255]),
    )
    .save(&source)
    .unwrap();
    worker.commands.send(Job::Import(Some(source))).unwrap();
    let selected = worker
        .results
        .recv_timeout(timeout)
        .expect("selection commands must not wait for texture acquisition");
    assert!(selected.completed_command);
    assert!(selected.view.selected_skin().unwrap().imported);
    assert!(selected.view.message.is_none());
    release.send(()).unwrap();
    let refreshed = worker.results.recv_timeout(timeout).unwrap();
    assert!(!refreshed.completed_command);
    assert_eq!(
        refreshed.view.selected_skin(),
        selected.view.selected_skin()
    );
    assert_eq!(refreshed.view.skins, selected.view.skins);
}

#[test]
fn passive_cape_refresh_preserves_open_editor_and_pending_command_count() {
    let mut menu = MenuRuntime::new(false, 2, "passive refresh".to_owned());
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
    let mut view = menu.dressing_room.as_ref().clone();
    let editor = launcher::dressing_room::SkinEditor {
        index: 0,
        mode: launcher::dressing_room::SkinEditorMode::Rename,
        target: launcher::dressing_room::SkinEditorTarget::Cape,
        draft: "Unsubmitted name".to_owned(),
    };
    view.editor = Some(editor.clone());
    view.message = Some("Current editor message".to_owned());
    menu.dressing_room = Arc::new(view.clone());
    menu.field = Some(MenuField::SkinName);
    let (results, receive) = crossbeam_channel::unbounded();
    let (commands, _jobs) = crossbeam_channel::bounded(1);
    view.editor = None;
    view.message = None;
    results
        .send(Outcome {
            view,
            changed: false,
            packet: None,
            completed_command: false,
        })
        .unwrap();
    menu.dressing_room_worker = Some(Worker {
        commands,
        results: receive,
        pending: 1,
    });
    menu.poll_dressing_room(None, &mut ClientWorld::default(), None, 0);
    assert_eq!(menu.dressing_room.editor.as_ref(), Some(&editor));
    assert_eq!(menu.field, Some(MenuField::SkinName));
    assert_eq!(menu.dressing_room_worker.as_ref().unwrap().pending, 1);
    assert!(menu.dressing_room.busy);
    assert_eq!(
        menu.dressing_room.message.as_deref(),
        Some("Current editor message")
    );
}

#[test]
fn busy_cancel_dismisses_editor_without_cancelling_pending_work() {
    let mut menu = MenuRuntime::new(false, 2, "busy cancel".to_owned());
    menu.activate(MenuAction::Navigate(MenuScreen::DressingRoom));
    let (commands, jobs) = crossbeam_channel::bounded(1);
    let (_results, receive) = crossbeam_channel::unbounded();
    commands
        .send(Job::RenameCape(0, "Queued name".to_owned()))
        .unwrap();
    menu.dressing_room_worker = Some(Worker {
        commands,
        results: receive,
        pending: 1,
    });
    let view = Arc::make_mut(&mut menu.dressing_room);
    view.busy = true;
    view.editor = Some(launcher::dressing_room::SkinEditor {
        index: 0,
        mode: launcher::dressing_room::SkinEditorMode::Rename,
        target: launcher::dressing_room::SkinEditorTarget::Cape,
        draft: "Queued name".to_owned(),
    });
    menu.field = Some(MenuField::SkinName);
    assert_eq!(
        menu.focus_actions(),
        vec![MenuAction::DressingRoom(Action::Cancel)]
    );
    menu.activate(MenuAction::DressingRoom(Action::Cancel));
    assert!(menu.dressing_room.editor.is_none());
    assert_eq!(menu.field, None);
    assert!(menu.dressing_room.busy);
    assert_eq!(menu.dressing_room_worker.as_ref().unwrap().pending, 1);
    assert!(matches!(jobs.try_recv(),Ok(Job::RenameCape(0,name)) if name=="Queued name"));
}

#[test]
fn dressing_room_focus_excludes_selected_section_and_model_tabs() {
    use launcher::dressing_room::DressingRoomSection;
    let mut menu = MenuRuntime::new(false, 2, "focus tabs".to_owned());
    let view = launcher::dressing_room::DressingRoomView {
        skins: vec![launcher::dressing_room::DressingRoomSkin {
            id: "fixture".to_owned(),
            name: "Custom".to_owned(),
            path: "private.png".to_owned(),
            imported: true,
            model: SkinModel::Classic,
            skin: menu.player_skin.standard_skin(),
        }]
        .into(),
        selected: Some(0),
        ..Default::default()
    };
    menu.dressing_room = Arc::new(view);
    let actions = menu.dressing_room_focus();
    assert!(
        !actions.contains(&MenuAction::DressingRoom(Action::SetSection(
            DressingRoomSection::Skins
        )))
    );
    assert!(
        actions.contains(&MenuAction::DressingRoom(Action::SetSection(
            DressingRoomSection::Capes
        )))
    );
    assert!(
        !actions.contains(&MenuAction::DressingRoom(Action::SetModel(
            SkinModel::Classic
        )))
    );
    assert!(actions.contains(&MenuAction::DressingRoom(Action::SetModel(SkinModel::Slim))));
    let view = Arc::make_mut(&mut menu.dressing_room);
    view.section = DressingRoomSection::Capes;
    let actions = menu.dressing_room_focus();
    assert!(
        actions.contains(&MenuAction::DressingRoom(Action::SetSection(
            DressingRoomSection::Skins
        )))
    );
    assert!(
        !actions.contains(&MenuAction::DressingRoom(Action::SetSection(
            DressingRoomSection::Capes
        )))
    );
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, MenuAction::DressingRoom(Action::SetModel(_))))
    );
}

#[test]
fn rename_focus_omits_save_for_blank_draft() {
    let mut menu = MenuRuntime::new(false, 2, "blank name focus".to_owned());
    Arc::make_mut(&mut menu.dressing_room).editor = Some(launcher::dressing_room::SkinEditor {
        index: 0,
        mode: launcher::dressing_room::SkinEditorMode::Rename,
        target: launcher::dressing_room::SkinEditorTarget::Skin,
        draft: " \t  ".to_owned(),
    });
    assert_eq!(
        menu.dressing_room_focus(),
        vec![
            MenuAction::DressingRoom(Action::Cancel),
            MenuAction::EditSkinName
        ]
    );
    Arc::make_mut(&mut menu.dressing_room)
        .editor
        .as_mut()
        .unwrap()
        .draft = "Name".to_owned();
    assert!(
        menu.dressing_room_focus()
            .contains(&MenuAction::DressingRoom(Action::SaveRename))
    );
}

#[test]
fn busy_skin_name_cannot_receive_text_or_focus() {
    let mut menu = MenuRuntime::new(false, 2, "busy name field".to_owned());
    let view = Arc::make_mut(&mut menu.dressing_room);
    view.busy = true;
    view.editor = Some(launcher::dressing_room::SkinEditor {
        index: 0,
        mode: launcher::dressing_room::SkinEditorMode::Rename,
        target: launcher::dressing_room::SkinEditorTarget::Skin,
        draft: "Queued".to_owned(),
    });
    menu.skin_name.set_text("Queued");
    menu.field = Some(MenuField::SkinName);
    menu.edit_text(" changes");
    assert_eq!(menu.skin_name.as_str(), "Queued");
    assert_eq!(menu.dressing_room.editor.as_ref().unwrap().draft, "Queued");
    menu.field = None;
    menu.activate(MenuAction::EditSkinName);
    assert_eq!(menu.field, None);
    assert_eq!(
        menu.dressing_room_focus(),
        vec![MenuAction::DressingRoom(Action::Cancel)]
    );
}
