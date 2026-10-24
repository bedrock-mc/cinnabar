use super::*;
use launcher::menu::MenuAction;

/// Builds an editor with one unapplied switch change.
fn edited() -> Editor {
    let pack = ShaderPack::new(shaderpack::PackSource::from_files([
        ("settings.glsl", "//#define GLOW\n"),
        ("final.fsh", "#ifdef GLOW\n#endif\nvoid main() {}\n"),
    ]));
    let mut session = OptionSession::new(&pack, &OptionValues::new());
    session.toggle("GLOW");
    assert!(session.has_changes());
    Editor::new(session, "Dusk")
}

#[test]
fn a_command_line_pack_can_open_its_options_without_a_saved_selection() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("CommandLinePack");
    std::fs::create_dir_all(pack.join("shaders")).unwrap();
    let mut app = App::new();
    let mut settings = RuntimeSettings::default();
    super::super::set_render_mode(&mut settings, ui::RenderMode::ShaderPack);
    app.insert_resource(RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: Some(pack),
        shader_packs: None,
        shader_pack_revision: 0,
    })
    .insert_resource(settings)
    .insert_resource(MenuRuntime::new(true, 2, "Player".into()))
    .init_resource::<ShaderOptionsHost>()
    .add_systems(
        Update,
        (super::super::apply_menu_render_mode, drive_shader_options).chain(),
    );
    app.update();
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert_eq!(menu.view().renderer, None);
    assert!(menu.view().shader_options_available);
    menu.activate(MenuAction::ShaderOptions(Action::Open));
    assert!(menu.view().shader_options.is_some());
    app.update();
    assert!(app.world().resource::<ShaderOptionsHost>().open.is_some());
}

#[test]
fn apply_and_done_retain_edits_until_storage_accepts_them() {
    for action in [Action::Apply, Action::Done] {
        let directory = tempfile::tempdir().unwrap();
        let editor = edited();
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.sync_shader_options(Some(editor.view()));
        menu.activate(MenuAction::ShaderOptions(action));
        let mut app = App::new();
        app.insert_resource(RenderModeConfig {
            cli: None,
            attributable: false,
            path: None,
            cli_shader_pack: None,
            shader_packs: None,
            shader_pack_revision: 0,
        })
        .init_resource::<RuntimeSettings>()
        .insert_resource(menu)
        .insert_resource(ShaderOptionsHost {
            open: Some(Open {
                name: "Dusk".into(),
                file: Some(directory.path().to_owned()),
                state: State::Ready(Box::new(editor)),
                saving: None,
                save_error: None,
            }),
            writer: None,
            ..Default::default()
        })
        .add_systems(Update, drive_shader_options);
        app.update();
        let host = app.world().resource::<ShaderOptionsHost>();
        let open = host
            .open
            .as_ref()
            .expect("a failed save keeps the editor open");
        let State::Ready(editor) = &open.state else {
            panic!("the edits remain available");
        };
        assert!(editor.session().has_changes());
        assert!(
            !app.world_mut()
                .resource_mut::<MenuRuntime>()
                .take_shader_pack_reload()
        );
        test_time::eventually("the failed option write", || {
            app.update();
            app.world()
                .resource::<ShaderOptionsHost>()
                .open
                .as_ref()
                .is_some_and(|open| matches!(open.view().status, Status::SaveFailed(_)))
        });
        assert!(
            !app.world_mut()
                .resource_mut::<MenuRuntime>()
                .take_shader_pack_reload()
        );
        let path = directory.path().join("Pack.txt");
        app.world_mut()
            .resource_mut::<ShaderOptionsHost>()
            .open
            .as_mut()
            .unwrap()
            .file = Some(path.clone());
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::ShaderOptions(action));
        test_time::eventually("the retried option write and reload", || {
            app.update();
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .take_shader_pack_reload()
        });
        assert_eq!(
            shaderpack::read_option_file(&path)
                .unwrap()
                .get("GLOW")
                .map(String::as_str),
            Some("true")
        );
        let host = app.world().resource::<ShaderOptionsHost>();
        assert!(
            host.writer.is_none(),
            "a completed save releases its worker"
        );
        if action == Action::Done {
            assert!(host.open.is_none());
        } else {
            let open = host.open.as_ref().unwrap();
            let State::Ready(editor) = &open.state else {
                panic!("the saved editor stays ready");
            };
            assert!(!editor.session().has_changes());
            assert_eq!(open.view().status, Status::Ready);
        }
    }
}

#[test]
fn leaving_the_options_screen_releases_its_queued_reader() {
    let mut app = App::new();
    app.insert_resource(RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: None,
        shader_packs: None,
        shader_pack_revision: 0,
    })
    .init_resource::<RuntimeSettings>()
    .insert_resource(MenuRuntime::new(true, 2, "Player".into()))
    .insert_resource(ShaderOptionsHost {
        open: Some(Open::pending(SelectedPack {
            name: "fixture".into(),
            path: "not-opened".into(),
            options: None,
        })),
        ..Default::default()
    })
    .add_systems(Update, drive_shader_options);
    app.update();
    assert!(app.world().resource::<ShaderOptionsHost>().open.is_none());
}

#[test]
fn closing_the_screen_preserves_and_retires_an_accepted_save() {
    /// Releases the held save before app teardown, including when an assertion fails.
    struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);

    impl Drop for ReleaseOnDrop {
        /// Lets the save finish before its owner joins it at teardown.
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("accepted-save");
    let destination = path.clone();
    let (release, held) = std::sync::mpsc::channel();
    let mut app = App::new();
    let mut release = ReleaseOnDrop(Some(release));
    let open = Open::pending(SelectedPack {
        name: "fixture".into(),
        path: "not-opened".into(),
        options: None,
    });
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.sync_shader_options(Some(open.view()));
    app.insert_resource(RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: None,
        shader_packs: None,
        shader_pack_revision: 0,
    })
    .init_resource::<RuntimeSettings>()
    .insert_resource(menu)
    .insert_resource(ShaderOptionsHost {
        open: Some(open),
        writer: Some(
            Writer::spawn(move || {
                held.recv().map_err(|error| error.to_string())?;
                std::fs::write(destination, "complete").map_err(|error| error.to_string())
            })
            .unwrap(),
        ),
        ..Default::default()
    })
    .add_systems(Update, drive_shader_options);
    app.update();
    assert_eq!(
        app.world()
            .resource::<ShaderOptionsHost>()
            .open
            .as_ref()
            .unwrap()
            .view()
            .status,
        Status::Loading,
        "a reopened editor waits for the accepted save before reading"
    );
    app.world_mut().remove_resource::<MenuRuntime>();
    app.update();
    let host = app.world().resource::<ShaderOptionsHost>();
    assert!(host.open.is_none());
    assert!(
        host.writer.is_some(),
        "closing the screen preserves accepted work"
    );
    release.0.take().unwrap().send(()).unwrap();
    test_time::eventually("the closed screen's accepted save", || {
        app.update();
        app.world().resource::<ShaderOptionsHost>().writer.is_none()
    });
    assert_eq!(std::fs::read_to_string(path).unwrap(), "complete");
}
