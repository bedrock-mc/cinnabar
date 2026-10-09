use super::*;
use mod_host::helper::{CallFailure, FailureKind};
use server_experience::screen::GuiSize;
use server_experience::{
    manifest::{Manifest, Offer, PackageOffer, Permission, Scope},
    negotiation::{Limits, VerifiedOffer, Wire},
    runtime::Transaction,
    wire::{Channel, Direction, Field, Fragment, Scalar},
};
use std::collections::BTreeSet;

/// The one template every fixture bundle indexes.
const SCREEN: &str = "ui/screen.json";

struct FakeWorker {
    response: Option<Transaction>,
    /// A failure reply that the next poll delivers before any response.
    failure: Option<CallFailure>,
    /// Makes each dispatch fail its callback instead of completing it.
    fail: bool,
    dispatched: Vec<Event>,
    /// The modal size each dispatch carried.
    sizes: Vec<Option<GuiSize>>,
    owner: Principal,
}

impl Worker for FakeWorker {
    /// Leaves initialization pending until the test supplies its completion.
    fn spawn(_: &Path, _: Vec<u8>, owner: Principal, _: Capabilities, _: u64) -> Result<Self> {
        Ok(Self {
            response: None,
            failure: None,
            fail: false,
            dispatched: Vec::new(),
            sizes: Vec::new(),
            owner,
        })
    }
    /// Delivers only explicitly completed transactions and failures.
    fn poll(&mut self) -> Option<Result<Reply>> {
        if let Some(failure) = self.failure.take() {
            return Some(Ok(Reply::Failed(failure)));
        }
        self.response.take().map(|transaction| {
            Ok(Reply::Committed {
                transaction,
                fuel: 1,
            })
        })
    }
    /// Records delivered events and completes them on the next poll, in the dispatch's epoch,
    /// or fails them when the test asks.
    fn dispatch(&mut self, request: Dispatch) -> Result<()> {
        self.sizes.push(request.gui);
        if self.fail {
            self.failure = Some(CallFailure {
                bundle: self.owner.bundle.clone(),
                callback: "dispatch".into(),
                kind: FailureKind::Panic,
                reason: "wasm trap: wasm `unreachable` instruction executed".into(),
                fuel: Some(1),
            });
            self.dispatched.push(request.event);
            return Ok(());
        }
        self.dispatched.push(request.event);
        self.response = Some(Transaction {
            owner: self.owner.clone(),
            epoch: request.epoch,
            commands: Vec::new(),
        });
        Ok(())
    }
}

/// Reserves real aggregate budgets for pending component helpers without starting processes.
fn fixture(count: usize) -> Live<FakeWorker> {
    let scope = Scope {
        permissions: BTreeSet::from([
            Permission::Messaging,
            Permission::Ui,
            Permission::ModalUi,
            Permission::Input,
        ]),
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
    };
    let packages = (0..count)
        .map(|i| PackageOffer {
            id: format!("bundle{i}"),
            publisher_key: String::new(),
            digest: format!("digest{i}"),
            bytes: 1,
            url: String::new(),
        })
        .collect::<Vec<_>>();
    let grant = Grant {
        offer: VerifiedOffer {
            offer: Offer {
                version: WIRE_VERSION,
                audience: String::new(),
                server_key: String::new(),
                revision: 1,
                expires_unix: u64::MAX,
                scope: scope.clone(),
                packages,
                fallback: String::new(),
                carrier: protocol::EXPERIENCE_CHANNEL.into(),
            },
            digest: String::new(),
        },
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        wire: server_experience::negotiation::Wire::v1(),
        expires_unix: u64::MAX,
    };
    let mut budget = Budget::default();
    budget.begin_slice();
    let instances = grant
        .offer
        .offer
        .packages
        .iter()
        .map(|package| {
            let owner = Principal {
                session: grant.session.clone(),
                bundle: package.id.clone(),
                generation: INITIAL_BUNDLE_GENERATION,
            };
            budget.reserve(owner.clone(), 0, 0).unwrap();
            let channels = [Direction::ToClient, Direction::ToServer]
                .into_iter()
                .enumerate()
                .map(|(i, direction)| Channel {
                    id: format!("{}.events{i}", owner.bundle),
                    schema: API_VERSION,
                    direction,
                    fields: vec![Field::Bool],
                })
                .collect();
            (
                owner.bundle.clone(),
                Instance {
                    helper: None,
                    component: Some(vec![0]),
                    capabilities: Capabilities {
                        scope: scope.clone(),
                        assets: BTreeSet::from([SCREEN.to_owned()]),
                        templates: BTreeSet::from([SCREEN.to_owned()]),
                        channels,
                        actions: BTreeSet::from([format!("{}.pick", owner.bundle)]),
                        max_message_bytes: grant.wire.limits.max_message_bytes,
                    },
                    owner,
                    contributions: Contributions::default(),
                    busy: true,
                    files: Arc::default(),
                    opened: 0,
                    events: VecDeque::new(),
                    epoch: 1,
                    strikes: VecDeque::new(),
                    stopped: None,
                    callback: "init",
                },
            )
        })
        .collect();
    let mut live = Live {
        media: super::super::media::Media::new(grant.clone(), 1, PathBuf::new()),
        screens: Vec::new(),
        screens_built: None,
        scene_revision: 0,
        grant,
        instances,
        executable: PathBuf::new(),
        pending_sends: VecDeque::new(),
        pending_send_bytes: 0,
        budget,
        ingress: Ingress::new(0),
        egress: RateLimit::new(0),
        sequence: 1,
        slice_ms: 0,
        ready: false,
        epoch: 1,
        modal_order: 0,
        gui: None,
    };
    live.initialize().unwrap();
    live
}

