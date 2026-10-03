use super::*;
use crate::{
    app::{configure_client_frame_schedule, configure_client_production_frame_systems},
    runtime::world::{ClientWorld, drain_committed_ui_before_authority},
    ui_runtime::presentation::tests::fixture_font,
};
use bevy::ecs::schedule::{IntoSystemSet, NodeId, ScheduleGraph, Schedules};
use server_experience::{
    manifest::{Offer, Scope},
    negotiation::{Grant, VerifiedOffer},
    policy::WIRE_VERSION,
};
use std::collections::BTreeSet;

/// Finds a real production system node in Bevy's unbuilt schedule graph.
fn system_node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>) -> NodeId {
    let parent = NodeId::Set(
        graph
            .system_sets
            .get_key(system.into_system_set().intern())
            .unwrap(),
    );
    graph
        .systems
        .iter()
        .find(|(key, _, _)| {
            graph
                .hierarchy()
                .graph()
                .contains_edge(parent, NodeId::System(*key))
        })
        .map(|(key, _, _)| NodeId::System(key))
        .unwrap()
}

/// A verified offer whose signed expiry never passes.
fn offer() -> VerifiedOffer {
    VerifiedOffer {
        offer: Offer {
            version: WIRE_VERSION,
            audience: "127.0.0.1:19132".into(),
            server_key: String::new(),
            revision: 1,
            expires_unix: u64::MAX,
            scope: Scope {
                permissions: BTreeSet::new(),
                origins: BTreeSet::new(),
                memory_bytes: 0,
                gpu_bytes: 0,
            },
            packages: Vec::new(),
            fallback: String::new(),
            carrier: protocol::EXPERIENCE_CHANNEL.into(),
        },
        digest: String::new(),
    }
}

/// The controller over a closed menu, as on the join's loading screen, saving trust to a
/// private temporary file.
fn join_app(state: State) -> App {
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseButtonInput};
    let mut runtime = UiRuntime::new(1);
    runtime.experiences.session.state = state;
    let mut app = App::new();
    app.insert_resource(MenuRuntime::new(false, 2, "Test".into()))
        .insert_resource(runtime)
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(crate::ui_runtime::presentation::forms::tests::mini_engine_presentation())
        .insert_resource(NetworkHandle::disconnected())
        .init_resource::<ClientWorld>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>();
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        PrimaryWindow,
    ));
    configure(&mut app);
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut service = app.world_mut().resource_mut::<ExperienceService>();
    service.generation = 1;
    service.settings = Some(Settings::default());
    service.settings_path = std::env::temp_dir().join(format!(
        "cinnabar-experience-popup-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    app
}

/// Presents one frame, as the renderer does between controller updates.
fn present(app: &mut App) {
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    world.resource::<UiRuntime>(),
                    0,
                    [1280, 720],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        });
}

fn consent_owned(app: &App) -> bool {
    app.world()
        .resource::<super::super::input::ConsentInput>()
        .0
}

#[test]
fn join_offer_opens_the_trusted_popup_and_owns_input_without_a_menu() {
    let mut app = join_app(State::Offered(offer()));
    app.update();
    assert!(consent_owned(&app));
    present(&mut app);
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .experience_prompt_visible()
    );
}

