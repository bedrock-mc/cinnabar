use std::sync::Arc;

use bevy::{
    ecs::schedule::{IntoSystemSet, NodeId, ScheduleGraph, Schedules, SystemSet},
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    prelude::{App, IntoScheduleConfigs, MouseButton, ResMut, Resource, Update},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window},
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};

use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
use crate::{
    app::{
        ClientFrameSet, configure_client_frame_schedule, configure_client_production_frame_systems,
    },
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    runtime::network::receive_network_events,
    ui_runtime::{
        UiRuntime, drain_inventory_authority, drive_chat_keyboard_input,
        drive_inventory_ui_actions, flush_inventory_network, flush_inventory_send,
        inventory_ledger::InventoryPendingState,
        presentation::{UiPresentationRuntime, tests::fixture_font},
    },
};
use ui::UiPoint;

#[test]
fn production_schedule_drains_content_before_click_and_admits_only_in_network_send() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();

    assert_system_in_stage(
        graph,
        drain_inventory_authority,
        "drain_inventory_authority",
        ClientFrameSet::UiAuthority,
    );
    assert!(graph.dependency().graph().contains_edge(
        system_node(graph, receive_network_events, "receive_network_events"),
        system_set_node(
            drain_inventory_authority,
            graph,
            "drain_inventory_authority",
        ),
    ));
    assert_system_in_stage(
        graph,
        drive_inventory_ui_actions,
        "drive_inventory_ui_actions",
        ClientFrameSet::UiAuthority,
    );
    assert_system_in_stage(
        graph,
        drive_chat_keyboard_input,
        "drive_chat_keyboard_input",
        ClientFrameSet::UiAuthority,
    );
    assert_system_in_stage(
        graph,
        flush_inventory_network,
        "flush_inventory_network",
        ClientFrameSet::NetworkSend,
    );
    assert!(graph.dependency().graph().contains_edge(
        system_node(
            graph,
            drain_inventory_authority,
            "drain_inventory_authority"
        ),
        system_node(
            graph,
            drive_chat_keyboard_input,
            "drive_chat_keyboard_input"
        ),
    ));
    assert!(graph.dependency().graph().contains_edge(
        system_node(
            graph,
            drive_chat_keyboard_input,
            "drive_chat_keyboard_input"
        ),
        system_node(graph, drive_menu_input, "drive_menu_input"),
    ));
    assert!(graph.dependency().graph().contains_edge(
        system_node(graph, drive_menu_input, "drive_menu_input"),
        system_node(
            graph,
            drive_inventory_ui_actions,
            "drive_inventory_ui_actions"
        ),
    ));
    assert!(graph.dependency().graph().contains_edge(
        stage_node(graph, ClientFrameSet::UiPublication),
        stage_node(graph, ClientFrameSet::NetworkSend),
    ));
}

#[test]
fn supported_open_suppresses_gameplay_before_content_and_escape_closes_before_menu() {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .add_message::<KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .insert_resource(SameFrameIngress(Some(InventoryEvent::Open(
            ContainerOpenEvent {
                container: ContainerIdentity::window(1),
                window_type: 0,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ))))
        .init_resource::<OpenSuppressionObserved>()
        .add_systems(
            Update,
            (
                route_same_frame_inventory_ingress,
                drain_inventory_authority,
                observe_open_suppression,
                drive_chat_keyboard_input,
                drive_menu_input,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        );
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });

    app.update();

    assert!(app.world().resource::<OpenSuppressionObserved>().0);
    assert!(!app.world().resource::<UiRuntime>().inventory_open());
    assert!(
        app.world()
            .resource::<UiRuntime>()
            .inventory_ledger()
            .pending_packet()
            .unwrap()
            .is_some()
    );
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn scheduled_mouse_press_uses_same_frame_complete_and_partial_inventory_ingress() {
    for complete in [true, false] {
        run_scheduled_ingress_click(complete);
    }
}

#[test]
fn pause_opening_escape_is_not_replayed_as_back_on_the_next_frame() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .add_message::<KeyboardInput>()
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn same_frame_storage_open_and_content_drive_real_button_input_before_network_send() {
    let current = stack(6, 3, 91);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let physical_size = [1280, 720];
    let pointer = (0..physical_size[1])
        .find_map(|y| {
            (0..physical_size[0]).find_map(|x| {
                let point = UiPoint::new(x as f32, y as f32).unwrap();
                let gui = presentation.inventory_gui_point(point, physical_size, 1.0)?;
                (presentation.inventory_cell_hit(gui, physical_size, 1.0, Some(27))
                    == Some(InventoryCellHit::Storage(2)))
                .then_some(bevy::math::Vec2::new(x as f32, y as f32))
            })
        })
        .expect("storage slot two has a physical hit point");
    let mut window = Window {
        focused: true,
        resolution: bevy::window::WindowResolution::new(1280, 720),
        ..Default::default()
    };
    window.set_cursor_position(Some(pointer));
    let mut slots = vec![NetworkItemStack::default(); 27];
    slots[2] = current.clone();
    let ingress = vec![
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(1),
            window_type: 0,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(1),
                slot_type: Some(7),
                dynamic_id: None,
            },
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::default(),
        }),
    ];

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(presentation)
        .insert_resource(SameFrameStorageIngress(ingress))
        .init_resource::<AdmissionObserved>()
        .add_systems(
            Update,
            (
                route_same_frame_storage_ingress,
                drain_inventory_authority,
                drive_inventory_ui_actions,
                drive_chat_keyboard_input,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            admit_inventory_request.in_set(ClientFrameSet::NetworkSend),
        );
    app.world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();

    assert!(app.world().resource::<AdmissionObserved>().0);
    let runtime = app.world().resource::<UiRuntime>();
    assert!(runtime.inventory_open());
    assert_eq!(runtime.inventory_ledger().cursor_stack(), Some(&current));
    assert_eq!(
        runtime.inventory_ledger().pending_state(),
        Some(InventoryPendingState::AwaitingResponse)
    );
}

