use super::*;

#[test]
fn stale_worker_failure_does_not_consume_a_newer_request() {
    let mut app = super::super::pack_reload_tests::app_with_assets(Arc::new(
        assets::RuntimeAssets::diagnostic(),
    ));
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.request_globals(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::default(),
        ));
        reload.pending = Some(Pending {
            revision: 0,
            generation: 0,
            result: Mutex::new(rx),
            carrier: Mutex::new(mpsc::sync_channel(1).1),
            early: None,
        });
    }
    tx.send(Err("stale failure".into())).unwrap();
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert!(reload.error().is_none());
    assert!(reload.progress().is_some());
    assert!(reload.pending.is_some());
}

#[test]
fn stale_session_worker_disconnect_does_not_fail_the_current_session() {
    let mut app = super::super::pack_reload_tests::app_with_assets(Arc::new(
        assets::RuntimeAssets::diagnostic(),
    ));
    let (tx, rx) = mpsc::sync_channel(1);
    {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.generation = 2;
        reload.revision = 1;
        reload.pending = Some(Pending {
            revision: 1,
            generation: 1,
            result: Mutex::new(rx),
            carrier: Mutex::new(mpsc::sync_channel(1).1),
            early: None,
        });
    }
    drop(tx);
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert!(reload.error().is_none());
    assert!(reload.progress().is_some());
    assert!(reload.pending.is_some());
}

#[test]
fn environment_images_publish_only_for_the_accepted_revision_and_generation() {
    use super::super::reload_environment::{EnvironmentBase, prepare_environment};
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([11, 22, 33, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mask_path = render_scene::screen_overlay::PUMPKIN_MASK_PATH;
    let end_path = format!("{}.png", assets::weather_textures::END_SKY_TEXTURE_PATH);
    let stack = super::super::pack_reload_tests::stack(&[
        (mask_path, png.get_ref()),
        (&end_path, png.get_ref()),
    ]);
    let base = EnvironmentBase::new(Default::default(), None, Default::default());
    for (revision, generation, accepted) in [(1, 2, true), (0, 2, false), (1, 1, false)] {
        let mut app = super::super::pack_reload_tests::app_with_assets(assets.clone());
        app.init_resource::<render_scene::WeatherTextureAssets>()
            .init_resource::<render_scene::ScreenOverlayScene>();
        let application =
            resource_packs::prepare_validated_application(stack.clone(), Arc::default());
        let environment = prepare_environment(
            &resource_pack::LayeredPackView::new(stack.clone()),
            &base,
            true,
            false,
        );
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut reload = app.world_mut().resource_mut::<PackReload>();
            reload.revision = 1;
            reload.generation = 2;
            reload.applied = 1;
            reload.pending = Some(Pending {
                revision,
                generation,
                result: Mutex::new(rx),
                carrier: Mutex::new(mpsc::sync_channel(1).1),
                early: None,
            });
        }
        tx.send(Ok(Prepared {
            revision,
            generation,
            application,
            assets: assets.clone(),
            environment: Some(environment),
            elapsed: Duration::ZERO,
        }))
        .unwrap();
        app.update();
        let sky = app
            .world()
            .resource::<render_scene::WeatherTextureAssets>()
            .end_sky();
        let scene = app.world().resource::<render_scene::ScreenOverlayScene>();
        assert_eq!(sky.is_some(), accepted);
        assert_eq!(scene.textures.is_some(), accepted);
        if accepted {
            assert_eq!(&sky.unwrap().rgba8[..4], &[11, 22, 33, 255]);
            assert_eq!(
                &scene.textures.as_ref().unwrap().pumpkin().unwrap()[..4],
                &[11, 22, 33, 255]
            );
        }
    }
}

#[test]
fn failed_global_selection_is_acknowledged_after_session_end_recovery() {
    use bevy::prelude::{IntoScheduleConfigs, Local, ResMut, Update};
    use client_shell::menu::MenuRuntime;
    use resource_pack::GlobalPackLibrary;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("installed");
    let archive = super::super::pack_reload_tests::stack(&[]);
    let path = fixture.path().join("fixture.mcpack");
    std::fs::write(&path, archive.packs()[0].archive_bytes().as_slice()).unwrap();
    let mut library = GlobalPackLibrary::open(&root, [1, 0, 0]).unwrap();
    let pack = library.import(&path).unwrap().imported.remove(0);
    library.activate(pack.id).unwrap();
    library.apply().unwrap();
    let expected = library.active().to_vec();
    let mut app = super::super::pack_reload_tests::app_with_assets(Arc::new(
        assets::RuntimeAssets::diagnostic(),
    ));
    app.insert_resource(MenuRuntime::new(true, 2, "Fixture".into()))
        .add_message::<bevy::window::FileDragAndDrop>();
    app.world_mut()
        .resource_mut::<PackReload>()
        .begin_session(1, &PackApplication::default());
    crate::global_resources::configure(&mut app, root, fixture.path().join("config"), Vec::new());
    app.add_systems(
        Update,
        (|mut reload: ResMut<PackReload>, mut failed: Local<bool>| {
            if reload.revision > 1 && !*failed {
                let (tx, rx) = mpsc::sync_channel(1);
                tx.send(Err("fixture reload failure".into())).unwrap();
                reload.pending = Some(Pending {
                    revision: reload.revision,
                    generation: reload.generation,
                    result: Mutex::new(rx),
                    carrier: Mutex::new(mpsc::sync_channel(1).1),
                    early: None,
                });
                *failed = true;
            }
        })
        .before(reload_resource_packs),
    );
    test_time::eventually("the failed global reload", || {
        app.update();
        app.world().resource::<PackReload>().error().is_some()
    });
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .global_resources
            .applied_selection
            .is_none()
    );
    let revision = {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.end_session();
        reload.revision
    };
    super::super::pack_reload_tests::settle(&mut app, revision);
    assert!(app.world().resource::<PackReload>().error().is_none());
    test_time::eventually("the recovered selection acknowledgement", || {
        app.update();
        app.world()
            .resource::<MenuRuntime>()
            .global_resources
            .applied_selection
            .as_ref()
            == Some(&expected)
    });
    assert!(
        !app.world()
            .resource::<MenuRuntime>()
            .global_resources
            .has_pending_changes()
    );
}
