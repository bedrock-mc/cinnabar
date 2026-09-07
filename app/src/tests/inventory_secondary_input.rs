use std::sync::Arc;

use bevy::{
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    math::Vec2,
    prelude::{App, IntoScheduleConfigs, MouseButton, Update},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window, WindowResolution},
};
use protocol::{
    CONTAINER_NAME_CURSOR, CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent,
    InventoryAuthority, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};
use semantic_input::Action;
use ui::UiPoint;

use crate::{
    app::{ClientFrameSet, configure_client_frame_schedule},
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    semantic_controls::{
        PendingDeviceFrame, SemanticInputRuntime, SemanticInputSnapshot, SemanticRouteState,
        SemanticTouchTargets, collect_raw_input, finalize_semantic_input_after_ui_authority,
        route_semantic_input, synchronize_semantic_input_authority,
    },
    settings_runtime::RuntimeSettings,
    ui_runtime::{
        UiRuntime, drive_chat_keyboard_input, drive_inventory_ui_actions,
        inventory_ledger::{
            GENERIC_STORAGE_WINDOW_TYPE, INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryPendingState,
            SMALL_STORAGE_SLOT_COUNT,
        },
        presentation::{
            UiPresentationRuntime, inventory_pointer::InventoryCellHit, tests::fixture_font,
        },
    },
};

const PHYSICAL_SIZE: [u32; 2] = [1280, 720];

#[test]
fn secondary_player_take_uses_ceiling_half_for_odd_even_and_single_stacks() {
    for (count, cursor_count, source_count) in [(33, 17, 16), (32, 16, 16), (1, 1, 0)] {
        let runtime = personal_runtime(Some(stack(count, 41)), None);
        let mut app = pointer_app(runtime, InventoryCellHit::Player(0), false, true);

        press(&mut app, MouseButton::Right);
        app.update();

        let ledger = app.world().resource::<UiRuntime>().inventory_ledger();
        assert_eq!(
            ledger.cursor_stack().map(|stack| stack.count),
            Some(cursor_count)
        );
        assert_eq!(
            ledger.displayed_stack(0).map(|stack| stack.count),
            (source_count != 0).then_some(source_count)
        );
        assert_eq!(
            ledger.pending_state(),
            Some(InventoryPendingState::AwaitingTransport)
        );
    }
}

#[test]
fn secondary_storage_take_uses_ceiling_half() {
    let runtime = storage_runtime(Some((2, stack(9, 51))), None);
    let mut app = pointer_app(runtime, InventoryCellHit::Storage(2), false, true);

    press(&mut app, MouseButton::Right);
    app.update();

    let ledger = app.world().resource::<UiRuntime>().inventory_ledger();
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(5));
    assert_eq!(ledger.storage_stack(2).map(|stack| stack.count), Some(4));
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
}

#[test]
fn secondary_places_one_into_empty_player_and_storage_cells() {
    let cases = [
        (
            personal_runtime(None, Some(stack(7, 61))),
            InventoryCellHit::Player(1),
        ),
        (
            storage_runtime(None, Some(stack(7, 62))),
            InventoryCellHit::Storage(3),
        ),
    ];
    for (runtime, target) in cases {
        let mut app = pointer_app(runtime, target, false, true);

        press(&mut app, MouseButton::Right);
        app.update();

        let ledger = app.world().resource::<UiRuntime>().inventory_ledger();
        assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(6));
        let placed = match target {
            InventoryCellHit::Player(slot) => ledger.displayed_stack(slot),
            InventoryCellHit::Storage(slot) => ledger.storage_stack(slot),
        };
        assert_eq!(placed.map(|stack| stack.count), Some(1));
        assert_eq!(
            ledger.pending_state(),
            Some(InventoryPendingState::AwaitingTransport)
        );
    }
}