/// Completes an initializer with a valid outbound channel command.
fn complete(live: &mut Live<FakeWorker>, id: &str) {
    let instance = live.instances.get_mut(id).unwrap();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![Command::Send {
            channel: format!("{id}.events1"),
            schema: API_VERSION,
            record: vec![Scalar::Bool(true)],
        }],
    });
}

#[test]
fn modal_presses_reach_only_declared_actions_of_the_open_screen() {
    let mut live = fixture(1);
    let owner = live.instances["bundle0"].owner.clone();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response = Some(Transaction {
        owner,
        epoch: live.epoch,
        commands: vec![Command::Screen {
            template: Some(SCREEN.into()),
        }],
    });
    live.poll(1, 0).unwrap();
    let modal = live.modal().unwrap();
    assert_eq!(
        (modal.bundle, modal.modal.template.as_deref()),
        ("bundle0", Some(SCREEN))
    );
    assert!(!live.press("bundle0.other", Some(2)));
    assert!(live.press("bundle0.pick", Some(2)));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(
        helper.dispatched,
        [Event::Action {
            id: "bundle0.pick".into(),
            index: Some(2),
        }]
    );
    live.close_modal();
    assert!(live.modal().unwrap().modal.template.is_none());
    assert!(!live.press("bundle0.pick", None));
}

/// Queues a reliable event through the real schema, sequence and rate validators.
fn receive(live: &mut Live<FakeWorker>, id: &str, sequence: u64) {
    let message = Envelope {
        version: live.grant.wire.version,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: 0,
        bundle: id.into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: format!("{id}.events0"),
        schema: API_VERSION,
        sequence,
        world_epoch: live.epoch,
        payload: vec![Scalar::Bool(sequence.is_multiple_of(2))],
    };
    live.receive(&serde_json::to_vec(&message).unwrap(), CALLBACK_INTERVAL_MS)
        .unwrap();
}

