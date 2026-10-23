//! Dropped files route by kind: world files to the Worlds tab, everything else to pack import.

use super::*;
use bevy::prelude::{App, Entity, Update};
use bevy::window::FileDragAndDrop;
use client_shell::{local_worlds::LocalWorlds, menu::MenuRuntime};
use launcher::{
    local_worlds::Screen,
    menu::{MenuAction, MenuScreen},
};

fn fixture(screen: MenuScreen) -> (App, crossbeam_channel::Receiver<Command>) {
    let (commands, incoming) = crossbeam_channel::unbounded();
    let (_outgoing, events) = crossbeam_channel::unbounded();
    let worker = Worker {
        commands: Some(Arc::new(commands)),
        disk: None,
        picker: crossbeam_channel::unbounded().0,
        picker_open: false,
        events,
        snapshot: Snapshot::default(),
        awaiting: None,
        minecraft_import_pending: false,
        minecraft_import_request: None,
    };
    let mut menu = MenuRuntime::new(true, 2, "Drop fixture".into());
    menu.activate(MenuAction::Navigate(screen));
    let mut app = App::new();
    app.insert_resource(worker)
        .insert_resource(menu)
        .init_resource::<LocalWorlds>()
        .init_resource::<crate::runtime::network::PackReload>()
        .add_message::<FileDragAndDrop>()
        .add_systems(Update, crate::global_resources::drive);
    (app, incoming)
}

fn drop_files(app: &mut App, files: &[&str]) {
    for file in files {
        app.world_mut().write_message(FileDragAndDrop::DroppedFile {
            window: Entity::PLACEHOLDER,
            path_buf: PathBuf::from(file),
        });
    }
    app.update();
}

fn pack_imports(incoming: &crossbeam_channel::Receiver<Command>) -> Vec<PathBuf> {
    incoming
        .try_iter()
        .filter_map(|command| match command {
            Command::Import(path) => Some(path),
            _ => None,
        })
        .collect()
}

#[test]
fn world_files_dropped_on_the_worlds_tab_import_as_worlds() {
    let (mut app, incoming) = fixture(MenuScreen::Play);
    drop_files(&mut app, &["/drop/Castle.MCWORLD", "/drop/Faithful.mcpack"]);
    assert_eq!(
        pack_imports(&incoming),
        [PathBuf::from("/drop/Faithful.mcpack")]
    );
    assert_ne!(
        app.world().resource::<LocalWorlds>().menu().screen(),
        Screen::List,
        "the world drop reached the worlds module"
    );
}

#[test]
fn world_files_dropped_elsewhere_are_not_imported_as_packs() {
    let (mut app, incoming) = fixture(MenuScreen::Home);
    drop_files(&mut app, &["/drop/Castle.mcworld"]);
    assert!(pack_imports(&incoming).is_empty());
    assert_eq!(
        app.world().resource::<LocalWorlds>().menu().screen(),
        Screen::List
    );
}

#[test]
fn shader_pack_drops_bypass_resource_import_only_in_the_shaders_tab() {
    use launcher::menu::settings_sections::SettingSection;
    for section in [SettingSection::Video, SettingSection::Shaders] {
        let (mut app, incoming) = fixture(MenuScreen::Settings);
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::SettingsSection(section.definition().index));
        let files = ["/drop/Dusk.zip", "/drop/Faithful.mcpack"];
        drop_files(&mut app, &files);
        let expected: &[PathBuf] =
            if render_model::SHADER_PACKS_ENABLED && section == SettingSection::Shaders {
                &[]
            } else {
                &files.map(PathBuf::from)
            };
        assert_eq!(pack_imports(&incoming), expected, "{section:?}");
    }
}
