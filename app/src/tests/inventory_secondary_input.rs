use crate::player_runtime::PlayerRuntime;
use std::sync::Arc;

use bevy::{
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput, NativeKey},
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    math::Vec2,
    prelude::{App, Entity, IntoScheduleConfigs, MouseButton, Update, With},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window, WindowResolution},
};
use protocol::{
    CONTAINER_NAME_CURSOR, CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent,
    InventoryAuthority, InventoryContentEvent, InventoryEvent, ItemRegistryEntry,
    ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
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
fn primary_merges_an_occupied_compatible_stack_up_to_capacity() {
    let mut player_runtime = PlayerRuntime::new(1);

    let mut runtime = personal_runtime(
        &mut player_runtime,
        Some(stack(60, 60)),
        Some(stack(33, 33)),
    );
    apply_apple_registry(&mut player_runtime, &mut runtime);
    let mut app = pointer_app(
        runtime,
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );

    press(&mut app, MouseButton::Left);
    app.update();
    release_and_update(&mut app, MouseButton::Left);

    let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(64));
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(29));
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );

    let mut full = personal_runtime(&mut player_runtime, Some(stack(64, 60)), Some(stack(1, 33)));
    apply_apple_registry(&mut player_runtime, &mut full);
    let mut full = pointer_app(
        full,
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press(&mut full, MouseButton::Left);
    full.update();
    release_and_update(&mut full, MouseButton::Left);
    let ledger = full.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(ledger.pending_state(), None);
    assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(64));
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(1));
}

#[test]
fn secondary_places_one_into_occupied_compatible_stack_without_gameplay_use() {
    let mut player_runtime = PlayerRuntime::new(1);

    let mut runtime = personal_runtime(
        &mut player_runtime,
        Some(stack(60, 60)),
        Some(stack(33, 33)),
    );
    apply_apple_registry(&mut player_runtime, &mut runtime);
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let pointer = hit_point(&presentation, InventoryCellHit::Player(0), None);
    let (mut app, _) = semantic_app(runtime, presentation, pointer, &player_runtime);

    press(&mut app, MouseButton::Right);
    app.update();
    release_and_update(&mut app, MouseButton::Right);

    let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(61));
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
    assert_eq!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Use),
        Default::default()
    );
}

#[test]
fn secondary_player_take_uses_ceiling_half_for_odd_even_and_single_stacks() {
    let mut player_runtime = PlayerRuntime::new(1);

    for (count, cursor_count, source_count) in [(33, 17, 16), (32, 16, 16), (1, 1, 0)] {
        let runtime = personal_runtime(&mut player_runtime, Some(stack(count, 41)), None);
        let mut app = pointer_app(
            runtime,
            InventoryCellHit::Player(0),
            false,
            true,
            &player_runtime,
        );

        press(&mut app, MouseButton::Right);
        app.update();

        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
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
    let mut player_runtime = PlayerRuntime::new(1);

    let runtime = storage_runtime(&mut player_runtime, Some((2, stack(9, 51))), None);
    let mut app = pointer_app(
        runtime,
        InventoryCellHit::Storage(2),
        false,
        true,
        &player_runtime,
    );

    press(&mut app, MouseButton::Right);
    app.update();

    let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(5));
    assert_eq!(ledger.storage_stack(2).map(|stack| stack.count), Some(4));
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
}