#[test]
fn staggered_initialization_retains_sends_and_publishes_ready_first() {
    let mut live = fixture(2);
    complete(&mut live, "bundle0");
    assert!(live.poll(1, 0).unwrap().is_empty());
    assert!(!live.ready);
    assert_eq!(live.pending_sends.len(), 1);
    complete(&mut live, "bundle1");
    let packets = live.poll(1, 1).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    let sends = packets[1..]
        .iter()
        .map(|bytes| serde_json::from_slice::<Envelope>(bytes).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sends.iter().map(|send| send.sequence).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(sends[0].bundle, "bundle0");
    assert_eq!(sends[1].bundle, "bundle1");
    assert_eq!(live.pending_send_bytes, 0);
}

#[test]
fn four_components_initialize_across_slices_before_readiness() {
    let mut live = fixture(MAX_BUNDLES);
    assert_eq!(
        live.instances
            .values()
            .filter(|instance| instance.helper.is_some())
            .count(),
        2
    );
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    assert!(live.poll(1, 0).unwrap().is_empty());
    assert!(!live.ready);
    assert!(live.poll(1, CALLBACK_INTERVAL_MS).unwrap().is_empty());
    assert!(
        live.instances
            .values()
            .all(|instance| instance.helper.is_some())
    );
    complete(&mut live, "bundle2");
    assert!(live.poll(1, CALLBACK_INTERVAL_MS + 1).unwrap().is_empty());
    complete(&mut live, "bundle3");
    assert_eq!(
        live.poll(1, CALLBACK_INTERVAL_MS + 2).unwrap().len(),
        MAX_BUNDLES + 1
    );
    assert!(live.ready);
}

#[test]
fn reliable_bursts_wait_for_busy_helpers_and_aggregate_callback_budget() {
    let mut live = fixture(3);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    complete(&mut live, "bundle2");
    live.poll(1, CALLBACK_INTERVAL_MS + 1).unwrap();
    for (index, id) in ["bundle0", "bundle0", "bundle1", "bundle2"]
        .into_iter()
        .enumerate()
    {
        receive(&mut live, id, index as u64 + 1);
    }
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.instances["bundle0"].busy);
    assert_eq!(live.ingress.peek(u64::MAX, 1).unwrap().sequence, 2);
    live.poll(1, 2 * CALLBACK_INTERVAL_MS + 1).unwrap();
    assert_eq!(live.ingress.peek(u64::MAX, 1).unwrap().sequence, 3);
    assert!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .is_empty()
    );
    for tick in 3..=6 {
        live.poll(1, tick * CALLBACK_INTERVAL_MS).unwrap();
    }
    let delivered = live
        .instances
        .values()
        .map(|instance| instance.helper.as_ref().unwrap().dispatched.len())
        .collect::<Vec<_>>();
    assert_eq!(delivered, [2, 1, 1]);
    assert!(live.ingress.peek(u64::MAX, 1).is_none());
    let records = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    let record = |value| Event::Message {
        channel: "bundle0.events0".into(),
        record: serde_json::to_vec(&vec![Scalar::Bool(value)]).unwrap(),
    };
    assert_eq!(records, &[record(false), record(true)]);
}

#[test]
fn a_v1_dimension_change_rejects_completed_old_epoch_output_before_publication() {
    let mut live = fixture(1);
    complete(&mut live, "bundle0");
    assert!(live.poll(2, CALLBACK_INTERVAL_MS).is_err());
    assert_eq!(live.sequence, 1);
    assert!(!live.ready);
    assert!(live.pending_sends.is_empty());
    assert!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .response
            .is_some()
    );
}

