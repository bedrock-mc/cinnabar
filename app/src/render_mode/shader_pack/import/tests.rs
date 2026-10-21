use super::*;
use crate::render_mode::shader_pack::ShaderPackDirs;
use launcher::menu::{MenuAction, MenuScreen, settings_sections::SettingSection};
use std::path::Path;

/// The render-mode config with its pack folder at `packs`.
fn config(packs: &Path) -> RenderModeConfig {
    RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: None,
        shader_packs: Some(ShaderPackDirs {
            packs: packs.to_path_buf(),
            cache: packs.join("cache"),
            discovered: Vec::new(),
        }),
        shader_pack_revision: 0,
    }
}

fn app(packs: &Path, section: SettingSection) -> App {
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(section.definition().index));
    let mut app = App::new();
    app.add_message::<FileDragAndDrop>()
        .init_resource::<Time<Real>>()
        .insert_resource(config(packs))
        .insert_resource(menu);
    configure(&mut app);
    app
}

fn drop_file(app: &mut App, path: PathBuf) {
    app.world_mut().write_message(FileDragAndDrop::DroppedFile {
        window: Entity::PLACEHOLDER,
        path_buf: path,
    });
}

fn listed(app: &App) -> Vec<String> {
    app.world()
        .resource::<MenuRuntime>()
        .view()
        .shader_packs
        .to_vec()
}

/// A dropped pack folder named `name` holding `shaders/`.
fn pack_folder(root: &Path, name: &str) -> PathBuf {
    let pack = root.join("downloads").join(name);
    std::fs::create_dir_all(pack.join("shaders")).unwrap();
    pack
}

#[test]
fn a_pack_dropped_on_shader_settings_is_copied_in_and_listed() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("shaderpacks");
    let mut app = app(&packs, SettingSection::Shaders);
    drop_file(&mut app, pack_folder(root.path(), "Dusk"));
    test_time::eventually("the dropped pack to be listed", || {
        app.update();
        listed(&app) == ["Dusk"]
    });
    assert!(packs.join("Dusk/shaders").is_dir());
}

#[test]
fn drops_elsewhere_are_left_to_other_importers() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("shaderpacks");
    let mut app = app(&packs, SettingSection::GlobalTexturePack);
    drop_file(&mut app, pack_folder(root.path(), "Dusk"));
    app.update();
    // Leaving for the Shaders tab does not replay a drop made elsewhere.
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsSection(
            SettingSection::Shaders.definition().index,
        ));
    app.update();
    app.update();
    assert!(!packs.exists());
    assert!(listed(&app).is_empty());
}

#[test]
fn notices_use_iris_wording() {
    let cases = [
        (
            DropOutcome::Added(vec!["Dusk.zip".into()]),
            "Added Shader Pack \"Dusk.zip\"",
        ),
        (
            DropOutcome::Added(vec!["Dusk.zip".into(), "Lumen".into()]),
            "Added 2 Shader Packs",
        ),
        (
            DropOutcome::NoPacks(Some("notes.txt".into())),
            "\"notes.txt\" is not a valid Shader Pack",
        ),
        (
            DropOutcome::NoPacks(None),
            "Files were not valid Shader Packs",
        ),
        (
            DropOutcome::AlreadyExists("Dusk.zip".into()),
            "\"Dusk.zip\" is already in your Shader Packs folder!",
        ),
        (
            DropOutcome::CopyFailed {
                name: "Dusk.zip".into(),
                error: "disk full".into(),
            },
            "Could not add Shader Pack \"Dusk.zip\"",
        ),
    ];
    for (outcome, expected) in cases {
        assert_eq!(notice(&outcome), expected);
    }
}
