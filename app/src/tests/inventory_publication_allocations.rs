use player_state::PlayerState;

#[test]
fn pre_send_publication_shares_cells_without_cloning_pending_ingress() {
    let mut live = PlayerState::new(1);
    let mut ui = client_ui::UiRuntime::new(1);
    live.inventory
        .enqueue_inventory_event(
            1,
            1,
            protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Client),
        )
        .unwrap();
    let before = super::alloc_count::thread_allocations();
    let copied = live.inventory.clone();
    let copied_allocations = super::alloc_count::thread_allocations() - before;
    assert_eq!(copied.session_id(), 1);
    let before = super::alloc_count::thread_allocations();
    let captured = live.capture_ledger();
    ui.poll_inventory_timeout(&mut live, 0);
    let presented = live.present(captured);
    let allocations = super::alloc_count::thread_allocations() - before;
    assert_eq!(presented.facts.session_id(), 1);
    assert_eq!(allocations, 0);
    println!(
        "pre-send publication allocations: ingress copy={copied_allocations}, shared capture/poll/presentation={allocations}"
    );
}
