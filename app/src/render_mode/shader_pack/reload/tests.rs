use super::*;
use std::path::PathBuf;

fn request(pack: Option<&str>) -> ShaderPackRequest {
    ShaderPackRequest {
        pack: pack.map(PathBuf::from),
        cache_dir: None,
        revision: 2,
        options: None,
    }
}

#[test]
fn a_key_reload_reports_once_its_own_load_settles() {
    let request = request(Some("Dusk"));
    let ready = ShaderPackStatus::Ready(PathBuf::from("Dusk"));
    // The shown pack stays Ready while the reload translates, so only the settled
    // revision tells the reload apart from the load before it.
    assert_eq!(report_line(2, &request, 1, Some(&ready)), None);
    assert_eq!(
        report_line(2, &request, 2, Some(&ready)).as_deref(),
        Some(RELOADED)
    );
    let failed = ShaderPackStatus::Failed {
        pack: PathBuf::from("Dusk"),
        reason: "required programs failed: final".into(),
    };
    assert_eq!(
        report_line(2, &request, 2, Some(&failed)).as_deref(),
        Some("§cFailed to reload shaders! Reason: required programs failed: final")
    );
}

/// The reload key neither scans the pack folder nor reports for players without a pack, and
/// reloads a selected pack without re-listing the folder on the frame.
#[cfg(feature = "shaderpacks")]
#[test]
fn the_reload_key_only_reloads_a_selected_pack() {
    use crate::render_mode::{
        GraphicsSettings, RuntimeSettings, set_graphics_settings,
        shader_pack::{ShaderPackDirs, sync_shader_pack_request},
    };
    use ui::RenderMode;

    let directory = tempfile::tempdir().unwrap();
    let packs = directory.path().to_path_buf();
    std::fs::create_dir_all(packs.join("Dusk/shaders")).unwrap();
    let mut app = App::new();
    app.init_resource::<RuntimeSettings>()
        .init_resource::<ShaderPackRequest>()
        .init_resource::<ShaderPackReloadReport>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(PlayerRuntime(player_state::PlayerState::new(1)))
        .insert_resource(MenuRuntime::new(false, 2, "Steve".into()))
        .insert_resource(RenderModeConfig {
            cli: None,
            attributable: false,
            path: None,
            cli_shader_pack: None,
            shader_packs: Some(ShaderPackDirs {
                packs: packs.clone(),
                cache: packs.join("cache"),
                discovered: Vec::new(),
            }),
            shader_pack_revision: 0,
        })
        .add_systems(
            Update,
            (reload_on_key, apply_reload, sync_shader_pack_request).chain(),
        );
    let press = |app: &mut App| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(KeyCode::KeyR);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    };
    let listed = |app: &App| {
        let config = app.world().resource::<RenderModeConfig>();
        config.shader_packs.as_ref().unwrap().discovered.len()
    };

    app.update();
    press(&mut app);
    let config = app.world().resource::<RenderModeConfig>();
    assert_eq!(config.shader_pack_revision, 0, "nothing to reload");
    assert_eq!(listed(&app), 0, "no folder scan");
    assert_eq!(
        app.world().resource::<ShaderPackReloadReport>().pending,
        None,
        "no chat line"
    );

    set_graphics_settings(
        &mut app.world_mut().resource_mut::<RuntimeSettings>(),
        &GraphicsSettings {
            mode: RenderMode::ShaderPack,
            shader_pack: Some("Dusk".into()),
        },
    );
    app.update();
    press(&mut app);
    let request = app.world().resource::<ShaderPackRequest>();
    assert_eq!(request.pack, Some(packs.join("Dusk")));
    assert_eq!(request.revision, 1);
    assert_eq!(
        app.world().resource::<ShaderPackReloadReport>().pending,
        Some(1)
    );
    assert_eq!(listed(&app), 0, "the key reload does not scan the folder");
}

#[cfg(feature = "shaderpacks")]
#[test]
fn a_reload_relists_packs_and_loads_the_selected_pack_again_under_a_new_revision() {
    use crate::render_mode::{
        GraphicsSettings, RuntimeSettings, set_graphics_settings,
        shader_pack::{ShaderPackDirs, sync_shader_pack_request},
    };
    use ui::RenderMode;

    let directory = tempfile::tempdir().unwrap();
    let packs = directory.path().to_path_buf();
    std::fs::create_dir_all(packs.join("Dusk/shaders")).unwrap();
    let mut app = App::new();
    app.init_resource::<RuntimeSettings>()
        .init_resource::<ShaderPackRequest>()
        .init_resource::<ShaderPackReloadReport>()
        .insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .insert_resource(RenderModeConfig {
            cli: None,
            attributable: false,
            path: None,
            cli_shader_pack: None,
            shader_packs: Some(ShaderPackDirs {
                packs: packs.clone(),
                cache: packs.join("cache"),
                discovered: Vec::new(),
            }),
            shader_pack_revision: 0,
        })
        .add_systems(Update, (apply_reload, sync_shader_pack_request).chain());
    set_graphics_settings(
        &mut app.world_mut().resource_mut::<RuntimeSettings>(),
        &GraphicsSettings {
            mode: RenderMode::ShaderPack,
            shader_pack: Some("Dusk".into()),
        },
    );
    app.update();
    let first = app.world().resource::<ShaderPackRequest>().clone();
    assert_eq!(first.pack, Some(packs.join("Dusk")));

    std::fs::create_dir_all(packs.join("Lumen/shaders")).unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .request_shader_pack_reload();
    app.update();
    let reloaded = app.world().resource::<ShaderPackRequest>().clone();
    assert_eq!(reloaded.pack, first.pack);
    assert_eq!(reloaded.revision, first.revision + 1);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().shader_packs[..],
        ["Dusk".to_owned(), "Lumen".to_owned()]
    );

    app.update();
    assert_eq!(
        app.world().resource::<ShaderPackRequest>().revision,
        reloaded.revision,
        "one request reloads once"
    );
    assert_eq!(
        app.world().resource::<ShaderPackReloadReport>().pending,
        None
    );
}