#[test]
fn secondary_rejects_busy_empty_occupied_and_outside_targets() {
    let mut busy = pointer_app(
        personal_runtime(Some(stack(9, 71)), None),
        InventoryCellHit::Player(0),
        false,
        true,
    );
    press(&mut busy, MouseButton::Right);
    busy.update();
    let first_request = busy
        .world()
        .resource::<UiRuntime>()
        .inventory_ledger()
        .pending_request_id();
    press(&mut busy, MouseButton::Right);
    busy.update();
    let busy_ledger = busy.world().resource::<UiRuntime>().inventory_ledger();
    assert_eq!(busy_ledger.pending_request_id(), first_request);
    assert_eq!(busy_ledger.cursor_stack().map(|stack| stack.count), Some(5));

    let mut unknown = UiRuntime::new(1);
    unknown.publish_inventory_authority(InventoryAuthority::Server);
    unknown.publish_local_runtime_id(1, 42).unwrap();
    unknown.inventory_ledger_mut().apply(&cursor_content(None));
    unknown.toggle_inventory();
    assert!(unknown.inventory_ledger_mut().mark_transport_enqueued(0));
    unknown
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    let mut unknown = pointer_app(unknown, InventoryCellHit::Player(0), false, true);
    press(&mut unknown, MouseButton::Right);
    unknown.update();
    assert_eq!(
        unknown
            .world()
            .resource::<UiRuntime>()
            .inventory_ledger()
            .pending_state(),
        None
    );

    let mut recovering = personal_runtime(Some(stack(9, 75)), None);
    assert!(
        recovering
            .inventory_ledger_mut()
            .begin_take_count(0, 5)
            .is_ok()
    );
    assert!(recovering.inventory_ledger_mut().mark_transport_enqueued(0));
    recovering.poll_inventory_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert!(recovering.inventory_ledger().resync_required());
    let mut recovering = pointer_app(recovering, InventoryCellHit::Player(0), false, true);
    press(&mut recovering, MouseButton::Right);
    recovering.update();
    let recovering_ledger = recovering
        .world()
        .resource::<UiRuntime>()
        .inventory_ledger();
    assert!(recovering_ledger.resync_required());
    assert_eq!(recovering_ledger.pending_state(), None);
    assert_eq!(recovering_ledger.cursor_stack(), None);

    for runtime in [
        personal_runtime(None, None),
        personal_runtime(Some(stack(4, 72)), Some(stack(3, 73))),
    ] {
        let mut app = pointer_app(runtime, InventoryCellHit::Player(0), false, true);
        press(&mut app, MouseButton::Right);
        app.update();
        assert_eq!(
            app.world()
                .resource::<UiRuntime>()
                .inventory_ledger()
                .pending_state(),
            None
        );
    }

    let mut outside = pointer_app_at(
        personal_runtime(Some(stack(9, 74)), None),
        Vec2::ZERO,
        false,
        true,
    );
    press(&mut outside, MouseButton::Right);
    outside.update();
    assert_eq!(
        outside
            .world()
            .resource::<UiRuntime>()
            .inventory_ledger()
            .pending_state(),
        None
    );
    assert!(
        !outside
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
}

#[test]
fn simultaneous_primary_and_secondary_preserves_primary_operation() {
    let runtime = personal_runtime(Some(stack(9, 81)), None);
    let mut app = pointer_app(runtime, InventoryCellHit::Player(0), false, true);

    press(&mut app, MouseButton::Left);
    press(&mut app, MouseButton::Right);
    app.update();

    let ledger = app.world().resource::<UiRuntime>().inventory_ledger();
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(9));
    assert_eq!(ledger.displayed_stack(0), None);
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
}

#[test]
fn secondary_is_consumed_before_gameplay_use() {
    let runtime = personal_runtime(Some(stack(9, 91)), None);
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let pointer = hit_point(&presentation, InventoryCellHit::Player(0), None);
    let (mut app, _) = semantic_app(runtime, presentation, pointer);

    press(&mut app, MouseButton::Right);
    app.update();

    assert_eq!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Use),
        Default::default()
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right)
    );
    assert_eq!(
        app.world()
            .resource::<UiRuntime>()
            .inventory_ledger()
            .cursor_stack()
            .map(|stack| stack.count),
        Some(5)
    );
}

#[test]
fn inventory_open_and_close_transitions_suppress_secondary_edges() {
    for initially_open in [false, true] {
        let mut runtime = if initially_open {
            personal_runtime(Some(stack(9, 101)), None)
        } else {
            let mut runtime = UiRuntime::new(1);
            runtime.publish_inventory_authority(InventoryAuthority::Server);
            runtime.publish_local_runtime_id(1, 42).unwrap();
            runtime
                .inventory_ledger_mut()
                .apply(&player_content(Some(stack(9, 101))));
            runtime
        };
        if !initially_open {
            runtime.inventory_ledger_mut().apply(&cursor_content(None));
        }
        let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let pointer = hit_point(&presentation, InventoryCellHit::Player(0), None);
        let (mut app, window) = semantic_app(runtime, presentation, pointer);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        press(&mut app, MouseButton::Right);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyE,
            logical_key: Key::Character("e".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();

        let runtime = app.world().resource::<UiRuntime>();
        assert_eq!(runtime.inventory_open(), !initially_open);
        assert_eq!(runtime.inventory_ledger().pending_state(), None);
        assert_eq!(runtime.inventory_ledger().cursor_stack(), None);
        assert_eq!(
            app.world()
                .resource::<SemanticInputSnapshot>()
                .phase(Action::Use),
            Default::default()
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Right)
        );
    }
}

