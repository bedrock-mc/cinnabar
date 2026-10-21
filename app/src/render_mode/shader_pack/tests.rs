use super::*;
use crate::render_mode::{GraphicsSettings, set_graphics_settings};

fn packs() -> PathBuf {
    PathBuf::from("data").join("shaderpacks")
}

fn cache() -> PathBuf {
    PathBuf::from("data").join("cache").join("shaderpacks")
}

fn config(launch: Option<PathBuf>) -> RenderModeConfig {
    RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: launch,
        shader_packs: Some(ShaderPackDirs {
            packs: packs(),
            cache: cache(),
            discovered: vec![("Nested".to_owned(), packs().join("Nested").join("Inner"))],
        }),
        shader_pack_revision: 0,
    }
}

fn app(config: RenderModeConfig) -> App {
    let mut app = App::new();
    app.init_resource::<RuntimeSettings>()
        .insert_resource(config)
        .init_resource::<ShaderPackRequest>()
        .add_systems(Update, sync_shader_pack_request);
    app
}

fn select(app: &mut App, mode: RenderMode, pack: Option<&str>) {
    set_graphics_settings(
        &mut app.world_mut().resource_mut::<RuntimeSettings>(),
        &GraphicsSettings {
            mode,
            shader_pack: pack.map(str::to_owned),
        },
    );
}

fn requested(app: &App) -> Option<PathBuf> {
    app.world().resource::<ShaderPackRequest>().pack.clone()
}

/// The path a compiled build requests; builds without shader packs request nothing.
fn compiled(path: PathBuf) -> Option<PathBuf> {
    SHADER_PACKS_ENABLED.then_some(path)
}

#[test]
fn request_follows_the_selected_pack_only_in_shader_pack_mode() {
    let mut app = app(config(None));
    app.update();
    assert_eq!(requested(&app), None);
    assert_eq!(
        app.world().resource::<ShaderPackRequest>().cache_dir,
        Some(cache())
    );
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), compiled(packs().join("Dusk.zip")));
    select(&mut app, RenderMode::ShaderPack, Some("Nested"));
    app.update();
    assert_eq!(
        requested(&app),
        compiled(packs().join("Nested").join("Inner"))
    );
    select(&mut app, RenderMode::ShaderPack, Some("../escape"));
    app.update();
    assert_eq!(requested(&app), None);
    select(&mut app, RenderMode::ShaderPack, None);
    app.update();
    assert_eq!(requested(&app), None);
    select(&mut app, RenderMode::Vanilla, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), None);
}

#[test]
fn the_launch_pack_wins_over_the_saved_selection() {
    let launch = PathBuf::from("elsewhere").join("Lumen");
    let mut app = app(config(Some(launch.clone())));
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), compiled(launch));
    select(&mut app, RenderMode::Vanilla, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), None);
}

#[test]
fn an_unchanged_request_is_not_rewritten() {
    let mut app = app(config(None));
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    app.world_mut().clear_trackers();
    let same = app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .clone();
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(same);
    app.update();
    assert!(!app.world().resource_ref::<ShaderPackRequest>().is_changed());
}

#[cfg(feature = "shaderpacks")]
#[test]
fn reselecting_a_failed_pack_requests_another_load() {
    use launcher::menu::{MenuAction, RendererChoice};
    use render_scene::shaderpack::ShaderPackStatus;

    let mut app = app(config(None));
    app.insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_systems(
            Update,
            super::super::apply_menu_render_mode.before(sync_shader_pack_request),
        );
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    let path = requested(&app).unwrap();
    let previous = app.world().resource::<ShaderPackRequest>().clone();
    app.insert_resource(ShaderPackStatus::Failed {
        pack: path.clone(),
        reason: "pack was missing".into(),
    });
    app.world_mut().clear_trackers();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned()]);
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(0)));
    }
    app.update();
    assert_eq!(requested(&app), Some(path));
    let reselected = app.world().resource::<ShaderPackRequest>().clone();
    assert_ne!(reselected, previous);
    app.world_mut().clear_trackers();
    app.update();
    assert_eq!(app.world().resource::<ShaderPackRequest>(), &reselected);
}

#[cfg(feature = "shaderpacks")]
#[test]
fn a_menu_pick_replaces_the_launch_pack_and_saves_only_its_name() {
    use launcher::menu::{MenuAction, RendererChoice};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graphics.json");
    let launch = PathBuf::from("elsewhere").join("Lumen");
    let mut config = config(Some(launch.clone()));
    config.path = Some(path.clone());
    let mut app = app(config);
    app.insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_systems(
            Update,
            super::super::apply_menu_render_mode.before(sync_shader_pack_request),
        );
    select(&mut app, RenderMode::ShaderPack, None);
    app.update();
    assert_eq!(requested(&app), Some(launch));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned()]);
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0, revision,
        )));
    }
    app.update();
    assert_eq!(requested(&app), Some(packs().join("Dusk.zip")));
    let saved = super::super::load_graphics_settings(&path).unwrap();
    assert_eq!(saved.mode, RenderMode::ShaderPack);
    assert_eq!(saved.shader_pack.as_deref(), Some("Dusk.zip"));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("Lumen"));
}

#[cfg(feature = "shaderpacks")]
#[test]
fn opening_settings_lists_packs_added_while_running() {
    use launcher::menu::MenuAction;

    let directory = tempfile::tempdir().unwrap();
    let mut config = config(None);
    config.shader_packs = Some(ShaderPackDirs {
        packs: directory.path().to_path_buf(),
        cache: cache(),
        discovered: Vec::new(),
    });
    let mut app = App::new();
    app.insert_resource(config)
        .insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_systems(Update, refresh_shader_packs);
    app.update();
    std::fs::write(directory.path().join("Dusk.zip"), b"").unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_packs
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().shader_packs[..],
        ["Dusk.zip".to_owned()]
    );
}
