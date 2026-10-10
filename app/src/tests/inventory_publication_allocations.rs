use player_state::PlayerState;

#[test]
fn pre_send_publication_shares_cells_without_cloning_pending_ingress() {
    let mut live = PlayerState::new(1);
    live.inventory
        .enqueue_inventory_event(
            1,
            1,
            protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Client),
        )
        .unwrap();
    let before = super::alloc_count::thread_allocations();
    let captured = live.capture_ledger();
    let presented = live.present(captured);
    let allocations = super::alloc_count::thread_allocations() - before;
    assert_eq!(presented.facts.session_id(), 1);
    assert_eq!(allocations, 0);
    println!("pre-send capture and presentation allocations={allocations}");
}