#[test]
fn menu_and_focus_loss_preempt_secondary_inventory_dispatch() {
    for (menu_visible, focused) in [(true, true), (false, false)] {
        let runtime = personal_runtime(Some(stack(9, 111)), None);
        let mut app = pointer_app(runtime, InventoryCellHit::Player(0), menu_visible, focused);
        press(&mut app, MouseButton::Right);
        app.update();

        let runtime = app.world().resource::<UiRuntime>();
        assert!(!runtime.inventory_open());
        assert_eq!(runtime.inventory_ledger().pending_state(), None);
        assert_eq!(runtime.inventory_ledger().cursor_stack(), None);
    }
}

fn pointer_app(
    runtime: UiRuntime,
    target: InventoryCellHit,
    menu_visible: bool,
    focused: bool,
) -> App {
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let storage_slots = runtime.inventory_ledger().storage_slot_count();
    let pointer = hit_point(&presentation, target, storage_slots);
    pointer_app_with(runtime, presentation, pointer, menu_visible, focused)
}

fn pointer_app_at(runtime: UiRuntime, pointer: Vec2, menu_visible: bool, focused: bool) -> App {
    pointer_app_with(
        runtime,
        UiPresentationRuntime::new(fixture_font()).unwrap(),
        pointer,
        menu_visible,
        focused,
    )
}

fn pointer_app_with(
    runtime: UiRuntime,
    presentation: UiPresentationRuntime,
    pointer: Vec2,
    menu_visible: bool,
    focused: bool,
) -> App {
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .add_message::<KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(presentation)
        .insert_resource(MenuRuntime::new(menu_visible, 2, "Tester".to_owned()))
        .add_systems(
            Update,
            (
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
            )
                .chain(),
        );
    app.world_mut().spawn((
        window(pointer, focused),
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app
}

fn semantic_app(
    runtime: UiRuntime,
    presentation: UiPresentationRuntime,
    pointer: Vec2,
) -> (App, bevy::prelude::Entity) {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<RuntimeSettings>()
        .add_message::<KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(presentation)
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .add_systems(Update, collect_raw_input.in_set(ClientFrameSet::RawInput))
        .add_systems(
            Update,
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
        )
        .add_systems(
            Update,
            (
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
                synchronize_semantic_input_authority,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        );
    let window = app
        .world_mut()
        .spawn((
            window(pointer, true),
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

fn personal_runtime(
    source: Option<NetworkItemStack>,
    cursor: Option<NetworkItemStack>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    runtime.publish_local_runtime_id(1, 42).unwrap();
    runtime
        .inventory_ledger_mut()
        .apply(&player_content(source));
    runtime
        .inventory_ledger_mut()
        .apply(&cursor_content(cursor));
    runtime.toggle_inventory();
    assert!(runtime.inventory_open());
    assert!(runtime.inventory_ledger_mut().mark_transport_enqueued(0));
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    runtime
}

fn storage_runtime(
    source: Option<(usize, NetworkItemStack)>,
    cursor: Option<NetworkItemStack>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    runtime.inventory_ledger_mut().apply(&player_content(None));
    runtime
        .inventory_ledger_mut()
        .apply(&cursor_content(cursor));
    let mut slots = vec![NetworkItemStack::default(); SMALL_STORAGE_SLOT_COUNT];
    if let Some((slot, stack)) = source {
        slots[slot] = stack;
    }
    for (sequence, event) in [
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [1, 64, 1],
            runtime_entity_id: -1,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(7),
                slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
                dynamic_id: Some(91),
            },
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::default(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory();
    assert!(runtime.inventory_open());
    runtime
}

fn player_content(source: Option<NetworkItemStack>) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::default(); 36];
    if let Some(source) = source {
        slots[0] = source;
    }
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

fn cursor_content(cursor: Option<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([cursor.unwrap_or_default()]),
        storage_item: NetworkItemStack::default(),
    })
}

fn stack(count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn window(pointer: Vec2, focused: bool) -> Window {
    let mut window = Window {
        focused,
        resolution: WindowResolution::new(PHYSICAL_SIZE[0], PHYSICAL_SIZE[1]),
        ..Default::default()
    };
    window.set_cursor_position(Some(pointer));
    window
}

fn hit_point(
    presentation: &UiPresentationRuntime,
    target: InventoryCellHit,
    storage_slots: Option<usize>,
) -> Vec2 {
    (0..PHYSICAL_SIZE[1])
        .find_map(|y| {
            (0..PHYSICAL_SIZE[0]).find_map(|x| {
                let point = UiPoint::new(x as f32, y as f32).unwrap();
                let gui = presentation.inventory_gui_point(point, PHYSICAL_SIZE, 1.0)?;
                (presentation.inventory_cell_hit(gui, PHYSICAL_SIZE, 1.0, storage_slots)
                    == Some(target))
                .then_some(Vec2::new(x as f32, y as f32))
            })
        })
        .unwrap_or_else(|| panic!("{target:?} has a physical hit point"))
}

fn press(app: &mut App, button: MouseButton) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
}
