use super::*;
use crate::render_mode::shader_pack::ShaderPackDirs;
use launcher::menu::MenuAction;

/// Runs the production removal worker against an isolated imported library.
fn fixture(path: &std::path::Path, selected: Option<&str>) -> App {
    let packs = path.join("shaderpacks");
    for name in ["Dusk", "Lumen"] {
        std::fs::create_dir_all(packs.join(name).join("shaders")).unwrap();
    }
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.sync_shader_packs(vec!["Dusk".into(), "Lumen".into()]);
    let mut settings = RuntimeSettings::default();
    set_graphics_settings(
        &mut settings,
        &GraphicsSettings {
            mode: if selected.is_some() {
                ui::RenderMode::ShaderPack
            } else {
                ui::RenderMode::Vanilla
            },
            shader_pack: selected.map(str::to_owned),
        },
    );
    let mut app = App::new();
    app.insert_resource(RenderModeConfig {
        cli: None,
        attributable: false,
        path: Some(path.join("graphics.json")),
        cli_shader_pack: None,
        shader_pack_revision: 0,
        shader_packs: Some(ShaderPackDirs {
            packs,
            cache: path.join("cache"),
            discovered: Vec::new(),
        }),
    })
    .insert_resource(menu)
    .insert_resource(settings)
    .init_resource::<Time<Real>>();
    app.add_systems(Update, super::super::super::apply_menu_render_mode);
    configure(&mut app);
    app.update();
    app
}

/// Confirms the current row through the same actions as the popup.
fn remove(app: &mut App, index: u16) {
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    let revision = menu.view().shader_packs_revision;
    menu.activate(MenuAction::RemoveShaderPackDialog(index, revision));
    menu.activate(MenuAction::ConfirmRemoveShaderPack(index, revision));
}

#[test]
fn removing_the_selected_copy_switches_off_and_forgets_its_saved_selection() {
    let root = tempfile::tempdir().unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    remove(&mut app, 0);
    test_time::eventually("the confirmed shader pack removal", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(!root.path().join("shaderpacks/Dusk").exists());
    assert!(root.path().join("shaderpacks/Lumen/shaders").is_dir());
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::Vanilla);
    assert!(video.shader_pack.is_none());
    let saved =
        crate::render_mode::load_graphics_settings(&root.path().join("graphics.json")).unwrap();
    assert_eq!(saved.mode, ui::RenderMode::Vanilla);
    assert!(saved.shader_pack.is_none());
    assert_eq!(
        &*app.world().resource::<MenuRuntime>().view().shader_packs,
        ["Lumen".to_owned()]
    );
}

#[test]
fn removing_an_inactive_copy_keeps_the_other_pack_selected() {
    let root = tempfile::tempdir().unwrap();
    let mut app = fixture(root.path(), Some("Lumen"));
    remove(&mut app, 0);
    test_time::eventually("the inactive shader pack removal", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::ShaderPack);
    assert_eq!(video.shader_pack.as_deref(), Some("Lumen"));
}

#[test]
fn removing_a_library_copy_preserves_an_external_cli_pack_with_the_same_name() {
    let root = tempfile::tempdir().unwrap();
    let external = root.path().join("external/Dusk");
    std::fs::create_dir_all(external.join("shaders")).unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    app.world_mut()
        .resource_mut::<RenderModeConfig>()
        .cli_shader_pack = Some(external.clone());
    app.update();
    remove(&mut app, 0);
    test_time::eventually("the shader copy removal with a CLI override", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(!root.path().join("shaderpacks/Dusk").exists());
    assert!(external.join("shaders").is_dir());
    assert_eq!(
        app.world().resource::<RenderModeConfig>().cli_shader_pack,
        Some(external)
    );
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::ShaderPack);
    assert!(video.shader_pack.is_none());
}

#[test]
fn removing_a_relative_cli_selection_switches_off_the_deleted_pack() {
    let current = std::env::current_dir().unwrap();
    let root = tempfile::tempdir_in(&current).unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    let selected = root.path().join("shaderpacks/Dusk");
    app.world_mut()
        .resource_mut::<RenderModeConfig>()
        .cli_shader_pack = Some(selected.strip_prefix(current).unwrap().to_owned());
    app.update();
    remove(&mut app, 0);
    test_time::eventually("the relative CLI shader pack removal", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(!selected.exists());
    assert!(
        app.world()
            .resource::<RenderModeConfig>()
            .cli_shader_pack
            .is_none()
    );
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::Vanilla);
    assert!(video.shader_pack.is_none());
}

