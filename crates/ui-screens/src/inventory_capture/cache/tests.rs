use super::*;
use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};
use std::cell::RefCell;

/// Creates a nonempty stack whose count also controls the synthetic durability bar.
fn stack(id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: id,
        stack_network_id: id + 10,
        count,
        ..NetworkItemStack::empty()
    }
}

/// Applies one authoritative write to any protocol container.
fn write(ledger: &mut PlayerInventoryLedger, window: i32, slot: u16, stack: NetworkItemStack) {
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(window),
            slot,
        },
        stack,
        storage_item: None,
    }));
}

/// Supplies three populated player cells, including two independent dial kinds.
fn ledger() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    for id in 1..=3 {
        write(&mut ledger, 0, (id - 1) as u16, stack(id, 5));
    }
    ledger
}

/// Synthetic artwork exposes source, local animation and dial dependencies in the output.
fn icon(
    stack: &NetworkItemStack,
    local: Option<u32>,
    animation: ProjectionAnimation,
    source: u16,
) -> ProjectedIcon {
    let dial = match stack.network_id {
        2 => Some(DialKind::Clock),
        3 => Some(DialKind::Compass),
        _ => None,
    };
    let frame = local
        .or_else(|| dial.and_then(|kind| animation.dials.frame(kind)))
        .unwrap_or(0);
    ProjectedIcon {
        icon: Some(IconRef {
            page: source,
            uv: [stack.network_id as u16, frame as u16, stack.count, 16],
            glint: false,
        }),
        dial,
    }
}

/// Synthetic bars preserve response corrections independently of stack artwork.
fn fraction(stack: &NetworkItemStack, correction: Option<i32>) -> Option<f32> {
    Some(correction.unwrap_or(i32::from(stack.count)) as f32 / 100.0)
}

/// Synthetic names expose language and source ordering for duplicate identities.
fn name(stack: &NetworkItemStack, language: u16) -> Option<String> {
    Some(format!("{language}:{}:{}", stack.network_id, stack.count))
}

#[derive(Default)]
struct Calls {
    icons: Vec<i32>,
    bars: Vec<i32>,
    names: Vec<i32>,
    recipes: usize,
}

/// Captures with observable resolvers and checks every field against uncached output.
fn capture(
    cache: &mut InventoryProjectionCache,
    input: ProjectionInput<'_>,
    source: u16,
) -> (ProjectionUpdate, Calls) {
    let calls = RefCell::new(Calls::default());
    let ledger = input.ledger;
    let open = input.inventory_open;
    let animation = input.animation;
    let update = cache.capture(
        input,
        || {
            calls.borrow_mut().recipes += 1;
            inventory::CraftGridMatch::NoMatch
        },
        |stack, local| {
            calls.borrow_mut().icons.push(stack.network_id);
            icon(stack, local, animation, source)
        },
        |stack, correction| {
            calls.borrow_mut().bars.push(stack.network_id);
            fraction(stack, correction)
        },
        |stack| {
            calls.borrow_mut().names.push(stack.network_id);
            name(stack, source)
        },
    );
    let expected = capture_contents(
        ledger,
        open,
        || inventory::CraftGridMatch::NoMatch,
        |stack, slot| {
            icon(
                stack,
                slot.and_then(|slot| animation.player.get(usize::from(slot)).copied().flatten()),
                animation,
                source,
            )
            .icon
        },
        |stack| name(stack, source),
    );
    let decorations = capture_decorations(ledger, fraction, |stack| {
        icon(stack, None, animation, source).icon
    });
    assert_eq!(cache.contents, expected);
    assert_eq!(cache.durability, decorations.0);
    assert_eq!(cache.ui_icons, decorations.1);
    (update, calls.into_inner())
}

/// Supplies ordinary frame state; tests override only the invalidated inputs.
fn input(ledger: &PlayerInventoryLedger) -> ProjectionInput<'_> {
    ProjectionInput {
        ledger,
        inventory_open: false,
        refresh: Default::default(),
        animation: Default::default(),
    }
}

/// Opens the inventory while retaining the ordinary source and animation inputs.
fn open_input(ledger: &PlayerInventoryLedger) -> ProjectionInput<'_> {
    ProjectionInput {
        inventory_open: true,
        ..input(ledger)
    }
}

