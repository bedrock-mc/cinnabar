//! Original server-form cells resolve their artwork through the admitted registry and icon set.

use std::sync::Arc;

use protocol::{ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion};
use ui::{DpiScale, UiPoint, UiVisual};

use super::{ServerUiPack, pack_harness};
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use crate::ui_runtime::presentation::{SessionIcon, SessionIcons};
use crate::ui_runtime::{
    UiRuntime,
    forms::{
        engine_input::{EngineInput, PointerButtons, drive},
        flush_form_response,
    },
};

const ITEM_COUNT: usize = 17;

/// Creates an original grid form whose final response row is its Back control.
fn fixture() -> (player_state::PlayerState, UiRuntime, UiPresentationRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let item_id = 12_000;
    player
        .inventory
        .ledger_mut()
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([ItemRegistryEntry {
                identifier: Arc::from("fixture:supply"),
                network_id: item_id,
                component_based: false,
                version: ItemRegistryVersion::Legacy,
                components: None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            }]),
        });
    let mut labels: Vec<_> = (0..ITEM_COUNT)
        .map(|index| ((item_id << 16) | if index % 2 == 0 { 0 } else { 3 }).to_string())
        .collect();
    labels.push("Back".into());
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let mut runtime = pack_harness::action_form(&mut player, "Supply shop", &labels);
    runtime.set_session_icons(Some(Arc::new(SessionIcons {
        icons: [0, 3]
            .map(|metadata| SessionIcon {
                identifier: Arc::from("fixture:supply"),
                metadata,
                width: 8,
                height: 8,
                rgba8: [40, 80 + metadata as u8 * 40, 200, 255]
                    .repeat(64)
                    .into_boxed_slice(),
            })
            .into(),
        ..Default::default()
    })));
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            include_bytes!("../../../../fixtures/server_shop.json").to_vec(),
        )]],
        ..Default::default()
    });
    (player, runtime, presentation)
}

#[test]
fn server_form_item_controls_draw_negotiated_item_variants() {
    let (mut player, runtime, mut presentation) = fixture();
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let expected =
        [0, 3].map(|metadata| presentation.item_icon("fixture:supply", metadata).unwrap());
    let nodes = &presentation.last_frame.as_ref().unwrap().nodes;
    let drawn = expected.map(|icon| nodes.iter().filter(|node| {
        matches!(node.visual(), UiVisual::Sprite { texture_page, uv, .. } if *texture_page == icon.page && *uv == icon.uv)
    }).count());
    assert_eq!(
        drawn,
        [ITEM_COUNT.div_ceil(2), ITEM_COUNT / 2],
        "each cell must use its negotiated item and auxiliary variant"
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    assert!((0..=ITEM_COUNT).all(|index| frame.hits.iter().any(|hit| {
        hit.collection_index == Some(index)
            && hit.pressed.as_deref() == Some("button.form_button_click")
            && hit.contains([hit.rect.x + 1.0, hit.rect.y + 1.0])
    })));
    let mut entry = player
        .inventory
        .ledger()
        .negotiated_item_entry(12_000)
        .unwrap()
        .clone();
    entry.identifier = "fixture:unknown".into();
    player
        .inventory
        .ledger_mut()
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([entry]),
        });
    presentation
        .build(
            &player,
            &runtime,
            1,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(!presentation.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
        matches!(node.visual(), UiVisual::Sprite { texture_page, uv, .. } if expected.iter().any(|icon| *texture_page == icon.page && *uv == icon.uv))
    }), "a registry replacement must not keep stale item artwork");
    assert_eq!(runtime.server_forms().active().unwrap().identity, identity);
}

#[test]
fn shop_item_back_and_close_send_distinct_responses_and_preserve_inventory() {
    for response in [Some(0), Some(ITEM_COUNT as u32), None] {
        let (mut player, mut runtime, mut presentation) = fixture();
        let stack = protocol::NetworkItemStack {
            network_id: 12_000,
            count: 7,
            stack_network_id: 10,
            ..Default::default()
        };
        let mut slots = vec![
            protocol::NetworkItemStack::empty();
            usize::from(protocol::PLAYER_INVENTORY_SLOTS)
        ];
        slots[0] = stack.clone();
        player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(0),
                    slots: slots.into(),
                    storage_item: Default::default(),
                },
            ));
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let identity = runtime.server_forms().active().unwrap().identity;
        let frame = presentation.form_engine_frame(identity).unwrap().clone();
        let hit = frame
            .hits
            .iter()
            .find(|hit| match response {
                Some(index) => {
                    hit.collection_index == Some(index as usize)
                        && hit.pressed.as_deref() == Some("button.form_button_click")
                }
                None => hit.pressed.as_deref() == Some("button.menu_exit"),
            })
            .expect("requested shop control is clickable");
        let cursor = UiPoint::new(
            frame.origin[0] + (hit.rect.x + 1.0) as f32 * frame.scale,
            frame.origin[1] + (hit.rect.y + 1.0) as f32 * frame.scale,
        )
        .unwrap();
        for down in [true, false] {
            drive(
                &mut runtime,
                &frame,
                EngineInput {
                    cursor: Some(cursor),
                    keys: &[],
                    pointer: PointerButtons {
                        pressed: down,
                        released: !down,
                        held: down,
                    },
                    pointer_edges: vec![down],
                    wheel: Vec::new(),
                    typed: Vec::new(),
                    now: if down { 0.0 } else { 0.01 },
                    animator: None,
                },
            );
        }
        let mut sent = Vec::new();
        assert!(
            flush_form_response(
                &mut runtime,
                |_| false,
                |packet| {
                    sent.push(packet);
                    Ok(())
                }
            )
            .unwrap()
        );
        let expected = response.map_or_else(
            || protocol::modal_form_cancel_response(identity.form_id),
            |index| {
                protocol::modal_form_submit_response(
                    identity.form_id,
                    protocol::ModalFormResponseSelection::ButtonIndex(index),
                )
            },
        );
        assert_eq!(sent, [expected]);
        assert_eq!(player.inventory.ledger().displayed_stack(0), Some(&stack));
    }
}
