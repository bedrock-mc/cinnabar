use super::*;
use resource_pack::{GlobalPackLibrary, LayeredPackView};

use super::super::pack_reload_tests::stack;

#[test]
fn saved_globals_reopen_the_selected_revision_in_the_launcher_layout() {
    use launcher::install_layout::{InstallEnvironment, InstallLayout, Platform};

    let platform = if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    for isolated in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let environment = InstallEnvironment {
            executable: fixture.path().join("target/debug/cinnabar"),
            user_root: isolated.then(|| fixture.path().join("profile")),
            home: None,
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        };
        let layout = InstallLayout::resolve(platform, &environment).unwrap();
        let root = layout.global_resource_packs_dir();
        assert_eq!(
            root,
            fixture
                .path()
                .join(if isolated { "profile/data" } else { ".local" })
                .join("resource-packs/global")
        );
        let mut library = GlobalPackLibrary::open(&root, [1, 0, 0]).unwrap();
        let source = fixture.path().join("fixture.mcpack");
        let mut selections = Vec::new();
        for payload in [b"global=old".as_slice(), b"global=new"] {
            let pack = stack(&[("texts/en_US.lang", payload)]);
            std::fs::write(&source, pack.packs()[0].archive_bytes().as_ref()).unwrap();
            let id = library.import(&source).unwrap().imported[0].id;
            library.activate(id).unwrap();
            library.apply().unwrap();
            selections.push(library.active().to_vec());
            if selections.len() == 1 {
                library.retain_for_restore(&selections[0]).unwrap();
            }
        }
        assert_ne!(selections[0], selections[1]);
        for (selection, payload) in selections.iter().zip([b"global=old", b"global=new"]) {
            library.stage_selection(selection).unwrap();
            library.apply().unwrap();
            let mut reopened = GlobalPackLibrary::open(&root, [1, 0, 0]).unwrap();
            let view = LayeredPackView::new(reopened.preview().unwrap());
            assert_eq!(view.read("texts/en_US.lang").unwrap().as_ref(), payload);
        }
    }
}

#[test]
fn joined_server_packs_allow_global_replacement_and_removal() {
    let server = stack(&[("texts/en_US.lang", b"shared=server\nserver=kept")]);
    let joined = resource_packs::prepare_validated_application(server, Arc::default());
    let mut reload = PackReload::default();
    reload.begin_session(1, &joined);
    for (files, expected) in [
        (
            vec![("texts/en_US.lang", b"shared=old\nglobal=old".as_slice())],
            Some("old"),
        ),
        (
            vec![("texts/en_US.lang", b"shared=new\nglobal=new".as_slice())],
            Some("new"),
        ),
        (Vec::new(), None),
    ] {
        reload.request_globals(stack(&files));
        reload.start(
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            None,
            None,
            None,
        );
        let prepared = reload
            .pending
            .take()
            .unwrap()
            .result
            .into_inner()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        let language = prepared.application.server_lang.as_ref().unwrap();
        assert_eq!(language.lookup("global"), expected);
        assert_eq!(language.lookup("shared"), Some("server"));
        assert_eq!(language.lookup("server"), Some("kept"));
        reload.previous = Some(prepared.application);
    }
}
