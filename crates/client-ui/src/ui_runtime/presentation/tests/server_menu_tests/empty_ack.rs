use super::*;
use crate::ui_runtime::presentation::{SessionIcon, SessionIcons, forms, session_icons};

const SHOP_SLOT: usize = 19;

fn stock() -> NetworkItemStack {
    NetworkItemStack {
        network_id: 1,
        stack_network_id: 100,
        count: 16,
        ..NetworkItemStack::empty()
    }
}

#[test]
fn empty_success_ack_clears_the_rendered_shop_cursor() {
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(forms::pack_harness::font())
    else {
        eprintln!(
            "skipping empty_success_ack_clears_the_rendered_shop_cursor: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let menu = ContainerIdentity {
        slot_type: Some(0),
        ..ContainerIdentity::window(2)
    };
    let mut slots =
        vec![NetworkItemStack::empty(); inventory::inventory_ledger::LARGE_STORAGE_SLOT_COUNT];
    slots[SHOP_SLOT] = stock();
    for (sequence, event) in [
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
        InventoryEvent::Content(InventoryContentEvent {
            container: menu,
            slots: slots.into(),
            storage_item: NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(&mut player);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    let request = ledger.begin_storage_click(SHOP_SLOT as u8).unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    let acknowledgement = protocol::into_world_event(
        protocol::test_support::packets::empty_stack_response(request, false),
        0,
    );
    // Live ingress skips semantic rejections; render that failure before asserting acceptance.
    if let Ok(Some(WorldEvent::Inventory(event))) = &acknowledgement {
        ledger.apply(event);
    }
    let mut purchased =
        vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    purchased[1] = NetworkItemStack {
        stack_network_id: 200,
        ..stock()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: purchased.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            slot_type: Some(0),
            ..ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID)
        },
        slots: vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(2),
            slot: SHOP_SLOT as u16,
        },
        stack: stock(),
        storage_item: None,
    }));
    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: "fixture:shop_block".into(),
            metadata: 0,
            width: 16,
            height: 16,
            rgba8: [180, 42, 32, 255].repeat(16 * 16).into_boxed_slice(),
        }],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    runtime.set_session_icons(Some(icons));
    runtime.set_inventory_pointer_gui(Some([210.0, 260.0]));
    let icon = presentation.item_icon("fixture:shop_block", 0).unwrap();
    let ledger = runtime.inventory_ledger(&player);
    let frame = presentation.hud_frame_mut();
    frame.window_text.title = Some("Item Shop".into());
    frame.storage_icons.0[SHOP_SLOT] = ledger.storage_stack(SHOP_SLOT as u8).map(|_| icon);
    frame.inventory_icons.0[1] = Some(icon);
    frame.cursor_icon = ledger.cursor_stack().map(|_| icon);
    for now in [0, 500] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            5_000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    forms::snapshot::write(&input, "shop-empty-success-cursor");
    assert!(
        acknowledgement.is_ok(),
        "the shop acknowledgement must reach the ledger"
    );
    assert!(runtime.inventory_ledger(&player).cursor_stack().is_none());
    assert_eq!(runtime.inventory_ledger(&player).pending_request_count(), 0);
}