#[test]
fn large_server_menu_keeps_high_slot_artwork_names_and_updates() {
    let mut ledger = ledger();
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_CONTAINER,
        position: [0; 3],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: ContainerIdentity::window(7),
        slots: vec![NetworkItemStack::empty(); 200].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    write(&mut ledger, 7, 113, stack(11, 5));
    write(&mut ledger, 7, 199, stack(12, 8));
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, open_input(&ledger), 0);
    assert_eq!(cache.contents.storage_icons.0[113].unwrap().uv[0], 11);
    assert_eq!(cache.contents.storage_icons.0[199].unwrap().uv[0], 12);
    assert_eq!(cache.contents.item_names[&(12, 0)].as_ref(), "0:12:8");
    assert_eq!(cache.durability.storage[199], Some(0.08));

    write(&mut ledger, 7, 199, NetworkItemStack::empty());
    let (_, calls) = capture(&mut cache, open_input(&ledger), 0);
    assert!(calls.icons.is_empty());
    assert!(cache.contents.storage_icons.0[199].is_none());
    assert!(!cache.contents.item_names.contains_key(&(12, 0)));
    assert!(cache.durability.storage[199].is_none());
    assert!(cache.contents.storage_icons.0[113].is_some());
}

#[test]
fn unchanged_frames_resolve_nothing_and_single_slot_changes_stay_local() {
    let mut ledger = ledger();
    let mut cache = InventoryProjectionCache::default();
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.icons, [1, 2, 3]);
    for _ in 0..3 {
        let (update, calls) = capture(&mut cache, input(&ledger), 0);
        assert!(update.contents.is_none() && update.decorations.is_none());
        assert!(update.item_names.is_none());
        assert!(calls.icons.is_empty() && calls.bars.is_empty() && calls.names.is_empty());
        assert_eq!(calls.recipes, 0);
    }
    write(&mut ledger, 0, 1, stack(2, 7));
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.icons, [2]);
    assert_eq!(calls.bars, [2]);
    write(&mut ledger, 0, 1, NetworkItemStack::empty());
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert!(calls.icons.is_empty() && calls.bars.is_empty());
    assert!(cache.contents.inventory_icons.0[1].is_none());
}

#[test]
fn dial_and_charge_frames_update_only_their_icons() {
    let ledger = ledger();
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, input(&ledger), 0);
    let mut animation = ProjectionAnimation::default();
    animation.dials.clock = Some(3);
    let (update, calls) = capture(
        &mut cache,
        ProjectionInput {
            animation,
            ..input(&ledger)
        },
        0,
    );
    assert_eq!(calls.icons, [2]);
    assert!(calls.bars.is_empty() && calls.names.is_empty());
    assert!(update.decorations.is_none());
    assert!(update.item_names.is_none());
    animation.player[0] = Some(2);
    let (_, calls) = capture(
        &mut cache,
        ProjectionInput {
            animation,
            ..input(&ledger)
        },
        0,
    );
    assert_eq!(calls.icons, [1]);
    assert!(calls.bars.is_empty());
    // An identical server restatement can clear a local charge/fire prediction.
    let mut corrected = ledger.clone();
    write(&mut corrected, 0, 0, stack(1, 5));
    animation.player[0] = None;
    let (_, calls) = capture(
        &mut cache,
        ProjectionInput {
            animation,
            ..input(&corrected)
        },
        0,
    );
    assert_eq!(calls.icons, [1]);
    assert!(calls.bars.is_empty());
}

#[test]
fn first_open_and_language_refresh_resolve_names_without_rebuilding_artwork() {
    let mut ledger = ledger();
    write(
        &mut ledger,
        protocol::UI_INVENTORY_WINDOW_ID,
        u16::from(*protocol::CRAFTING_INPUT_SLOTS.start()),
        stack(4, 1),
    );
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, input(&ledger), 0);
    let (_, calls) = capture(&mut cache, open_input(&ledger), 0);
    assert!(calls.icons.is_empty() && calls.bars.is_empty());
    assert_eq!(calls.names, [1, 2, 3]);
    assert_eq!(calls.recipes, 1);
    assert!(cache.contents.crafting.icons[0].is_some());
    let (_, calls) = capture(&mut cache, open_input(&ledger), 0);
    assert!(calls.names.is_empty());
    let refresh = ProjectionRefresh {
        names: true,
        ..Default::default()
    };
    // Keep artwork's source stable while changing only the synthetic language.
    let result = cache.capture(
        ProjectionInput {
            refresh,
            ..open_input(&ledger)
        },
        || panic!("language cannot rematch recipes"),
        |_, _| panic!("language cannot resolve icons"),
        |_, _| panic!("language cannot resolve bars"),
        |stack| name(stack, 1),
    );
    assert_eq!(result.item_names.unwrap()[&(1, 0)].as_ref(), "1:1:5");
    let result = cache.capture(
        input(&ledger),
        || panic!("closed"),
        |_, _| panic!("unchanged"),
        |_, _| panic!("unchanged"),
        |_| panic!("closed"),
    );
    let contents = result.contents.unwrap();
    assert!(result.item_names.unwrap().is_empty());
    assert_eq!(contents.crafting, CraftingFrame::default());
}

