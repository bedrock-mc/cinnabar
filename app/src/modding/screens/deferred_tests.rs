//! Deferred edits through the production frame and screen adapters.

use super::*;
use crate::modding::{begin_mod_frame, drive_mod, install, interaction::ModInteraction};
use mod_host::CallbackOutcome;
use server_experience::screen::{GuiSize, Rect, ScreenLayout, Value};

/// Admits a fixture under the frame driver's initial fence before staging screen events.
pub(super) fn load_driver_host(path: &std::path::Path) -> ModHost {
    let mut host = ModHost::load_package(path, mod_host::ModGrants::default()).unwrap();
    host.bind_drawing_session(mod_api::drawing::DrawingSession::default());
    assert!(host.reload_if_changed().unwrap());
    assert!(host.is_active());
    host
}

/// Makes an old container scope without requiring an installed UI carrier.
fn old_layout() -> ScreenLayout {
    ScreenLayout {
        screen: "departed.screen".into(),
        size: GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        gui: Rect::default(),
        exclusions: Vec::new(),
        view: Some(Rect::default()),
    }
}

/// Installs the real frame driver with a known previously delivered screen scope.
fn install_driver(app: &mut App, hosts: Vec<ModHost>, owner: usize, layout: ScreenLayout) {
    let mut configured = App::new();
    install(&mut configured, hosts);
    let mut runtime = configured
        .world_mut()
        .remove_resource::<ModRuntime>()
        .unwrap();
    runtime.screens = ScreenState {
        owner: Some(owner),
        layout: Some(layout),
        ..Default::default()
    };
    app.insert_resource(runtime)
        .insert_resource(crate::environment::VisualTimeOverride(None))
        .init_resource::<ModInteraction>()
        .init_resource::<crate::modding::packet_render::Drawing>()
        .add_systems(Update, (begin_mod_frame, drive_mod).chain());
}

/// Exhausts the shared event lane and leaves a real edit waiting for the next frame.
fn defer_edit(app: &mut App, owner: usize) {
    let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
    runtime.hosts.begin_frame();
    let host = runtime.hosts.host_mut(owner);
    assert_eq!(
        host.dispatch(vec![ModEvent::Action {
            id: "probe.read_all".into(),
            index: None,
        }])
        .unwrap(),
        CallbackOutcome::Executed
    );
    assert_eq!(
        host.dispatch(vec![ModEvent::TextChanged {
            control: "probe.search".into(),
            text: "pending".into(),
        }])
        .unwrap(),
        CallbackOutcome::Deferred
    );
    assert!(!host.screens().data.values.contains_key("#search"));
}

#[test]
fn frame_driver_cancels_deferred_edits_when_the_container_or_owner_departs() {
    for change_owner in [false, true] {
        let (mut app, _) = input_app();
        let dir = tempfile::tempdir().unwrap();
        write_transition_probe(dir.path(), "0.1.0");
        let mut previous = load_driver_host(dir.path());
        let layout = old_layout();
        previous
            .dispatch(vec![
                ModEvent::ScreenChanged(Some(layout.clone())),
                ModEvent::Action {
                    id: "probe.view".into(),
                    index: None,
                },
            ])
            .unwrap();
        let mut hosts = Vec::new();
        if change_owner {
            hosts.push(load_driver_host(dir.path()));
        }
        let owner = hosts.len();
        hosts.push(previous);
        install_driver(&mut app, hosts, owner, layout);
        defer_edit(&mut app, owner);
        for _ in 0..3 {
            app.update();
            let runtime = app.world().resource::<ModRuntime>();
            let host = runtime.hosts.host(owner);
            assert!(host.is_active());
            assert!(!host.screens().data.values.contains_key("#search"));
            assert!(host.screens().view.is_none());
        }
        assert_eq!(
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(owner)
                .screens()
                .data
                .values
                .get("#layout_open"),
            Some(&Value::Bool(false))
        );
    }
}

#[test]
fn frame_driver_replays_edits_only_while_their_screen_and_view_remain() {
    for transition in ["unchanged", "screen", "escape"] {
        let (mut app, window) = input_app();
        let dir = tempfile::tempdir().unwrap();
        write_transition_probe(dir.path(), "0.1.0");
        let mut host = load_driver_host(dir.path());
        let package = host.package().unwrap();
        if !draw_package_view(&mut app, &package.id, &package.files) {
            return;
        }
        let mut layout = app
            .world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_screen_layout()
            .cloned()
            .unwrap();
        if transition == "screen" {
            layout.screen = "departed.screen".into();
        }
        host.dispatch(vec![
            ModEvent::ScreenChanged(Some(layout.clone())),
            ModEvent::Action {
                id: "probe.view".into(),
                index: None,
            },
        ])
        .unwrap();
        install_driver(&mut app, vec![host], 0, layout);
        defer_edit(&mut app, 0);
        if transition == "escape" {
            queue_key(
                &mut app,
                window,
                KeyCode::Escape,
                ButtonState::Pressed,
                None,
            );
        }
        for _ in 0..3 {
            app.update();
        }
        let runtime = app.world().resource::<ModRuntime>();
        let host = runtime.hosts.host(0);
        assert!(host.is_active());
        let text = host.screens().data.values.get("#search");
        if transition == "unchanged" {
            assert_eq!(text, Some(&Value::Text("probe.search=pending".into())));
        } else {
            assert_eq!(
                text, None,
                "{transition} delivered an edit from the departed scope"
            );
        }
        if transition == "escape" {
            assert!(host.screens().view.is_none());
        }
    }
}