/// A guest's `messaging.send` is checked when it is made, against the capabilities its helper
/// holds, and those refuse a record over the session's message limit: the inline payload limit
/// on wire v1, the negotiated one on v2. The limit itself fits. A record they let through would
/// only fail as it is encoded for the wire, and that ends the client part.
#[test]
fn sends_over_the_session_message_limit_are_refused_at_send_time() {
    let negotiated = Limits {
        max_message_bytes: 20_000,
        ..Limits::host()
    };
    let channel = Channel {
        id: "bundle0.events1".into(),
        schema: API_VERSION,
        direction: Direction::ToServer,
        fields: vec![Field::Text {
            max_bytes: u16::MAX,
        }],
    };
    let manifest = Manifest {
        version: WIRE_VERSION,
        api: API_VERSION,
        id: "bundle0".into(),
        publisher_key: String::new(),
        package_version: "1.0.0".into(),
        permissions: BTreeSet::from([Permission::Messaging]),
        component: None,
        channels: vec![channel.clone()],
        actions: BTreeSet::new(),
        templates: BTreeSet::new(),
        files: Vec::new(),
    };
    // A send whose record is `bytes` long as JSON.
    let send = |bytes: usize| Command::Send {
        channel: channel.id.clone(),
        schema: channel.schema,
        record: vec![Scalar::Text(
            "x".repeat(bytes - r#"[{"type":"text","value":""}]"#.len()),
        )],
    };
    for wire in [
        Wire::v1(),
        Wire {
            version: MAX_WIRE_VERSION,
            limits: negotiated,
        },
    ] {
        let mut grant = fixture(1).grant;
        grant.wire = wire;
        let capabilities = capabilities(&grant, &manifest, BTreeSet::new());
        let limit = wire.limits.max_message_bytes as usize;
        capabilities.validate(&send(limit)).unwrap();
        assert!(
            capabilities.validate(&send(limit + 1)).is_err(),
            "{wire:?} admits a record over its message limit"
        );
    }
}

/// The fixture on wire v2, its channels lists of up to 4096 strings of up to 64 bytes.
fn fixture_v2(count: usize) -> Live<FakeWorker> {
    let mut live = fixture(count);
    live.grant.wire = Wire {
        version: MAX_WIRE_VERSION,
        limits: Limits::host(),
    };
    for instance in live.instances.values_mut() {
        instance.capabilities.max_message_bytes = live.grant.wire.limits.max_message_bytes;
        for channel in &mut instance.capabilities.channels {
            channel.fields = vec![Field::List {
                item: Box::new(Field::Text { max_bytes: 64 }),
                max_items: 4096,
            }];
        }
    }
    live
}

/// A list whose record JSON needs fragments at the host's limits.
fn large_list() -> Vec<Scalar> {
    vec![Scalar::List(
        (0..1000)
            .map(|i| Scalar::Text(format!("item {i} \u{e9}")))
            .collect(),
    )]
}

fn sent(live: &mut Live<FakeWorker>, id: &str, record: Vec<Scalar>, epoch: u64) {
    let instance = live.instances.get_mut(id).unwrap();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch,
        commands: vec![Command::Send {
            channel: format!("{id}.events1"),
            schema: API_VERSION,
            record,
        }],
    });
}

#[test]
fn a_v2_epoch_change_keeps_the_runtime_and_announces_the_new_epoch() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.poll(1, 0).unwrap();
    assert!(live.ready);
    let message = |live: &Live<FakeWorker>, sequence, world_epoch| {
        serde_json::to_vec(&Envelope {
            version: live.grant.wire.version,
            session: live.grant.session.clone(),
            connection: live.grant.connection.clone(),
            subclient: 0,
            bundle: "bundle0".into(),
            generation: INITIAL_BUNDLE_GENERATION,
            channel: "bundle0.events0".into(),
            schema: API_VERSION,
            sequence,
            world_epoch,
            payload: vec![Scalar::List(vec![Scalar::Text(format!("{sequence}"))])],
        })
        .unwrap()
    };
    // A callback dispatched in epoch 1 is still running when the world moves to epoch 2.
    live.receive(&message(&live, 1, 1), 1).unwrap();
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.receive(&message(&live, 2, 1), 2).unwrap();
    let packets = live.poll(2, 2 * CALLBACK_INTERVAL_MS).unwrap();
    let Control::Epoch {
        session,
        world_epoch,
    } = serde_json::from_slice(&packets[0]).unwrap()
    else {
        panic!("the first packet is not an epoch control");
    };
    assert_eq!((session.as_str(), world_epoch), ("session", 2));
    // The old callback's send follows, in its own epoch, which the server drops and counts.
    let late: Envelope = serde_json::from_slice(&packets[1]).unwrap();
    assert_eq!((late.sequence, late.world_epoch), (2, 1));
    // The old epoch's queued message is dropped and counted; the guest is told of the epoch.
    assert_eq!(live.ingress.stale, 1);
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(helper.dispatched.len(), 2);
    assert_eq!(helper.dispatched[1], Event::Epoch);
    // Its transaction carries the new epoch, and the new epoch's messages reach the guest.
    live.receive(&message(&live, 3, 2), 3 * CALLBACK_INTERVAL_MS)
        .unwrap();
    live.poll(2, 3 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(2, 4 * CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert!(matches!(
        &helper.dispatched[2],
        Event::Message { record, .. } if record == &serde_json::to_vec(&[Scalar::List(vec![Scalar::Text("3".into())])]).unwrap()
    ));
    // A later send carries the new epoch.
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 2);
    let packets = live.poll(2, 5 * CALLBACK_INTERVAL_MS).unwrap();
    let next: Envelope = serde_json::from_slice(&packets[0]).unwrap();
    assert_eq!((next.sequence, next.world_epoch), (3, 2));
}