#[test]
fn artwork_and_registry_refresh_revisit_their_dependent_projections() {
    let ledger = ledger();
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, input(&ledger), 0);
    let refresh = ProjectionRefresh {
        icons: true,
        ..Default::default()
    };
    let (_, calls) = capture(
        &mut cache,
        ProjectionInput {
            refresh,
            ..input(&ledger)
        },
        1,
    );
    assert_eq!(calls.icons, [1, 2, 3]);
    assert!(calls.bars.is_empty() && calls.names.is_empty());
    let refresh = ProjectionRefresh {
        icons: true,
        durability: true,
        names: true,
        recipes: true,
    };
    let (_, calls) = capture(
        &mut cache,
        ProjectionInput {
            refresh,
            inventory_open: true,
            ..input(&ledger)
        },
        2,
    );
    assert_eq!(calls.icons, [1, 2, 3]);
    assert_eq!(calls.bars, [1, 2, 3]);
    assert_eq!(calls.names, [1, 2, 3]);
}

/// Opens a fresh furnace window and installs its known empty actual cells.
fn furnace(ledger: &mut PlayerInventoryLedger, window: i32) {
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(window),
        window_type: protocol::WINDOW_TYPE_FURNACE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: ContainerIdentity::window(window),
        slots: vec![NetworkItemStack::empty(); 3].into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

#[test]
fn furnace_ghosts_are_artwork_only_and_expire_with_the_storage_generation() {
    let mut ledger = ledger();
    furnace(&mut ledger, 7);
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, input(&ledger), 0);
    let recipe = protocol::ScreenRecipe {
        id: 1,
        kind: protocol::ScreenRecipeKind::Furnace,
        ingredients: vec![protocol::ScreenIngredient {
            name: "test:missing".into(),
            tag: false,
            aux: 0,
        }],
        output: Some(protocol::RecipeOutput {
            network_id: 4,
            aux: 0,
            count: 1,
            block_runtime_id: 0,
            empty_envelope: true,
        }),
    };
    ledger.begin_furnace_recipe(&recipe).unwrap();
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.icons, [4]);
    assert!(calls.bars.is_empty());
    assert!(cache.contents.storage_icons.0[2].is_some());
    assert!(cache.durability.storage[2].is_none());
    write(&mut ledger, 7, 2, stack(5, 3));
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.icons, [5]);
    assert_eq!(calls.bars, [5]);
    furnace(&mut ledger, 8);
    capture(&mut cache, input(&ledger), 0);
    assert!(cache.contents.storage_icons.0.iter().all(Option::is_none));
    assert!(cache.durability.storage.iter().all(Option::is_none));
}

#[test]
fn predicted_rejection_and_server_durability_corrections_restore_the_projection() {
    let mut ledger = ledger();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let mut cache = InventoryProjectionCache::default();
    capture(&mut cache, input(&ledger), 0);
    let original = cache.contents.clone();
    let request = ledger.begin_take_count(0, 2).unwrap();
    assert!(ledger.mark_transport_enqueued(1));
    capture(&mut cache, input(&ledger), 0);
    assert_ne!(cache.contents, original);
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Rejected,
                request_id: request,
                containers: Arc::from([]),
            }]),
        },
    ));
    capture(&mut cache, input(&ledger), 0);
    assert_eq!(cache.contents, original);
    let request = ledger.begin_mining_request(0, 7).unwrap();
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Accepted,
                request_id: request,
                containers: Arc::from([protocol::StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                        dynamic_id: None,
                    },
                    slots: Arc::from([protocol::StackResponseSlot {
                        slot: 0,
                        hotbar_slot: 0,
                        count: 5,
                        item_stack_id: 11,
                        custom_name: Arc::from(""),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: 7,
                    }]),
                }]),
            }]),
        },
    ));
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.bars, [1]);
    // Accepted damage also changes retained item data, so artwork must observe it.
    assert_eq!(calls.icons, [1]);
    assert_eq!(
        protocol::item_extra_damage_integer(&cache.player[0].stack.as_ref().unwrap().extra_data),
        Some(7)
    );
    assert_eq!(cache.durability.player[0], Some(0.07));
    write(&mut ledger, 0, 0, stack(1, 5));
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert_eq!(calls.bars, [1]);
    assert_eq!(calls.icons, [1]);
    assert_eq!(cache.durability.player[0], Some(0.05));
    let (_, calls) = capture(&mut cache, input(&ledger), 0);
    assert!(calls.bars.is_empty());
    assert!(calls.icons.is_empty());
}

#[test]
fn session_replacement_discards_previous_cells_and_names() {
    let mut ledger = ledger();
    let mut cache = InventoryProjectionCache::default();
    capture(
        &mut cache,
        ProjectionInput {
            inventory_open: true,
            ..input(&ledger)
        },
        0,
    );
    ledger.begin_session(2);
    capture(
        &mut cache,
        ProjectionInput {
            inventory_open: true,
            ..input(&ledger)
        },
        0,
    );
    assert!(cache.contents.inventory_icons.0.iter().all(Option::is_none));
    assert!(cache.contents.item_names.is_empty());
}