#[test]
fn secondary_places_one_into_empty_player_and_storage_cells() {
    let mut player_runtime = PlayerRuntime::new(1);

    for target in [InventoryCellHit::Player(1), InventoryCellHit::Storage(3)] {
        let runtime = match target {
            InventoryCellHit::Player(_) => {
                personal_runtime(&mut player_runtime, None, Some(stack(7, 61)))
            }
            InventoryCellHit::Storage(_) => {
                storage_runtime(&mut player_runtime, None, Some(stack(7, 62)))
            }
            _ => unreachable!(),
        };
        let mut app = pointer_app(runtime, target, false, true, &player_runtime);

        press(&mut app, MouseButton::Right);
        app.update();
        release_and_update(&mut app, MouseButton::Right);

        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(6));
        let placed = match target {
            InventoryCellHit::Player(slot) => ledger.displayed_stack(slot),
            InventoryCellHit::Storage(slot) => ledger.storage_stack(slot),
            _ => unreachable!("cases target player and storage cells"),
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
    let mut player_runtime = PlayerRuntime::new(1);

    let mut busy = pointer_app(
        personal_runtime(&mut player_runtime, Some(stack(9, 71)), None),
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press(&mut busy, MouseButton::Right);
    busy.update();
    let first_request = busy
        .world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger()
        .pending_request_id();
    press(&mut busy, MouseButton::Right);
    busy.update();
    // The unsettled split shares one server id, so it cannot be named yet.
    let busy_ledger = busy.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(busy_ledger.pending_request_id(), first_request);
    assert_eq!(busy_ledger.pending_request_count(), 1);
    assert_eq!(busy_ledger.cursor_stack().map(|stack| stack.count), Some(5));

    player_runtime = PlayerRuntime::new(1);
    let mut unknown = UiRuntime::new(1);
    unknown.publish_inventory_authority(&mut player_runtime, InventoryAuthority::Server);
    unknown
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    unknown
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&cursor_content(None));
    unknown.toggle_inventory(&mut player_runtime);
    assert!(
        unknown
            .inventory_ledger_mut(&mut player_runtime)
            .mark_transport_enqueued(0)
    );
    unknown
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    let mut unknown = pointer_app(
        unknown,
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press(&mut unknown, MouseButton::Right);
    unknown.update();
    assert_eq!(
        unknown
            .world()
            .resource::<PlayerRuntime>()
            .inventory
            .ledger()
            .pending_state(),
        None
    );

    let mut recovering = personal_runtime(&mut player_runtime, Some(stack(9, 75)), None);
    assert!(
        recovering
            .inventory_ledger_mut(&mut player_runtime)
            .begin_take_count(0, 5)
            .is_ok()
    );
    assert!(
        recovering
            .inventory_ledger_mut(&mut player_runtime)
            .mark_transport_enqueued(0)
    );
    recovering.poll_inventory_timeout(&mut player_runtime, INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert!(
        recovering
            .inventory_ledger(&player_runtime)
            .resync_required()
    );
    let mut recovering = pointer_app(
        recovering,
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press(&mut recovering, MouseButton::Right);
    recovering.update();
    let recovering_ledger = recovering
        .world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger();
    // The timed-out prediction stays and blocks new gestures until refresh.
    assert!(recovering_ledger.resync_required());
    assert_eq!(recovering_ledger.pending_request_count(), 1);
    assert_eq!(
        recovering_ledger.cursor_stack().map(|stack| stack.count),
        Some(5)
    );

    for (source, cursor) in [(None, None), (Some(stack(4, 72)), Some(stack(3, 73)))] {
        let runtime = personal_runtime(&mut player_runtime, source, cursor);
        let mut app = pointer_app(
            runtime,
            InventoryCellHit::Player(0),
            false,
            true,
            &player_runtime,
        );
        press(&mut app, MouseButton::Right);
        app.update();
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .pending_state(),
            None
        );
    }

    let mut outside = pointer_app_at(
        personal_runtime(&mut player_runtime, Some(stack(9, 74)), None),
        Vec2::ZERO,
        false,
        true,
        &player_runtime,
    );
    press(&mut outside, MouseButton::Right);
    outside.update();
    assert_eq!(
        outside
            .world()
            .resource::<PlayerRuntime>()
            .inventory
            .ledger()
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
    let mut player_runtime = PlayerRuntime::new(1);

    let runtime = personal_runtime(&mut player_runtime, Some(stack(9, 81)), None);
    let mut app = pointer_app(
        runtime,
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );

    press(&mut app, MouseButton::Left);
    press(&mut app, MouseButton::Right);
    app.update();

    let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(9));
    assert_eq!(ledger.displayed_stack(0), None);
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
}

#[test]
fn secondary_is_consumed_before_gameplay_use() {
    let mut player_runtime = PlayerRuntime::new(1);

    let runtime = personal_runtime(&mut player_runtime, Some(stack(9, 91)), None);
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let pointer = hit_point(&presentation, InventoryCellHit::Player(0), None);
    let (mut app, _) = semantic_app(runtime, presentation, pointer, &player_runtime);

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
            .resource::<PlayerRuntime>()
            .inventory
            .ledger()
            .cursor_stack()
            .map(|stack| stack.count),
        Some(5)
    );
}

#[test]
fn inventory_open_and_close_transitions_suppress_secondary_edges() {
    let mut player_runtime = PlayerRuntime::new(1);

    for initially_open in [false, true] {
        let mut runtime = if initially_open {
            personal_runtime(&mut player_runtime, Some(stack(9, 101)), None)
        } else {
            let mut runtime = UiRuntime::new(1);
            runtime.publish_inventory_authority(&mut player_runtime, InventoryAuthority::Server);
            runtime
                .publish_local_runtime_id(&mut player_runtime, 1, 42)
                .unwrap();
            runtime
                .inventory_ledger_mut(&mut player_runtime)
                .apply(&player_content(Some(stack(9, 101))));
            runtime
        };
        if !initially_open {
            runtime
                .inventory_ledger_mut(&mut player_runtime)
                .apply(&cursor_content(None));
        }
        let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let pointer = hit_point(&presentation, InventoryCellHit::Player(0), None);
        let (mut app, window) = semantic_app(runtime, presentation, pointer, &player_runtime);

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
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .pending_state(),
            None
        );
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .cursor_stack(),
            None
        );
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
    let mut player_runtime = PlayerRuntime::new(1);

    for (menu_visible, focused) in [(true, true), (false, false)] {
        let runtime = personal_runtime(&mut player_runtime, Some(stack(9, 111)), None);
        let mut app = pointer_app(
            runtime,
            InventoryCellHit::Player(0),
            menu_visible,
            focused,
            &player_runtime,
        );
        press(&mut app, MouseButton::Right);
        app.update();

        let runtime = app.world().resource::<UiRuntime>();
        assert!(!runtime.inventory_open());
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .pending_state(),
            None
        );
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .cursor_stack(),
            None
        );
    }
}

