use super::*;
use crate::ui_runtime::interaction::dispatch_inventory_hotbar;

#[test]
fn hotbar_swap_works_on_the_first_inventory_frame() {
    for transport_admitted in [false, true] {
        let Some(mut presentation) =
            engine_presentation_with(crate::ui_runtime::presentation::forms::pack_harness::font())
        else {
            eprintln!(
                "skipping hotbar_swap_works_on_the_first_inventory_frame: missing local UI carrier; make assets"
            );
            return;
        };
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = session(&mut player);
        let mut slots = vec![NetworkItemStack::empty(); protocol::PLAYER_INVENTORY_SLOTS as usize];
        slots[9] = NetworkItemStack {
            network_id: 1,
            stack_network_id: 100,
            count: 32,
            ..NetworkItemStack::empty()
        };
        runtime
            .inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
        runtime.toggle_inventory(&mut player, None);
        if transport_admitted {
            assert!(
                runtime
                    .inventory_ledger_mut(&mut player)
                    .mark_transport_enqueued(0)
            );
        }
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let hit = presentation
            .engine_container_frame()
            .unwrap()
            .hits
            .iter()
            .find_map(|region| {
                let point = [
                    (region.rect.x + region.rect.w / 2.0) as f32,
                    (region.rect.y + region.rect.h / 2.0) as f32,
                ];
                presentation
                    .engine_container_hit(point)
                    .filter(|hit| *hit == InventoryCellHit::Player(9))
            });
        assert_eq!(
            hit,
            Some(InventoryCellHit::Player(9)),
            "first-frame slot hit"
        );
        let outcome = dispatch_inventory_hotbar(&mut player, &mut runtime, hit, 0);
        let input = presentation
            .build(
                &player,
                &runtime,
                1,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        crate::ui_runtime::presentation::forms::snapshot::write(
            &input,
            "inventory-first-frame-hotbar-swap",
        );
        assert!(
            outcome.is_some_and(|result| result.is_ok()),
            "first-frame swap, admitted={transport_admitted}: {outcome:?}"
        );
        assert_eq!(
            runtime
                .inventory_ledger(&player)
                .displayed_stack(0)
                .unwrap()
                .stack_network_id,
            -3
        );
    }
}
