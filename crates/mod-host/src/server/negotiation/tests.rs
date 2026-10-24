use super::*;
use crate::{
    helper::FailureKind,
    server::{
        BundleHost,
        tests::{on, owner, screen_capabilities, typed},
    },
};
use server_experience::{
    manifest::Permission,
    runtime::Transaction,
    screen,
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

#[test]
fn restart_with_an_open_modal_replays_held_shift_after_init() {
    let mut host = waiting();
    assert_request(host.take_transaction(), 1);
    let init = host.dispatch(&confirmation(), 1).unwrap();
    assert!(init.commands.iter().any(|command| matches!(command,
        Command::Screen { template: Some(template) } if template == "ui/terminal.json")));
    host.set_modal_size(Some(screen::GuiSize {
        width: 320.0,
        height: 240.0,
        scale: 2.0,
    }));
    host.dispatch(&Event::Shift { held: true }, 1).unwrap();
    assert!(
        host.dispatch(
            &Event::Message {
                channel: "demo.fail".into(),
                record: vec![]
            },
            1
        )
        .is_err()
    );
    assert_request(host.take_transaction(), 1);
    assert!(
        host.dispatch(&Event::Shift { held: true }, 1)
            .unwrap()
            .commands
            .is_empty()
    );
    let replay = host.dispatch(&confirmation(), 1).unwrap();
    assert_eq!(widgets(&replay), [("init", "init"), ("shift", "held")]);
}

#[test]
fn pending_events_follow_latest_state_in_arrival_order() {
    let mut host = waiting();
    host.take_transaction();
    let action = Event::Action {
        id: "demo.pick".into(),
        index: Some(7),
    };
    let secondary = Event::SecondaryAction {
        id: "demo.pick".into(),
        index: None,
    };
    for event in [
        on("demo.first"),
        Event::Shift { held: false },
        action,
        Event::Shift { held: true },
        on("demo.second"),
        secondary,
    ] {
        assert!(host.dispatch(&event, 1).unwrap().commands.is_empty());
    }
    let replay = host.dispatch(&confirmation(), 1).unwrap();
    assert_eq!(
        widgets(&replay),
        [
            ("init", "init"),
            ("shift", "held"),
            ("demo.first", "[]"),
            ("demo.pick", "primary"),
            ("demo.second", "[]"),
            ("demo.pick", "secondary"),
        ]
    );
    assert_eq!(
        widgets(&host.dispatch(&on("demo.live"), 1).unwrap()),
        [("demo.live", "[]")]
    );
}

#[test]
fn pending_negotiation_resends_once_per_new_epoch_without_dropping_callbacks() {
    let mut host = waiting();
    assert_request(host.take_transaction(), 1);
    for epoch in 2..=3 {
        assert_request(host.dispatch(&Event::Epoch, epoch).unwrap(), epoch);
        assert!(
            host.dispatch(&Event::Shift { held: true }, epoch)
                .unwrap()
                .commands
                .is_empty()
        );
    }
    let replay = host.dispatch(&confirmation(), 3).unwrap();
    assert_eq!(replay.epoch, 3);
    assert_eq!(
        widgets(&replay),
        [("init", "init"), ("epoch", "epoch"), ("shift", "held")]
    );
}

#[test]
fn pending_messages_stay_in_their_world_epoch() {
    let mut host = waiting();
    host.take_transaction();
    host.dispatch(&on("demo.old"), 1).unwrap();
    assert_request(host.dispatch(&Event::Epoch, 2).unwrap(), 2);
    host.dispatch(&on("demo.older"), 2).unwrap();
    assert_request(host.dispatch(&Event::Epoch, 3).unwrap(), 3);
    for channel in ["demo.first", "demo.second"] {
        assert!(host.dispatch(&on(channel), 3).unwrap().commands.is_empty());
    }
    let replay = host.dispatch(&confirmation(), 3).unwrap();
    assert_eq!(replay.epoch, 3);
    assert_eq!(
        widgets(&replay),
        [
            ("init", "init"),
            ("epoch", "epoch"),
            ("demo.first", "[]"),
            ("demo.second", "[]"),
        ]
    );
}

#[test]
fn confirmation_in_a_new_epoch_discards_pending_old_messages() {
    let mut host = waiting();
    host.take_transaction();
    host.dispatch(&on("demo.old"), 1).unwrap();
    let replay = host.dispatch(&confirmation(), 2).unwrap();
    assert_eq!(replay.epoch, 2);
    assert_eq!(widgets(&replay), [("init", "init")]);
}

#[test]
fn world_change_releases_stale_message_count_and_bytes() {
    for record in [vec![], vec![b' '; MAX_MESSAGE_BYTES]] {
        let mut pending = Negotiation::new("registry".into(), Command::Screen { template: None });
        pending.hold(&Event::Epoch, 1).unwrap();
        let message = Event::Message {
            channel: "demo.event".into(),
            record,
        };
        let mut capacity = 0;
        while pending.hold(&message, 1).is_ok() {
            capacity += 1;
        }
        assert!(capacity > 0);
        pending.hold(&Event::Epoch, 2).unwrap();
        for _ in 0..capacity {
            pending.hold(&message, 2).unwrap();
        }
        assert!(pending.hold(&message, 2).unwrap_err().is::<Overflow>());
        assert_eq!(
            pending.into_callbacks(2).collect::<Vec<_>>(),
            std::iter::once(Event::Epoch)
                .chain(std::iter::repeat_n(message, capacity))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn latest_state_is_coalesced_separately_for_each_control() {
    let mut pending = Negotiation::new("registry".into(), Command::Screen { template: None });
    let size = |width| Event::Resized {
        size: screen::GuiSize {
            width,
            height: 240.0,
            scale: 2.0,
        },
    };
    let scroll = |view: &str, offset| Event::Scrolled {
        view: view.into(),
        range: screen::ScrollRange {
            offset,
            viewport: 10.0,
            content: 100.0,
        },
    };
    let latest = [
        size(640.0),
        typed("search", "latest"),
        scroll("list", 50.0),
        Event::Shift { held: false },
    ];
    let other = [typed("other", "keep"), scroll("other", 20.0)];
    for event in [
        size(320.0),
        typed("search", "old"),
        scroll("list", 0.0),
        Event::Shift { held: true },
    ] {
        pending.hold(&event, 1).unwrap();
    }
    for event in other.iter().chain(latest.iter()) {
        pending.hold(event, 1).unwrap();
    }
    assert_eq!(
        pending.into_callbacks(1).collect::<Vec<_>>(),
        latest.into_iter().chain(other).collect::<Vec<_>>()
    );
}

#[test]
fn negotiation_queue_overflow_is_terminal_and_publishes_nothing() {
    let mut host = waiting();
    host.take_transaction();
    for _ in 0..MAX_QUEUE_MESSAGES {
        assert!(
            host.dispatch(&on("demo.event"), 1)
                .unwrap()
                .commands
                .is_empty()
        );
    }
    let error = host.dispatch(&on("demo.event"), 1).unwrap_err();
    let failure = crate::helper::CallFailure::of("demo", "dispatch", &error);
    assert_eq!(failure.kind, FailureKind::Overload);
    assert!(failure.reason.contains("queue overflow"));
    assert!(host.take_transaction().commands.is_empty());
    assert!(host.dispatch(&confirmation(), 1).is_err());
}

#[test]
fn negotiation_queue_bounds_bytes_as_well_as_callback_count() {
    let mut host = waiting();
    host.take_transaction();
    let message = Event::Message {
        channel: "demo.event".into(),
        record: vec![b' '; MAX_MESSAGE_BYTES],
    };
    let mut retained = 0;
    loop {
        match host.dispatch(&message, 1) {
            Ok(transaction) => {
                assert!(transaction.commands.is_empty());
                retained += 1;
            }
            Err(error) => {
                assert!(error.is::<Overflow>());
                break;
            }
        }
    }
    assert!(retained > 0 && retained < MAX_QUEUE_MESSAGES);
    assert!(host.take_transaction().commands.is_empty());
}