#[cfg(unix)]
#[test]
fn removing_a_library_link_preserves_its_external_cli_target() {
    let root = tempfile::tempdir().unwrap();
    let external = root.path().join("external/Dusk");
    std::fs::create_dir_all(external.join("shaders")).unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    let library = root.path().join("shaderpacks/Dusk");
    std::fs::remove_dir_all(&library).unwrap();
    std::os::unix::fs::symlink(&external, &library).unwrap();
    assert!(!selected_entry(&external, &library));
    assert!(selected_entry(&library.join("shaders"), &library));
    app.world_mut()
        .resource_mut::<RenderModeConfig>()
        .cli_shader_pack = Some(external.clone());
    app.update();
    remove(&mut app, 0);
    test_time::eventually("the shader library link removal", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(std::fs::symlink_metadata(&library).is_err());
    assert!(external.join("shaders").is_dir());
    assert_eq!(
        app.world().resource::<RenderModeConfig>().cli_shader_pack,
        Some(external)
    );
    assert_eq!(
        app.world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .render_mode,
        ui::RenderMode::ShaderPack
    );
}

#[cfg(unix)]
#[test]
fn selected_path_identity_resolves_links() {
    let root = tempfile::tempdir().unwrap();
    let library = root.path().join("Dusk");
    let alias = root.path().join("alias");
    std::fs::create_dir(&library).unwrap();
    std::os::unix::fs::symlink(&library, &alias).unwrap();
    assert!(selected_entry(&alias, &library));
    std::fs::create_dir(library.join("shaders")).unwrap();
    assert!(selected_entry(&alias.join("shaders"), &library));
}

#[test]
fn completing_a_removal_preserves_a_newer_pack_selection() {
    let root = tempfile::tempdir().unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<RemovalWorker>().0 = Some(("Dusk".into(), Mutex::new(receive)));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(
            launcher::menu::RendererChoice::ShaderPack(1, revision),
        ));
    }
    assert!(
        send.send(RemovalOutcome {
            result: Ok(()),
            selected_path: Some(root.path().join("shaderpacks/Dusk")),
            removed: true,
        })
        .is_ok()
    );
    app.update();
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::ShaderPack);
    assert_eq!(video.shader_pack.as_deref(), Some("Lumen"));
}

#[test]
fn completion_disables_a_library_selection_after_its_nested_path_disappears() {
    let root = tempfile::tempdir().unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<RemovalWorker>().0 = Some(("Dusk".into(), Mutex::new(receive)));
    assert!(
        send.send(RemovalOutcome {
            result: Ok(()),
            selected_path: Some(root.path().join("shaderpacks/Dusk/Nested")),
            removed: true,
        })
        .is_ok()
    );
    app.update();
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::Vanilla);
    assert!(video.shader_pack.is_none());
}

#[test]
fn failed_deletion_restores_controls_and_keeps_the_library_entry() {
    let root = tempfile::tempdir().unwrap();
    let mut app = fixture(root.path(), Some("Dusk"));
    let saved_path = root.path().join("graphics.json");
    save_graphics_settings(
        &saved_path,
        &GraphicsSettings {
            mode: ui::RenderMode::ShaderPack,
            shader_pack: Some("Dusk".into()),
        },
    )
    .unwrap();
    // A settings directory cannot be removed as an option file, so report failure.
    std::fs::create_dir(root.path().join("shaderpacks/Dusk.txt")).unwrap();
    remove(&mut app, 0);
    test_time::eventually("shader removal failure", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(root.path().join("shaderpacks/Dusk/shaders").is_dir());
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::ShaderPack);
    assert_eq!(video.shader_pack.as_deref(), Some("Dusk"));
    let saved = crate::render_mode::load_graphics_settings(&saved_path).unwrap();
    assert_eq!(saved.mode, ui::RenderMode::ShaderPack);
    assert_eq!(saved.shader_pack.as_deref(), Some("Dusk"));
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_packs
            .len(),
        2
    );
}

#[cfg(unix)]
#[test]
fn removing_the_target_of_a_selected_library_link_switches_off() {
    let root = tempfile::tempdir().unwrap();
    let packs = root.path().join("shaderpacks");
    let mut app = fixture(root.path(), Some("Alias"));
    std::fs::create_dir_all(packs.join("Actual/shaders")).unwrap();
    std::os::unix::fs::symlink(packs.join("Actual"), packs.join("Alias")).unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .sync_shader_packs(vec!["Actual".into(), "Alias".into()]);
    remove(&mut app, 0);
    test_time::eventually("the confirmed shader pack removal", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_pack_removing
            .is_none()
    });
    assert!(!packs.join("Actual").exists());
    let video = &app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .video;
    assert_eq!(video.render_mode, ui::RenderMode::Vanilla);
    assert!(video.shader_pack.is_none());
}