fn run_scheduled_ingress_click(complete: bool) {
    let stale = stack(5, 1, 44);
    let current = stack(6, 2, 55);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    runtime.inventory_ledger_mut().apply(&content(stale));
    runtime.toggle_inventory();

    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let physical_size = [1280, 720];
    let pointer = (0..physical_size[1])
        .find_map(|y| {
            (0..physical_size[0]).find_map(|x| {
                let point = UiPoint::new(x as f32, y as f32).unwrap();
                let gui = presentation.inventory_gui_point(point, physical_size, 1.0)?;
                (presentation.inventory_slot_hit(gui, physical_size, 1.0) == Some(0))
                    .then_some(bevy::math::Vec2::new(x as f32, y as f32))
            })
        })
        .expect("slot zero has a physical hit point");
    let mut window = Window {
        focused: true,
        resolution: bevy::window::WindowResolution::new(1280, 720),
        ..Default::default()
    };
    window.set_cursor_position(Some(pointer));

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(presentation)
        .insert_resource(SameFrameIngress(Some(if complete {
            content(current.clone())
        } else {
            partial_content(current.clone())
        })))
        .init_resource::<AdmissionObserved>()
        .add_systems(
            Update,
            (
                route_same_frame_inventory_ingress,
                drain_inventory_authority,
                drive_inventory_ui_actions,
                drive_chat_keyboard_input,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            admit_inventory_request.in_set(ClientFrameSet::NetworkSend),
        );
    app.world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    app.update();

    assert!(app.world().resource::<AdmissionObserved>().0);
    let runtime = app.world().resource::<UiRuntime>();
    assert_eq!(runtime.inventory_ledger().cursor_stack(), Some(&current));
    assert_eq!(
        runtime.inventory_ledger().pending_state(),
        Some(InventoryPendingState::AwaitingResponse)
    );
}

#[derive(Resource)]
struct SameFrameIngress(Option<InventoryEvent>);

#[derive(Resource)]
struct SameFrameStorageIngress(Vec<InventoryEvent>);

#[derive(Default, Resource)]
struct AdmissionObserved(bool);

#[derive(Default, Resource)]
struct OpenSuppressionObserved(bool);

fn observe_open_suppression(
    runtime: bevy::prelude::Res<UiRuntime>,
    mut observed: ResMut<OpenSuppressionObserved>,
) {
    observed.0 = runtime.inventory_open() && runtime.ui_focused();
}

fn route_same_frame_inventory_ingress(
    mut ingress: ResMut<SameFrameIngress>,
    mut runtime: ResMut<UiRuntime>,
) {
    if let Some(event) = ingress.0.take() {
        runtime.enqueue_inventory_event(1, 1, event).unwrap();
    }
}

fn route_same_frame_storage_ingress(
    mut ingress: ResMut<SameFrameStorageIngress>,
    mut runtime: ResMut<UiRuntime>,
) {
    for (index, event) in ingress.0.drain(..).enumerate() {
        runtime
            .enqueue_inventory_event(1, index as u64 + 1, event)
            .unwrap();
    }
}

fn admit_inventory_request(
    mut runtime: ResMut<UiRuntime>,
    mut observed: ResMut<AdmissionObserved>,
) {
    observed.0 = flush_inventory_send(&mut runtime, 10, |_| Ok::<_, ()>(())).unwrap();
}

fn content(first: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(
            (0..36)
                .map(|index| {
                    if index == 0 {
                        first.clone()
                    } else {
                        NetworkItemStack::default()
                    }
                })
                .collect::<Vec<_>>(),
        ),
        storage_item: NetworkItemStack::default(),
    })
}

fn partial_content(first: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from([first]),
        storage_item: NetworkItemStack::default(),
    })
}

fn stack(network_id: i32, count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn stage_node(graph: &ScheduleGraph, stage: ClientFrameSet) -> NodeId {
    let key = graph
        .system_sets
        .get_key(stage.intern())
        .expect("production stage");
    NodeId::Set(key)
}

fn assert_system_in_stage<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
    stage: ClientFrameSet,
) {
    assert!(
        graph
            .hierarchy()
            .graph()
            .contains_edge(stage_node(graph, stage), system_node(graph, system, label),)
    );
}

fn system_set_node<M>(system: impl IntoSystemSet<M>, graph: &ScheduleGraph, label: &str) -> NodeId {
    let key = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing {label}"));
    NodeId::Set(key)
}

fn system_node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>, label: &str) -> NodeId {
    let key = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing {label}"));
    let parent = NodeId::Set(key);
    graph
        .systems
        .iter()
        .find_map(|(key, _, _)| {
            let child = NodeId::System(key);
            graph
                .hierarchy()
                .graph()
                .contains_edge(parent, child)
                .then_some(child)
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}
