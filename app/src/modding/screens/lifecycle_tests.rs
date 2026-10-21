use super::*;
use crate::modding::{hud_editor, registration::HostMetadata};

/// Loads an inert guest whose source identifies its associated metadata.
fn host(directory: &std::path::Path, name: &str) -> ModHost {
    let bytes = br#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#;
    ModHost::load_snapshot_with_grants(&directory.join(name), bytes, Default::default()).unwrap()
}

#[test]
fn host_mutations_keep_metadata_and_retained_owners_attached_to_their_guests() {
    let directory = tempfile::tempdir().unwrap();
    let first = host(directory.path(), "first.wat");
    let second = host(directory.path(), "second.wat");
    let third = host(directory.path(), "third.wat");
    let second_source = second.source().clone();
    let third_source = third.source().clone();
    let mut runtime = ModRuntime::new(first, HostMetadata::environment(10));
    runtime.render_sources.push(0);
    runtime.push_host(second, HostMetadata::environment(20));
    assert!(runtime.render_sources.is_empty());
    runtime.push_host(third, HostMetadata::environment(30));
    runtime.hud_editor_owner = Some(hud_editor::Owner {
        host: 2,
        session: 7,
    });
    runtime.screens = ScreenState {
        owner: Some(2),
        data_change: Some(ModEvent::Action {
            id: "third".into(),
            index: None,
        }),
        ..Default::default()
    };
    runtime.controls.keys_pressed.push("KeyA".into());
    runtime.render_sources = vec![0; 3];

    runtime.remove_host(0);
    assert_eq!(runtime.hosts.host(0).source(), &second_source);
    assert_eq!(
        runtime.metadata[0].owner,
        mod_ui::PanelUiOwner::Environment(20)
    );
    assert_eq!(runtime.hosts.host(1).source(), &third_source);
    assert_eq!(
        runtime.metadata[1].owner,
        mod_ui::PanelUiOwner::Environment(30)
    );
    assert_eq!(runtime.hud_editor_owner.unwrap().host, 1);
    assert_eq!(runtime.hud_editor_owner.unwrap().session, 7);
    assert_eq!(runtime.screens.owner, Some(1));
    assert!(runtime.controls.keys_pressed.is_empty());
    assert!(runtime.render_sources.is_empty());

    runtime.render_sources = vec![0; 2];
    runtime.swap_hosts(0, 1);
    assert_eq!(runtime.hosts.host(0).source(), &third_source);
    assert_eq!(
        runtime.metadata[0].owner,
        mod_ui::PanelUiOwner::Environment(30)
    );
    assert_eq!(runtime.hosts.host(1).source(), &second_source);
    assert_eq!(
        runtime.metadata[1].owner,
        mod_ui::PanelUiOwner::Environment(20)
    );
    assert_eq!(runtime.hud_editor_owner.unwrap().host, 0);
    assert_eq!(runtime.screens.owner, Some(0));
    assert!(
        matches!(&runtime.screens.data_change, Some(ModEvent::Action { id, .. }) if id == "third")
    );
    assert!(runtime.render_sources.is_empty());

    runtime.remove_host(0);
    assert_eq!(runtime.hosts.host_count(), 1);
    assert_eq!(runtime.hosts.host(0).source(), &second_source);
    assert_eq!(
        runtime.metadata[0].owner,
        mod_ui::PanelUiOwner::Environment(20)
    );
    assert!(runtime.hud_editor_owner.is_none());
    assert!(runtime.screens.owner.is_none());
    assert!(runtime.screens.data_change.is_none());
}
