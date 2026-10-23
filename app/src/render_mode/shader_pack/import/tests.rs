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
    app_picking(packs, section, Arc::new(|_: &Filter| Ok(Vec::new())))
}

/// [`app`] with a file picker that answers with `pick`.
fn app_picking(packs: &Path, section: SettingSection, pick: Pick) -> App {
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(section.definition().index));
    let mut app = App::new();
    app.add_message::<FileDragAndDrop>()
        .init_resource::<Time<Real>>()
        .insert_resource(config(packs))
        .insert_resource(menu);
    configure_with(&mut app, pick);
    app
}

/// Whether the menu still waits on a picked import.
fn importing(app: &App) -> bool {
    app.world()
        .resource::<MenuRuntime>()
        .view()
        .shader_pack_importing
}

/// Presses the Shaders tab's import target.
fn press_import(app: &mut App) {
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::ImportShaderPacks);
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
fn a_picked_pack_is_copied_in_and_listed() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("shaderpacks");
    let picked = pack_folder(root.path(), "Dusk");
    let mut app = app_picking(
        &packs,
        SettingSection::Shaders,
        Arc::new(move |filter: &Filter| {
            assert_eq!(filter.extensions, shaderpack::PACK_ARCHIVE_EXTENSIONS);
            Ok(vec![picked.clone()])
        }),
    );
    press_import(&mut app);
    test_time::eventually("the picked pack to be listed", || {
        app.update();
        listed(&app) == ["Dusk"] && !importing(&app)
    });
    assert!(packs.join("Dusk/shaders").is_dir());
}

#[test]
fn a_cancelled_or_failed_pick_accepts_the_next_press() {
    for answer in [Ok(Vec::new()), Err("no picker".to_owned())] {
        let root = tempfile::tempdir().unwrap();
        let packs = root.path().join("shaderpacks");
        let mut app = app_picking(
            &packs,
            SettingSection::Shaders,
            Arc::new(move |_: &Filter| answer.clone()),
        );
        press_import(&mut app);
        assert!(importing(&app));
        test_time::eventually("the pick to finish", || {
            app.update();
            !importing(&app)
        });
        assert!(listed(&app).is_empty());
        press_import(&mut app);
        assert!(importing(&app));
    }
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
