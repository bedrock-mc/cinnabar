use super::*;
use protocol::{
    InventoryAuthority, InventoryContentEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
};
use std::sync::Arc;

struct Fixture(Arc<FixtureOwner>);
impl Fixture {
    fn new() -> Self {
        let owner = Arc::new(FixtureOwner {
            probe: Mutex::new(Probe::new()),
            retired: AtomicBool::new(false),
        });
        FIXTURE.with(|fixture| {
            assert!(fixture.borrow().is_none());
            *fixture.borrow_mut() = Some(Arc::clone(&owner));
        });
        Self(owner)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        FIXTURE.with(|fixture| *fixture.borrow_mut() = None);
    }
}

fn state() -> CraftingAuthority {
    let mut state = CraftingAuthority::new(1);
    state.bootstrap(None, InventoryAuthority::Server);
    state.synchronize(Some((1, 0, Some(0))));
    state
}
fn grid(count: u16) -> InventoryAuthorityEvent {
    let mut slots = vec![NetworkItemStack::empty(); 54];
    for (index, slot) in slots[28..32].iter_mut().enumerate() {
        *slot = NetworkItemStack {
            network_id: if count == 0 { 0 } else { 6 },
            stack_network_id: index as i32 + 101,
            count,
            nbt_digest: Sha256::digest([]).into(),
            ..NetworkItemStack::empty()
        };
    }
    InventoryAuthorityEvent::Inventory(InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(124),
            slot_type: Some(0),
            dynamic_id: None,
        },
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }))
}
fn slot(name: u8, position: u16) -> InventoryAuthorityEvent {
    InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(124),
                slot_type: Some(name),
                dynamic_id: None,
            },
            slot: position,
        },
        stack: NetworkItemStack::empty(),
        storage_item: None,
    }))
}

#[test]
fn transaction_origins_keep_one_fifo_source_for_crafting_and_cursor() {
    let fixture = Fixture::new();
    let mut state = state();
    let updates = [(0, 28), (0, 29), (protocol::CONTAINER_NAME_CURSOR, 0)].map(|(name, index)| {
        let InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(update)) = slot(name, index)
        else {
            panic!()
        };
        update
    });
    let event = InventoryAuthorityEvent::Inventory(InventoryEvent::Transaction(
        protocol::InventoryTransactionEvent {
            slots: Arc::from(updates),
            skipped_actions: 0,
        },
    ));
    state.observe(1, 1, &event);
    state.synchronize(Some((1, 0, Some(1))));
    state.advance();
    let probe = fixture.0.probe.lock().unwrap();
    assert!(!probe.retired);
    for index in [0, 1, 4] {
        let source = probe.origins[index].unwrap();
        assert_eq!(source.kind, 3);
        assert_eq!(source.sequence, 1);
        assert_eq!(source.mask, 19);
        assert_eq!(source.epoch, Some(0));
    }
    assert_eq!(probe.origins[0].unwrap().slots, [28, 29, 0, 0]);
    assert!(probe.origins[2..4].iter().all(Option::is_none));
}

#[test]
fn origins_join_actual_apply_prefix_and_epoch_not_latest_ingress() {
    let fixture = Fixture::new();
    let mut state = state();
    state.observe(1, 1, &grid(1));
    state.observe(1, 3, &slot(0, 29));
    state.observe(1, 5, &grid(2));
    state.advance();
    assert!(
        fixture
            .0
            .probe
            .lock()
            .unwrap()
            .origins
            .iter()
            .all(Option::is_none)
    );
    state.synchronize(Some((1, 0, Some(1))));
    state.advance();
    assert!(
        fixture.0.probe.lock().unwrap().origins[..4]
            .iter()
            .all(|source| source.unwrap().sequence == 1)
    );
    state.synchronize(Some((1, 2, Some(3))));
    state.advance();
    let probe = fixture.0.probe.lock().unwrap();
    assert!(probe.origins[0].is_none());
    assert_eq!(probe.origins[1].unwrap().sequence, 3);
    assert_eq!(probe.origins[1].unwrap().epoch, Some(2));
    assert_eq!(probe.origins[1].unwrap().name, Some(0));
    assert!(probe.origins[4].is_none());
    drop(probe);
    state.synchronize(Some((1, 2, Some(5))));
    state.advance();
    let probe = fixture.0.probe.lock().unwrap();
    assert!(
        probe.origins[..4]
            .iter()
            .all(|source| source.unwrap().sequence == 5 && source.unwrap().epoch == Some(2))
    );
    assert!(probe.pending.iter().all(Option::is_none));
}