#[test]
fn popup_keys_answer_the_offer_and_then_release_input() {
    use server_experience::trust::Decision;
    for (key, remembered) in [
        (KeyCode::Escape, None),
        (KeyCode::F8, Some(Decision::Never)),
    ] {
        let mut app = join_app(State::Offered(offer()));
        app.update();
        present(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        assert!(
            matches!(
                app.world()
                    .resource::<UiRuntime>()
                    .experiences
                    .session
                    .state,
                State::Disabled
            ),
            "{key:?} must answer the offer"
        );
        let service = app.world().resource::<ExperienceService>();
        let path = service.settings_path.clone();
        let decision = service
            .settings
            .as_ref()
            .unwrap()
            .decision(&offer().offer)
            .unwrap();
        assert_eq!(decision, remembered, "{key:?}");
        assert!(consent_owned(&app), "the answering frame stays consent's");
        app.update();
        assert!(
            !consent_owned(&app),
            "{key:?} must return input to the game"
        );
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn running_or_disabled_experiences_never_open_the_popup() {
    let grant = Grant {
        offer: offer(),
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: u64::MAX,
    };
    for state in [State::Disabled, State::Granted(grant)] {
        let mut app = join_app(state);
        app.world_mut()
            .resource_mut::<ExperienceService>()
            .attempted = true;
        app.update();
        present(&mut app);
        assert!(!consent_owned(&app));
        assert!(
            !app.world()
                .resource::<UiPresentationRuntime>()
                .experience_prompt_visible()
        );
    }
}

#[test]
fn only_an_unanswered_offer_holds_the_world_entry() {
    let mut extension = super::super::ExperienceSession::default();
    assert!(!extension.holds_world_entry());
    extension.marker = Some(std::sync::Arc::from(&b"{}"[..]));
    assert!(
        extension.holds_world_entry(),
        "an unread marker can still offer"
    );
    extension.marker = None;
    extension.handled_marker = true;
    assert!(!extension.holds_world_entry());
    extension.session.state = State::Offered(offer());
    assert!(extension.holds_world_entry());
    extension
        .session
        .choose(Choice::Once, &mut Settings::default(), 0)
        .unwrap();
    assert!(matches!(extension.session.state, State::Awaiting(_)));
    // Dragonfly reads the hello only after the client initializes, so the handshake must not wait.
    assert!(!extension.holds_world_entry());
}

#[test]
fn controller_follows_committed_drain_before_semantic_input() {
    let mut app = App::new();
    app.insert_resource(MenuRuntime::new(false, 2, "Test".into()));
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    configure(&mut app);
    let schedules = app.world().resource::<Schedules>();
    let graph = schedules.get(Update).unwrap().graph();
    let drain = NodeId::Set(
        graph
            .system_sets
            .get_key(
                drain_committed_ui_before_authority
                    .into_system_set()
                    .intern(),
            )
            .unwrap(),
    );
    assert!(
        graph
            .dependency()
            .graph()
            .contains_edge(drain, system_node(graph, drive))
    );
    let semantic = NodeId::Set(
        graph
            .system_sets
            .get_key(ClientFrameSet::SemanticSample.intern())
            .unwrap(),
    );
    assert!(
        graph
            .dependency()
            .graph()
            .contains_edge(system_node(graph, drive), semantic)
    );

    let mut schedules = app.world_mut().remove_resource::<Schedules>().unwrap();
    let result = schedules
        .get_mut(Update)
        .unwrap()
        .initialize(app.world_mut());
    app.world_mut().insert_resource(schedules);
    assert!(result.is_ok(), "experience schedule: {result:?}");
}

#[test]
fn committed_dimension_transition_revokes_live_runtime_in_the_same_frame() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut stream = client_world::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let old_epoch = stream.form_dimension_epoch();
    stream
        .submit(
            1,
            protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 70.0, 0.0],
            }),
        )
        .unwrap();
    stream.poll([0.0, 70.0, 0.0], 0);
    let new_epoch = stream.form_dimension_epoch();
    assert_ne!(old_epoch, new_epoch);
    let grant = Grant {
        offer: offer(),
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: u64::MAX,
    };
    let mut runtime = UiRuntime::new(1);
    runtime.experiences.epoch = old_epoch;
    runtime.experiences.active = true;
    runtime.experiences.session.state = State::Granted(grant.clone());
    let mut app = App::new();
    app.insert_resource(MenuRuntime::new(false, 2, "Test".into()))
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(NetworkHandle::disconnected())
        .insert_resource(ClientWorld {
            stream: Some(stream),
            ..Default::default()
        })
        .init_resource::<crate::environment::WorldClock>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(Update, drain_committed_ui_before_authority);
    configure_client_frame_schedule(&mut app);
    configure(&mut app);
    let mut service = app.world_mut().resource_mut::<ExperienceService>();
    service.generation = 1;
    service.attempted = true;
    service.live = Some(
        super::super::live::Live::start(grant, Vec::new(), old_epoch, 0, PathBuf::new().as_path())
            .unwrap(),
    );
    app.update();
    let extension = &app.world().resource::<UiRuntime>().experiences;
    assert_eq!(extension.epoch, new_epoch);
    assert!(matches!(extension.session.state, State::Disabled));
    assert!(!extension.active);
    assert!(app.world().resource::<ExperienceService>().live.is_none());
}

#[test]
fn unadvertised_experience_preserves_input_and_rendered_menu() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::forms::{pack_harness, snapshot};
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseButtonInput};
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let runtime = UiRuntime::new(1);
    let menu = MenuRuntime::new(true, 2, "Test".into());
    presentation.set_menu_view(Some(menu.view()));
    let before = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&before, "experience-unadvertised-before");
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(presentation)
        .insert_resource(NetworkHandle::disconnected())
        .init_resource::<ClientWorld>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>();
    configure(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        !app.world()
            .resource::<super::super::input::ConsentInput>()
            .0
    );
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left)
    );
    let service = app.world().resource::<ExperienceService>();
    assert!(service.settings.is_none());
    assert!(service.download.is_none());
    assert!(service.live.is_none());
    let runtime = app.world().resource::<UiRuntime>().clone();
    let after = app
        .world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&after, "experience-unadvertised-after");
    assert_eq!(snapshot::rasterize(&before), snapshot::rasterize(&after));
}