#[test]
fn v2_records_over_the_inline_limit_travel_in_fragments_both_ways() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", large_list(), 1);
    let packets = live.poll(1, 0).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    let parts = packets[1..]
        .iter()
        .map(|bytes| serde_json::from_slice::<Fragment>(bytes).unwrap())
        .collect::<Vec<_>>();
    assert!(parts.len() > 1);
    assert!(parts.iter().all(|part| part.sequence == 1));
    assert_eq!(
        parts
            .iter()
            .map(|part| part.fragment.data.as_str())
            .collect::<String>(),
        serde_json::to_string(&large_list()).unwrap()
    );
    assert_eq!(live.sequence, 2);

    let incoming = Envelope {
        version: live.grant.wire.version,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: 0,
        bundle: "bundle0".into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: "bundle0.events0".into(),
        schema: API_VERSION,
        sequence: 1,
        world_epoch: 1,
        payload: large_list(),
    };
    for bytes in wire::encode(&incoming, &live.grant.wire).unwrap() {
        live.receive(&bytes, 1).unwrap();
    }
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(
        helper.dispatched,
        [Event::Message {
            channel: "bundle0.events0".into(),
            record: serde_json::to_vec(&large_list()).unwrap(),
        }]
    );
}

/// Starts one bundle, ready, whose helper fails every callback from now on.
fn failing() -> Live<FakeWorker> {
    let mut live = fixture(1);
    complete(&mut live, "bundle0");
    live.poll(1, 0).unwrap();
    assert!(live.ready);
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    live
}

/// Delivers one message to `bundle0` at `now_ms` and polls until its reply is handled.
fn fail_once(live: &mut Live<FakeWorker>, sequence: u64, now_ms: u64) {
    receive(live, "bundle0", sequence);
    live.poll(1, now_ms).unwrap();
    live.poll(1, now_ms).unwrap();
}

/// A failed callback is one dropped transaction: the helper is free for the next event, the
/// part keeps running and the session stays up until the strike limit.
#[test]
fn a_failed_callback_drops_its_transaction_and_frees_the_helper() {
    let mut live = failing();
    for strike in 1..MAX_GUEST_STRIKES {
        let now = strike as u64 * CALLBACK_INTERVAL_MS;
        fail_once(&mut live, strike as u64, now);
        let instance = &live.instances["bundle0"];
        assert!(!instance.busy, "strike {strike}");
        assert!(instance.helper.is_some(), "strike {strike}");
        assert_eq!(instance.helper.as_ref().unwrap().dispatched.len(), strike);
    }
    assert_eq!(
        live.text(),
        "Cinnabar: server code running (developer helper). F9: disable"
    );
}

/// `MAX_GUEST_STRIKES` failures within `GUEST_STRIKE_WINDOW_MS` stop the client part, which the
/// trusted status names; the session goes on.
#[test]
fn repeated_failures_stop_the_client_part_and_say_so() {
    let mut live = failing();
    for strike in 1..=MAX_GUEST_STRIKES {
        fail_once(
            &mut live,
            strike as u64,
            strike as u64 * CALLBACK_INTERVAL_MS,
        );
    }
    let instance = &live.instances["bundle0"];
    assert!(instance.helper.is_none() && !instance.busy);
    assert_eq!(
        live.text(),
        "Cinnabar: bundle0 client part stopped after repeated errors. F9: disable server code"
    );
    // Its later messages are dropped, and polling goes on.
    receive(&mut live, "bundle0", MAX_GUEST_STRIKES as u64 + 1);
    live.poll(1, 10 * CALLBACK_INTERVAL_MS).unwrap();
}