#[test]
fn skipped_stale_records_and_explicit_cursor_do_not_invent_empty() {
    let fixture = Fixture::new();
    let mut state = state();
    state.observe(1, 1, &grid(0));
    state.synchronize(Some((1, 2, Some(2))));
    state.advance();
    assert!(state.grid.iter().all(Option::is_none));
    assert!(
        fixture
            .0
            .probe
            .lock()
            .unwrap()
            .origins
            .iter()
            .all(Option::is_none)
    );
    state.observe(1, 3, &grid(0));
    state.synchronize(Some((1, 2, Some(3))));
    state.advance();
    let row = snapshot_row(&state, &fixture.0.probe.lock().unwrap(), "Unit");
    assert!(row.cells[..4].iter().all(|cell| cell.state == 1));
    assert_eq!(row.cells[4].state, 0);
    state.observe(1, 4, &slot(protocol::CONTAINER_NAME_CURSOR, 0));
    state.synchronize(Some((1, 2, Some(4))));
    state.advance();
    let row = snapshot_row(&state, &fixture.0.probe.lock().unwrap(), "Unit");
    assert_eq!(row.cells[4].state, 1);
    assert_eq!(row.cells[4].origin.unwrap().sequence, 4);
}

#[test]
fn quotas_reserve_all_useful_phases_and_do_not_remint_on_snapshot_or_session() {
    let fixture = Fixture::new();
    let state = state();
    let clone = state.clone();
    with_probe(|probe| probe.snapshot(&state, false));
    with_probe(|probe| probe.snapshot(&clone, true));
    for _ in 0..100 {
        with_probe(|probe| probe.snapshot(&state, true));
    }
    with_probe(|probe| probe.snapshot(&state, false));
    let before = {
        let probe = fixture.0.probe.lock().unwrap();
        (probe.rows, probe.bytes, probe.spent)
    };
    assert_eq!(before.0, 5);
    assert!(before.1 <= OUTPUT_BYTES);
    let mut replacement = CraftingAuthority::new(2);
    replacement.bootstrap(None, InventoryAuthority::Server);
    replacement.synchronize(Some((2, 0, Some(0))));
    with_probe(|probe| probe.snapshot(&replacement, true));
    let probe = fixture.0.probe.lock().unwrap();
    assert!(probe.retired);
    assert_eq!((probe.rows, probe.bytes, probe.spent), before);
    assert!(std::mem::size_of::<Probe>() <= 8192);
    assert!(std::mem::size_of::<Source>() * META_RECORDS <= 8192);
}

#[test]
fn contention_and_bookkeeping_loss_permanently_prevent_later_complete_capture() {
    let fixture = Fixture::new();
    let state = state();
    let lock = fixture.0.probe.lock().unwrap();
    with_probe(|probe| probe.snapshot(&state, false));
    assert!(fixture.0.retired.load(Ordering::Acquire));
    drop(lock);
    with_probe(|probe| probe.snapshot(&state, true));
    assert_eq!(
        fixture.0.probe.lock().unwrap().rows,
        1,
        "reserved incomplete terminal only"
    );
    crate::InventorySession::retire_crafting_observation();
    let probe = fixture.0.probe.lock().unwrap();
    assert!(probe.incomplete && probe.retired);
    assert!(probe.spent[4]);
}

#[test]
fn prebootstrap_absence_is_not_loss_of_a_bound_observation() {
    let fixture = Fixture::new();
    let mut state = CraftingAuthority::new(1);
    state.synchronize(None);
    assert!(!fixture.0.probe.lock().unwrap().incomplete);
    state.bootstrap(None, InventoryAuthority::Server);
    state.synchronize(Some((1, 0, Some(0))));
    assert_eq!(fixture.0.probe.lock().unwrap().session, Some(1));
    assert!(!fixture.0.probe.lock().unwrap().incomplete);
    state.synchronize(None);
    assert!(fixture.0.probe.lock().unwrap().incomplete);
    assert!(fixture.0.probe.lock().unwrap().retired);
}

#[test]
fn metadata_refusal_and_credit_loss_never_modify_gameplay_projection() {
    let fixture = Fixture::new();
    let mut state = state();
    for sequence in 1..=64 {
        state.observe(1, sequence, &grid(1));
    }
    assert_eq!(
        fixture
            .0
            .probe
            .lock()
            .unwrap()
            .pending
            .iter()
            .flatten()
            .count(),
        64
    );
    state.observe(1, 65, &grid(1));
    assert!(state.queue.is_none());
    let probe = fixture.0.probe.lock().unwrap();
    assert!(probe.incomplete);
    assert!(probe.pending.iter().all(Option::is_none));
    drop(probe);
    state.synchronize(None);
    assert!(fixture.0.probe.lock().unwrap().retired);
}

