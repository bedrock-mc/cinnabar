use crate::{
    helper::Event,
    server::{
        BundleHost, DispatchResult,
        tests::{on, owner, screen_capabilities},
    },
};
use server_experience::{
    manifest::Permission,
    runtime::{Command, Transaction},
    wire::{Channel, Direction, Field, Scalar},
};

/// Grants the registry-aware probe its declared UI, input and handshake channels.
fn waiting() -> BundleHost {
    let mut capabilities = screen_capabilities(&[
        Permission::Ui,
        Permission::ModalUi,
        Permission::Input,
        Permission::Messaging,
    ]);
    for (id, direction) in [
        ("demo.registry", Direction::ToClient),
        ("demo.registry_request", Direction::ToServer),
    ] {
        capabilities.channels.push(Channel {
            id: id.into(),
            schema: 1,
            direction,
            fields: vec![Field::Text { max_bytes: 64 }],
        });
    }
    BundleHost::launch(
        include_bytes!("../guest_negotiation.wat"),
        owner(),
        capabilities,
        1,
    )
    .unwrap()
}

/// The successful registry reply that runs the probe's authored initializer.
fn confirmation() -> Event {
    Event::Message {
        channel: "demo.registry".into(),
        record: br#"[{"type":"text","value":"expected"}]"#.to_vec(),
    }
}

/// Extracts the probe's observable callback log from its committed UI commands.
fn widgets(transaction: &Transaction) -> Vec<(&str, &str)> {
    transaction
        .commands
        .iter()
        .filter_map(|command| match command {
            Command::Widget { id, text } => Some((id.as_str(), text.as_str())),
            _ => None,
        })
        .collect()
}

/// Checks that negotiation publishes exactly the generated request and no authored output.
fn assert_request(transaction: Transaction, epoch: u64) {
    assert_eq!(transaction.epoch, epoch);
    assert!(
        matches!(transaction.commands.as_slice(),
        [Command::Send { channel, schema: 1, record }]
        if channel == "demo.registry_request" && record == &[Scalar::Text("expected".into())]),
        "{transaction:?}"
    );
}

/// Deferred callbacks return no authored output and consume no guest fuel.
#[test]
fn callbacks_remain_with_the_caller_until_confirmation() {
    let mut host = waiting();
    assert_request(host.take_transaction(), 1);
    let events = [
        Event::Shift { held: true },
        on("demo.first"),
        Event::Action {
            id: "demo.pick".into(),
            index: Some(7),
        },
    ];
    for event in &events {
        let DispatchResult::NotReady(transaction) = host.dispatch(event, 1).unwrap() else {
            panic!("an unconfirmed callback must remain queued");
        };
        assert!(transaction.commands.is_empty());
        assert_eq!(host.last_fuel_used(), 0);
    }
    let init = host
        .dispatch(&confirmation(), 1)
        .unwrap()
        .into_transaction();
    assert_eq!(widgets(&init), [("init", "init")]);
    let mut log = Vec::new();
    for event in events {
        let DispatchResult::Committed(transaction) = host.dispatch(&event, 1).unwrap() else {
            panic!("confirmed callbacks must run normally");
        };
        log.extend(
            widgets(&transaction)
                .into_iter()
                .map(|(id, text)| (id.to_owned(), text.to_owned())),
        );
    }
    assert_eq!(
        log,
        [
            ("shift", "held"),
            ("demo.first", "[]"),
            ("demo.pick", "primary")
        ]
        .map(|(id, text)| (id.to_owned(), text.to_owned()))
    );
}

#[test]
fn pending_negotiation_resends_once_per_new_epoch_without_consuming_state() {
    let mut host = waiting();
    assert_request(host.take_transaction(), 1);
    for epoch in 2..=3 {
        let DispatchResult::NotReady(transaction) = host.dispatch(&Event::Epoch, epoch).unwrap()
        else {
            panic!("epoch remains pending");
        };
        assert_request(transaction, epoch);
        assert!(
            host.dispatch(&Event::Epoch, epoch)
                .unwrap()
                .into_transaction()
                .commands
                .is_empty()
        );
    }
    let init = host
        .dispatch(&confirmation(), 3)
        .unwrap()
        .into_transaction();
    assert_eq!(init.epoch, 3);
    assert_eq!(widgets(&init), [("init", "init")]);
    let epoch = host.dispatch(&Event::Epoch, 3).unwrap().into_transaction();
    assert_eq!(widgets(&epoch), [("epoch", "epoch")]);
}

/// A failed callback leaves later caller-owned work available after a fresh confirmation.
#[test]
fn early_replay_trap_preserves_later_events_for_fresh_confirmation() {
    let mut host = waiting();
    host.take_transaction();
    let mut pending = std::collections::VecDeque::from([
        Event::Message {
            channel: "demo.fail".into(),
            record: Vec::new(),
        },
        on("demo.after"),
        Event::Action {
            id: "demo.pick".into(),
            index: None,
        },
    ]);
    assert!(matches!(
        host.dispatch(pending.front().unwrap(), 1).unwrap(),
        DispatchResult::NotReady(_)
    ));
    let init = host
        .dispatch(&confirmation(), 1)
        .unwrap()
        .into_transaction();
    assert_eq!(widgets(&init), [("init", "init")]);
    assert!(host.dispatch(pending.front().unwrap(), 1).is_err());
    pending.pop_front();
    assert_request(host.take_transaction(), 1);
    assert!(matches!(
        host.dispatch(pending.front().unwrap(), 1).unwrap(),
        DispatchResult::NotReady(_)
    ));
    assert_eq!(pending.len(), 2);
    assert_eq!(
        widgets(
            &host
                .dispatch(&confirmation(), 1)
                .unwrap()
                .into_transaction()
        ),
        [("init", "init")]
    );
    for expected in [[("demo.after", "[]")], [("demo.pick", "primary")]] {
        let DispatchResult::Committed(transaction) =
            host.dispatch(pending.front().unwrap(), 1).unwrap()
        else {
            panic!("fresh confirmation must release the remaining callbacks");
        };
        pending.pop_front();
        assert_eq!(widgets(&transaction), expected);
        assert!(
            host.last_fuel_used() > 0
                && host.last_fuel_used() < server_experience::runtime::CALLBACK_FUEL
        );
    }
    assert!(pending.is_empty());
}

/// An incompatible reply ends negotiation without restarting or running queued callbacks.
#[test]
fn registry_mismatch_quarantines_the_waiting_client() {
    let mut host = waiting();
    host.take_transaction();
    let pending = on("demo.queued");
    assert!(matches!(
        host.dispatch(&pending, 1).unwrap(),
        DispatchResult::NotReady(_)
    ));
    let error = host
        .dispatch(
            &Event::Message {
                channel: "demo.registry".into(),
                record: br#"[{"type":"text","value":"changed-server"}]"#.to_vec(),
            },
            1,
        )
        .unwrap_err();
    let failure = crate::helper::CallFailure::of("demo", "dispatch", &error);
    assert_eq!(failure.kind, crate::helper::FailureKind::RegistryMismatch);
    assert_eq!(failure.reason, experience_sdk::REGISTRY_MISMATCH);
    assert!(failure.reason.contains("rebuild"));
    assert!(host.waiting_for().is_none());
    assert!(host.take_transaction().commands.is_empty());
    assert!(host.dispatch(&pending, 1).is_err());
}