/// Failures farther apart than the window never add up to the limit.
#[test]
fn failures_outside_the_strike_window_do_not_stop_the_part() {
    let mut live = failing();
    for strike in 1..=2 * MAX_GUEST_STRIKES {
        fail_once(
            &mut live,
            strike as u64,
            strike as u64 * GUEST_STRIKE_WINDOW_MS,
        );
    }
    assert!(live.instances["bundle0"].helper.is_some());
}

/// A part that fails to start is stopped at once, without waiting for strikes.
#[test]
fn a_failed_start_stops_the_part() {
    let mut live = fixture(1);
    let helper = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap();
    helper.failure = Some(CallFailure {
        bundle: "bundle0".into(),
        callback: "init".into(),
        kind: FailureKind::Startup,
        reason: "import not found".into(),
        fuel: None,
    });
    live.poll(1, 0).unwrap();
    assert!(live.ready);
    assert!(live.instances["bundle0"].helper.is_none());
    assert_eq!(
        live.text(),
        "Cinnabar: bundle0 client part stopped because it failed to start. F9: disable server code"
    );
}

/// One bundle, ready, with its modal open.
fn opened() -> Live<FakeWorker> {
    let mut live = fixture(1);
    let owner = live.instances["bundle0"].owner.clone();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response = Some(Transaction {
        owner,
        epoch: live.epoch,
        commands: vec![Command::Screen {
            template: Some(SCREEN.into()),
        }],
    });
    live.poll(1, 0).unwrap();
    live
}

fn size(width: f64) -> GuiSize {
    GuiSize {
        width,
        height: 240.0,
        scale: 2.0,
    }
}

/// The open modal's size reaches its bundle as one `modal-resized` per change, the latest
/// replacing one still waiting, and every dispatch to it carries the size `ui.modal-size` reads.
#[test]
fn modal_size_changes_reach_the_open_bundle_once_each() {
    let mut live = opened();
    live.set_modal_size(Some(size(320.0)));
    live.set_modal_size(Some(size(320.0)));
    live.set_modal_size(Some(size(400.0)));
    let pending = &live.instances["bundle0"].events;
    assert_eq!(
        pending.iter().cloned().collect::<Vec<_>>(),
        [Event::Resized { size: size(400.0) }]
    );
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(helper.dispatched, [Event::Resized { size: size(400.0) }]);
    assert_eq!(helper.sizes, [Some(size(400.0))]);
    // Closed, there is no size and nothing to report.
    live.close_modal();
    live.set_modal_size(None);
    assert!(live.instances["bundle0"].events.is_empty());
}

/// Edits reach the open bundle only for a declared action's box, latest text per box.
#[test]
fn text_edits_reach_the_open_bundle_for_declared_boxes_only() {
    let mut live = opened();
    assert!(!live.text_changed("bundle0.other", "iron"));
    assert!(live.text_changed("bundle0.pick", "iro"));
    assert!(live.text_changed("bundle0.pick", "iron"));
    assert_eq!(
        live.instances["bundle0"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::Text {
            control: "bundle0.pick".into(),
            text: "iron".into()
        }]
    );
    live.close_modal();
    assert!(!live.text_changed("bundle0.pick", "gold"));
}

/// A declared scroll view's range reaches the open bundle, the latest replacing one still
/// waiting, so a fast scroll queues one callback; an undeclared view's does not.
#[test]
fn scroll_ranges_coalesce_for_declared_views_only() {
    let mut live = opened();
    let range = |offset| server_experience::screen::ScrollRange {
        offset,
        viewport: 72.0,
        content: 900.0,
    };
    assert!(!live.scroll_changed("bundle0.other", range(0.0)));
    assert!(live.scroll_changed("bundle0.pick", range(18.0)));
    assert!(live.scroll_changed("bundle0.pick", range(36.0)));
    assert!(!live.scroll_changed("bundle0.pick", range(f64::NAN)));
    assert_eq!(
        live.instances["bundle0"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::Scrolled {
            view: "bundle0.pick".into(),
            range: range(36.0)
        }]
    );
    live.close_modal();
    assert!(!live.scroll_changed("bundle0.pick", range(54.0)));
}