fn pointer_app(
    runtime: UiRuntime,
    target: InventoryCellHit,
    menu_visible: bool,
    focused: bool,
    player_runtime: &PlayerRuntime,
) -> App {
    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let storage_slots = runtime
        .inventory_ledger(player_runtime)
        .storage_slot_count();
    let pointer = hit_point(&presentation, target, storage_slots);
    pointer_app_with(
        runtime,
        presentation,
        pointer,
        menu_visible,
        focused,
        player_runtime,
    )
}

fn pointer_app_at(
    runtime: UiRuntime,
    pointer: Vec2,
    menu_visible: bool,
    focused: bool,
    player_runtime: &PlayerRuntime,
) -> App {
    pointer_app_with(
        runtime,
        UiPresentationRuntime::new(fixture_font()).unwrap(),
        pointer,
        menu_visible,
        focused,
        player_runtime,
    )
}

fn pointer_app_with(
    runtime: UiRuntime,
    presentation: UiPresentationRuntime,
    pointer: Vec2,
    menu_visible: bool,
    focused: bool,
    player_runtime: &PlayerRuntime,
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
        .insert_resource(player_runtime.clone())
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
    player_runtime: &PlayerRuntime,
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
        .insert_resource(player_runtime.clone())
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
    player_runtime: &mut PlayerRuntime,
    source: Option<NetworkItemStack>,
    cursor: Option<NetworkItemStack>,
) -> UiRuntime {
    *player_runtime = PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&player_content(source));
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&cursor_content(cursor));
    runtime.toggle_inventory(player_runtime);
    assert!(runtime.inventory_open());
    assert!(
        runtime
            .inventory_ledger_mut(player_runtime)
            .mark_transport_enqueued(0)
    );
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    runtime
}