#[test]
fn raw_source_and_extra_text_never_enter_serialized_receipt() {
    let fixture = Fixture::new();
    let mut state = state();
    let mut event = grid(1);
    let InventoryAuthorityEvent::Inventory(InventoryEvent::Content(content)) = &mut event else {
        unreachable!()
    };
    for stack in Arc::make_mut(&mut content.slots)
        .iter_mut()
        .skip(28)
        .take(4)
    {
        stack.extra_data = Arc::from(b"private custom text and NBT token".as_slice());
    }
    state.observe(1, 1, &event);
    state.synchronize(Some((1, 0, Some(1))));
    state.advance();
    let probe = fixture.0.probe.lock().unwrap();
    let row = snapshot_row(&state, &probe, "Unit");
    let text = serde_json::to_string(&row).unwrap();
    assert!(text.len() <= 2048);
    assert!(!text.contains("private custom") && !text.contains("token"));
    assert!(
        row.cells[..4]
            .iter()
            .all(|cell| !cell.extra[0] && !cell.extra[1])
    );
    assert!(row.cells[..4].iter().all(|cell| cell.capacity.is_none()));
}

#[test]
fn endpoint_gate_refuses_dns_remote_missing_and_wrong_port() {
    assert!(qualified(Some("127.0.0.1:60475"), Some("127.0.0.1:60475")));
    assert!(qualified(Some("[::1]:60475"), Some("[::1]:60475")));
    for address in [
        "localhost:60475",
        "192.0.2.1:60475",
        "127.0.0.1:0",
        "127.0.0.1:60476",
    ] {
        assert!(!qualified(Some(address), Some("127.0.0.1:60475")));
    }
    assert!(!qualified(None, Some("127.0.0.1:60475")));
    assert!(!qualified(Some("127.0.0.1:60475"), None));
}

#[test]
fn actual_configuration_mechanism_never_recreates_grant_after_first_setup() {
    let configured = AtomicBool::new(false);
    let owner = OnceLock::new();
    configure_once(&configured, &owner, None, None);
    assert!(
        owner.get().is_none(),
        "default-off creates no owner or observation work"
    );
    configure_once(
        &configured,
        &owner,
        Some("127.0.0.1:60475"),
        Some("127.0.0.1:60475"),
    );
    assert!(
        owner.get().is_none(),
        "later setup cannot acquire a new grant"
    );
    let configured = AtomicBool::new(false);
    let owner = OnceLock::new();
    configure_once(
        &configured,
        &owner,
        Some("127.0.0.1:60475"),
        Some("127.0.0.1:60475"),
    );
    let mut probe = owner.get().unwrap().lock().unwrap();
    probe.reserve(0);
    probe.retire();
    drop(probe);
    configure_once(
        &configured,
        &owner,
        Some("127.0.0.1:60475"),
        Some("127.0.0.1:60475"),
    );
    let probe = owner.get().unwrap().lock().unwrap();
    assert!(probe.retired && probe.spent[0]);
}

#[test]
fn registry_rebinding_and_dropping_clones_keep_only_real_current_origins() {
    let fixture = Fixture::new();
    let entry = |name: &str| protocol::ItemRegistryEvent {
        entries: vec![protocol::ItemRegistryEntry {
            identifier: Arc::from(name),
            network_id: 6,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        }]
        .into(),
    };
    let mut state = CraftingAuthority::new(1);
    state.bootstrap(Some(&entry("minecraft:stone")), InventoryAuthority::Server);
    state.synchronize(Some((1, 0, Some(0))));
    state.observe(1, 1, &grid(1));
    state.synchronize(Some((1, 0, Some(1))));
    state.advance();
    let clone = state.clone();
    drop(clone);
    assert!(
        fixture.0.probe.lock().unwrap().origins[..4]
            .iter()
            .all(Option::is_some)
    );
    state.observe(
        1,
        2,
        &InventoryAuthorityEvent::Registry(entry("minecraft:dirt")),
    );
    state.synchronize(Some((1, 0, Some(2))));
    state.advance();
    assert!(state.grid.iter().all(Option::is_none));
    assert!(
        fixture.0.probe.lock().unwrap().origins[..4]
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(fixture.0.probe.lock().unwrap().registry_source, Some(2));
}

#[test]
fn bounded_writer_refuses_overrun_and_reserved_phase_cannot_be_reused() {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        capacity: 2,
    };
    writer.write_all(b"12").unwrap();
    assert!(writer.write_all(b"3").is_err());
    assert_eq!(writer.bytes, b"12");
    let mut probe = Probe::new();
    for bank in [4, 3, 2, 1, 0] {
        assert_eq!(probe.reserve(bank), Some(BANKS[bank]));
    }
    assert!(probe.bytes <= OUTPUT_BYTES);
    for bank in 0..5 {
        assert_eq!(probe.reserve(bank), None);
    }
}