/// A secondary press (a right click) reaches the open bundle as its own event, only for a
/// declared action, as primary presses do.
#[test]
fn secondary_presses_reach_only_declared_actions() {
    let mut live = opened();
    assert!(!live.press_secondary("bundle0.other", Some(2)));
    assert!(live.press_secondary("bundle0.pick", Some(2)));
    assert_eq!(
        live.instances["bundle0"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::SecondaryAction {
            id: "bundle0.pick".into(),
            index: Some(2)
        }]
    );
}

const INTRO: &str = "media/clip.json";

/// Grants one fixture bundle the media and scene adapters and an indexed descriptor.
fn grant_media(live: &mut Live<FakeWorker>, id: &str) {
    let capabilities = &mut live.instances.get_mut(id).unwrap().capabilities;
    capabilities
        .scope
        .permissions
        .extend([Permission::Media, Permission::Scene]);
    capabilities.assets.insert(INTRO.into());
}

fn media_quad(x: f32) -> server_experience::runtime::SceneObject {
    server_experience::runtime::SceneObject::Quad {
        texture: INTRO.into(),
        transform: [x, 64.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        size: [16.0, 9.0],
    }
}

#[test]
fn guest_media_controls_reach_the_player_and_transitions_return_to_the_guest() {
    let mut live = fixture(1);
    grant_media(&mut live, "bundle0");
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![Command::Media {
            id: INTRO.into(),
            operation: server_experience::runtime::MediaOperation::Play,
            position_ms: 0,
        }],
    });
    live.poll(1, 0).unwrap();
    // No verified descriptor was registered, so the player is refused and reported stopped.
    live.media_mut().service(0, 0, true);
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    let [Event::Message { channel, record }] = helper.dispatched.as_slice() else {
        panic!("one media event, not {:?}", helper.dispatched);
    };
    assert_eq!(channel, super::super::media::EVENT_CHANNEL);
    let record: Vec<Scalar> = serde_json::from_slice(record).unwrap();
    assert!(matches!(
        record.as_slice(),
        [Scalar::Text(path), Scalar::Choice(2), Scalar::Integer(0)] if path == INTRO
    ));
}

#[test]
fn only_a_bundles_own_quad_textured_by_its_playing_media_becomes_a_textured_screen() {
    let mut live = fixture(2);
    for id in ["bundle0", "bundle1"] {
        let contributions = &mut live.instances.get_mut(id).unwrap().contributions;
        contributions
            .scene
            .insert(1, media_quad(if id == "bundle0" { 5.0 } else { 9.0 }));
    }
    assert!(
        live.changed_screens().unwrap().is_empty(),
        "no player means no screen"
    );
    let frame = render::MediaFrame {
        serial: 3,
        width: 2,
        height: 2,
        rgba: std::sync::Arc::from(vec![255u8; 16]),
    };
    live.media_mut().set_frame("bundle0", INTRO, Some(frame));
    let screens = live.changed_screens().unwrap();
    assert_eq!(screens.len(), 1);
    assert_eq!(screens[0].center, [5.0, 64.0, 0.0]);
    assert_eq!(screens[0].half_right, [8.0, 0.0, 0.0]);
    assert_eq!(screens[0].half_up, [0.0, 4.5, 0.0]);
    assert_eq!(screens[0].frame.as_ref().map(|frame| frame.serial), Some(3));
}

#[test]
fn an_unchanged_media_scene_is_presented_without_rebuilding() {
    let mut live = fixture(1);
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .contributions
        .scene
        .insert(1, media_quad(5.0));
    let frame = render::MediaFrame {
        serial: 3,
        width: 2,
        height: 2,
        rgba: std::sync::Arc::from(vec![255u8; 16]),
    };
    live.media_mut().set_frame("bundle0", INTRO, Some(frame));
    assert_eq!(live.changed_screens().map(<[_]>::len), Some(1));
    let before = crate::tests::alloc_count::thread_allocations();
    let unchanged = live.changed_screens().is_none();
    assert_eq!(crate::tests::alloc_count::thread_allocations() - before, 0);
    assert!(unchanged);
}