fn storage_runtime(
    player_runtime: &mut PlayerRuntime,
    source: Option<(usize, NetworkItemStack)>,
    cursor: Option<NetworkItemStack>,
) -> UiRuntime {
    *player_runtime = PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, InventoryAuthority::Server);
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&player_content(None));
    runtime
        .inventory_ledger_mut(player_runtime)
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
            .enqueue_inventory_event(player_runtime, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(player_runtime);
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

fn apply_apple_registry(player_runtime: &mut PlayerRuntime, runtime: &mut UiRuntime) {
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([ItemRegistryEntry {
                identifier: Arc::from("minecraft:apple"),
                network_id: 6,
                component_based: true,
                version: ItemRegistryVersion::DataDriven,
                component_digest: [6; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: false,
                item_tags: std::sync::Arc::from([]),
            }]),
        });
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
                let screen = storage_slots.map_or(
                    crate::ui_runtime::presentation::inventory_pointer::InventoryScreen::Personal,
                    crate::ui_runtime::presentation::inventory_pointer::InventoryScreen::Storage,
                );
                (presentation.inventory_cell_hit(gui, PHYSICAL_SIZE, 1.0, screen) == Some(target))
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

/// Releases `button` and runs a frame; a held cursor stack acts on release.
fn release_and_update(app: &mut App, button: MouseButton) {
    {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        // The inventory resets button state, so re-arm the held state first.
        buttons.press(button);
        buttons.clear();
        buttons.release(button);
    }
    app.update();
}

fn press_key(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
}

/// Shift-click quick-moves; a digit over a cell swaps with that hotbar cell.
#[test]
fn shift_click_and_number_keys_move_without_the_cursor() {
    let mut player_runtime = PlayerRuntime::new(1);

    let mut shifted = pointer_app(
        personal_runtime(&mut player_runtime, Some(stack(9, 90)), None),
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press_key(&mut shifted, KeyCode::ShiftLeft);
    press(&mut shifted, MouseButton::Left);
    shifted.update();
    let ledger = shifted
        .world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger();
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(9).map(|stack| stack.count), Some(9));

    let mut digit = pointer_app(
        personal_runtime(&mut player_runtime, Some(stack(9, 91)), None),
        InventoryCellHit::Player(0),
        false,
        true,
        &player_runtime,
    );
    press_key(&mut digit, KeyCode::Digit4);
    digit.update();
    let ledger = digit.world().resource::<PlayerRuntime>().inventory.ledger();
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(3).map(|stack| stack.count), Some(9));
}

/// Q drops one item from the hovered cell, Control+Q the whole stack, and a
/// click outside the panel drops the held stack.
#[test]
fn drop_keys_and_outside_clicks_drop_items() {
    let mut player_runtime = PlayerRuntime::new(1);

    for (control, remaining) in [(false, Some(8)), (true, None)] {
        let mut app = pointer_app(
            personal_runtime(&mut player_runtime, Some(stack(9, 92)), None),
            InventoryCellHit::Player(0),
            false,
            true,
            &player_runtime,
        );
        if control {
            press_key(&mut app, KeyCode::ControlLeft);
        }
        press_key(&mut app, KeyCode::KeyQ);
        app.update();
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(
            ledger.displayed_stack(0).map(|stack| stack.count),
            remaining
        );
        assert_eq!(ledger.pending_request_count(), 1);
    }

    let mut outside = pointer_app_at(
        personal_runtime(&mut player_runtime, None, Some(stack(5, 93))),
        Vec2::ZERO,
        false,
        true,
        &player_runtime,
    );
    press(&mut outside, MouseButton::Right);
    outside.update();
    let ledger = outside
        .world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger();
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(4));
    assert_eq!(ledger.pending_request_count(), 1);
}

#[test]
fn review_remapped_drop_does_not_keep_q_active_in_inventory() {
    let mut player_runtime = PlayerRuntime::new(1);

    use crate::menu::{
        MenuAction,
        settings_options::{EXTRA_KEYS, KEY_BINDINGS},
    };
    let index = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.drop")
            .unwrap();
    for control in [false, true] {
        let mut app = pointer_app(
            personal_runtime(&mut player_runtime, Some(stack(9, 92)), None),
            InventoryCellHit::Player(0),
            true,
            true,
            &player_runtime,
        );
        let mut layout = crate::install_layout::InstallLayout::discover().unwrap();
        layout.user_config_root =
            std::env::temp_dir().join(format!("review-drop-{}-{control}", std::process::id()));
        app.insert_resource(MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Tester".into(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Tester"),
        ));
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::SettingsKey(index as u16));
        press_key(&mut app, KeyCode::KeyR);
        app.update();
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert_eq!(
            menu.view().settings_options.named_key_control("key.drop"),
            Some(semantic_input::PhysicalControl::KeyboardUsage(
                crate::semantic_controls::keyboard_usage(KeyCode::KeyR).unwrap()
            ))
        );
        menu.set_visible(false);
        app.insert_resource(personal_runtime(
            &mut player_runtime,
            Some(stack(9, 92)),
            None,
        ));
        app.insert_resource(player_runtime.clone());
        if control {
            press_key(&mut app, KeyCode::ControlLeft);
        }
        press_key(&mut app, KeyCode::KeyQ);
        app.update();
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(9));
        assert_eq!(ledger.pending_request_count(), 0);
        press_key(&mut app, KeyCode::KeyR);
        app.update();
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(
            ledger.displayed_stack(0).map(|stack| stack.count),
            if control { None } else { Some(8) }
        );
        assert_eq!(ledger.pending_request_count(), 1);
    }
}