#[test]
fn maximal_snapshot_and_terminal_fit_reserved_banks_without_text() {
    let fixture = Fixture::new();
    let mut state = state();
    state.observe(1, 1, &grid(1));
    state.observe(1, 2, &slot(protocol::CONTAINER_NAME_CURSOR, 0));
    state.synchronize(Some((1, 0, Some(2))));
    state.advance();
    let mut probe = fixture.0.probe.lock().unwrap();
    probe.origins = [Some(Source {
        kind: u8::MAX,
        window: Some(i32::MIN),
        name: Some(u8::MAX),
        dynamic: Some(u32::MAX),
        slots: [u16::MAX; 4],
        mask: u8::MAX,
        sequence: u64::MAX,
        epoch: Some(u64::MAX),
    }); 5];
    let mut row = snapshot_row(&state, &probe, "BootstrapCommittedSnapshot");
    row.session = u64::MAX;
    row.stream = Some(u64::MAX);
    row.epoch = u64::MAX;
    row.through = Some(u64::MAX);
    row.barrier = u64::MAX;
    row.registry_revision = Some(u64::MAX);
    row.registry_source = Some(u64::MAX);
    row.catalog_revision = u64::MAX;
    for cell in &mut row.cells {
        cell.numbers = Some([
            i64::from(i32::MIN),
            i64::from(i32::MAX),
            i64::from(u16::MAX),
            i64::from(u32::MAX),
            i64::from(u32::MAX),
            1,
        ]);
        cell.capacity = Some(u16::MAX);
    }
    assert!(serde_json::to_vec(&row).unwrap().len() <= BANKS[0]);
    let terminal = TerminalRow {
        phase: "Terminal",
        capture_complete: false,
        incomplete: true,
        retired: true,
        opened: true,
        closed: true,
        rows: MAX_ROWS,
        reserved_bytes: OUTPUT_BYTES,
    };
    assert!(serde_json::to_vec(&terminal).unwrap().len() <= BANKS[4]);
}

#[test]
fn output_refusal_is_persistent_incomplete_and_does_not_mark_capture_complete() {
    let mut probe = Probe::new();
    probe.emit(0, || vec![u64::MAX; 256]);
    assert!(probe.incomplete);
    assert!(probe.spent[0]);
    assert_eq!(probe.rows, 1);
    probe.terminal();
    assert!(probe.incomplete);
    assert_eq!(probe.rows, 2);
}

#[test]
fn observer_does_not_change_normal_open_close_bytes_or_allocate_craft_request() {
    use protocol::ContainerOpenEvent;
    let mut ledger = crate::inventory_ledger::PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut before = ledger.clone();
    assert!(ledger.request_personal_open(42));
    assert!(before.request_personal_open(42));
    let bytes = |packet: protocol::Packet| {
        protocol::encode(&packet, &protocol::BedrockSession { shield_item_id: 0 }).unwrap()
    };
    let expected = bytes(before.pending_batch().unwrap().unwrap().0);
    let fixture = Fixture::new();
    let state = state();
    with_probe(|probe| probe.snapshot(&state, true));
    assert_eq!(bytes(ledger.pending_batch().unwrap().unwrap().0), expected);
    let open = ContainerOpenEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    };
    // No invented transport receipt: both ledgers receive the same actual lifecycle calls.
    assert!(ledger.mark_transport_enqueued(0));
    assert!(before.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(open));
    before.apply(&InventoryEvent::Open(open));
    ledger.request_personal_close();
    before.request_personal_close();
    assert_eq!(
        bytes(ledger.pending_batch().unwrap().unwrap().0),
        bytes(before.pending_batch().unwrap().unwrap().0)
    );
    assert!(ledger.pending_request_id().is_none());
    assert!(fixture.0.probe.lock().unwrap().rows <= MAX_ROWS);
}

#[test]
fn review_contention_during_an_action_retires_its_terminal_receipt() {
    let owner = std::sync::Mutex::new(Probe::new());
    let retired = std::sync::atomic::AtomicBool::new(false);
    try_action(&owner, &retired, |probe| {
        probe.spent[..4].fill(true);
        try_action(&owner, &retired, |_| panic!("contending action ran"));
        probe.terminal();
    });
    let probe = owner.lock().unwrap();
    assert!(probe.retired);
    assert!(probe.incomplete);
    assert!(probe.spent[4]);
}
