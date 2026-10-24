use super::*;

/// Starts registry-aware helpers with one open modal, without running authored callbacks.
fn negotiating(count: usize) -> Live<FakeWorker> {
    let mut live = fixture_v2(count);
    for (bundle, instance) in &mut live.instances {
        let mut channel = instance.capabilities.channels[0].clone();
        channel.id = format!("{bundle}.registry");
        instance.capabilities.channels.push(channel.clone());
        let helper = instance.helper.as_mut().unwrap();
        helper.registry = Some(channel.id.clone());
        helper.confirmation = Some(channel.id);
        helper.response = Some(Transaction {
            owner: instance.owner.clone(),
            epoch: 1,
            commands: Vec::new(),
        });
    }
    live.poll(1, 0).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.contributions.modal.open(Some(SCREEN.into()));
    instance.opened = 1;
    live
}

/// Admits a schema-checked message into the same ingress the live scheduler drains.
fn enqueue(live: &mut Live<FakeWorker>, sequence: u64, epoch: u64, channel: &str, text: &str) {
    let message = Envelope {
        version: live.grant.wire.version,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: 0,
        bundle: channel.split('.').next().unwrap().into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: channel.into(),
        schema: API_VERSION,
        sequence,
        world_epoch: epoch,
        payload: vec![Scalar::List(
            text.as_bytes()
                .chunks(64)
                .map(|chunk| Scalar::Text(String::from_utf8(chunk.to_vec()).unwrap()))
                .collect(),
        )],
    };
    for bytes in wire::encode(&message, &live.grant.wire).unwrap() {
        live.receive(&bytes, sequence * 1000, live.epoch).unwrap();
    }
}

/// An early replay trap consumes only its own event; messages and actions wait for fresh init.
#[test]
fn early_replay_trap_preserves_later_events_for_fresh_confirmation() {
    let mut live = negotiating(1);
    enqueue(&mut live, 1, 1, "bundle0.events0", "first");
    enqueue(&mut live, 2, 1, "bundle0.events0", "later");
    enqueue(&mut live, 3, 1, "bundle0.registry", "confirmed");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.press("bundle0.pick", Some(7)));
    assert!(live.press_secondary("bundle0.pick", Some(8)));
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(pending_messages(&live, "bundle0"), 1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert_eq!(instance.queue.entries.len(), 3);
    assert_eq!(instance.waiting_for.as_deref(), Some("bundle0.registry"));
    instance.helper.as_mut().unwrap().fail = false;
    enqueue(&mut live, 4, 1, "bundle0.registry", "fresh");
    for slice in 4..=8 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let dispatched = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert!(matches!(dispatched.as_slice(), [
        Event::Message { channel: first, .. }, Event::Message { channel: failed, .. },
        Event::Message { channel: fresh, .. }, Event::Message { channel: later, .. },
        Event::Action { index: Some(7), .. }, Event::SecondaryAction { index: Some(8), .. },
    ] if first == "bundle0.registry" && failed == "bundle0.events0" && fresh == first && later == failed));
    assert!(
        live.instances
            .values()
            .all(|instance| instance.queue.entries.is_empty())
    );
    assert!(live.instances["bundle0"].queue.entries.is_empty());
}

#[test]
fn pending_events_follow_latest_state_in_arrival_order() {
    let mut live = negotiating(1);
    assert!(live.press("bundle0.pick", Some(1)));
    assert!(live.text_changed("bundle0.pick", "old"));
    assert!(live.shift_changed(true));
    assert!(live.text_changed("bundle0.pick", "latest"));
    assert!(live.press_secondary("bundle0.pick", Some(2)));
    enqueue(&mut live, 1, 1, "bundle0.events0", "first");
    enqueue(&mut live, 2, 1, "bundle0.events0", "second");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .is_empty()
    );
    enqueue(&mut live, 3, 1, "bundle0.registry", "confirmed");
    for slice in 2..=9 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let dispatched = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert!(matches!(dispatched.as_slice(), [
        Event::Message { channel, .. }, Event::Text { text, .. }, Event::Shift { held: true },
        Event::Action { index: Some(1), .. }, Event::SecondaryAction { index: Some(2), .. },
        Event::Message { record: first, .. }, Event::Message { record: second, .. },
    ] if channel == "bundle0.registry" && text == "latest" && first != second));
}

#[test]
fn pending_messages_stay_in_their_world_epoch() {
    let mut live = negotiating(1);
    enqueue(&mut live, 1, 1, "bundle0.events0", "old");
    enqueue(&mut live, 2, 1, "bundle0.registry", "old confirmation");
    live.poll(2, CALLBACK_INTERVAL_MS).unwrap();
    live.poll(2, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.ingress.stale, 2);
    enqueue(&mut live, 3, 2, "bundle0.events0", "older");
    live.poll(3, 3 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(3, 4 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.ingress.stale, 3);
    enqueue(&mut live, 4, 3, "bundle0.events0", "first");
    enqueue(&mut live, 5, 3, "bundle0.events0", "second");
    enqueue(&mut live, 6, 3, "bundle0.registry", "confirmed");
    for slice in 5..=10 {
        live.poll(3, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let dispatched = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert!(matches!(dispatched.as_slice(), [
        Event::Message { channel, .. }, Event::Epoch,
        Event::Message { record: first, .. }, Event::Message { record: second, .. },
    ] if channel == "bundle0.registry" && first != second));
    assert!(live.instances["bundle0"].queue.entries.is_empty());
}

#[test]
fn not_ready_keeps_the_dispatched_message_in_its_recipient_queue() {
    let mut live = negotiating(1);
    // The helper is authoritative even if the caller has not learned its readiness yet.
    live.instances.get_mut("bundle0").unwrap().waiting_for = None;
    enqueue(&mut live, 1, 1, "bundle0.events0", "waiting");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(pending_messages(&live, "bundle0"), 1);
    assert!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .is_empty()
    );
    enqueue(&mut live, 2, 1, "bundle0.registry", "confirmed");
    for slice in 3..=5 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(
        live.instances
            .values()
            .all(|instance| instance.queue.entries.is_empty())
    );
}

#[test]
fn an_old_world_not_ready_reply_counts_the_undelivered_message_as_stale() {
    let mut live = negotiating(1);
    live.instances.get_mut("bundle0").unwrap().waiting_for = None;
    enqueue(&mut live, 1, 1, "bundle0.events0", "old");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    // The epoch changes while the helper's NotReady reply is still pending.
    live.poll(2, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(live.ingress.stale, 1);
    enqueue(&mut live, 2, 2, "bundle0.registry", "confirmed");
    for slice in 3..=6 {
        live.poll(2, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(
        matches!(live.instances["bundle0"].helper.as_ref().unwrap().dispatched.as_slice(),
        [Event::Message { channel, .. }, Event::Epoch] if channel == "bundle0.registry")
    );
    assert!(live.instances["bundle0"].queue.entries.is_empty());
}

/// Both reliable queue bounds stop the overflowing part without stopping another recipient.
#[test]
fn count_and_byte_overflow_stop_only_the_affected_part() {
    for text in [String::new(), "x".repeat(10_000)] {
        let mut live = negotiating(2);
        enqueue(&mut live, 1, 1, "bundle1.events0", "other");
        let mut sequence = 1;
        while live.instances["bundle0"].stopped.is_none() {
            sequence += 1;
            assert!(sequence <= MAX_QUEUE_MESSAGES as u64 + 2);
            enqueue(&mut live, sequence, 1, "bundle0.events0", &text);
        }
        assert_eq!(live.instances["bundle0"].stopped, Some(STOPPED_AT_OVERFLOW));
        assert!(live.instances["bundle1"].helper.is_some());
        assert!(live.text().contains(STOPPED_AT_OVERFLOW));
        assert_eq!(pending_messages(&live, "bundle1"), 1);
        assert!(live.instances["bundle0"].queue.entries.is_empty());
        enqueue(&mut live, sequence + 1, 1, "bundle1.registry", "confirmed");
        for slice in 1..=3 {
            live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
        }
        assert!(
            live.instances
                .values()
                .all(|instance| instance.queue.entries.is_empty())
        );
    }
}

/// A full input queue must not silently drop a host-generated state callback.
#[test]
fn modal_state_overflow_stops_the_part() {
    let mut live = negotiating(1);
    while live.press("bundle0.pick", None) {}
    live.set_modal_size(Some(size(320.0)));
    let instance = &live.instances["bundle0"];
    assert_eq!(instance.stopped, Some(STOPPED_AT_OVERFLOW));
    assert!(instance.helper.is_none());
    assert!(instance.strikes.is_empty());
}

#[test]
fn resize_overflow_removes_cached_media_screen() {
    let mut live = negotiating(1);
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .contributions
        .scene
        .insert(1, media_quad(5.0));
    live.media_mut().set_frame(
        "bundle0",
        INTRO,
        Some(render_scene::MediaFrame {
            serial: 3,
            width: 2,
            height: 2,
            rgba: Arc::from(vec![255u8; 16]),
        }),
    );
    assert_eq!(live.changed_screens().map(<[_]>::len), Some(1));
    assert!(live.changed_screens().is_none());
    while live.press("bundle0.pick", None) {}
    live.set_modal_size(Some(size(320.0)));
    assert_eq!(live.instances["bundle0"].stopped, Some(STOPPED_AT_OVERFLOW));
    assert_eq!(live.changed_screens().map(<[_]>::len), Some(0));
    assert!(live.changed_screens().is_none());
}

#[test]
fn idle_recipients_dispatch_in_one_poll() {
    let mut live = fixture(2);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, 0).unwrap();
    receive(&mut live, "bundle0", 1);
    receive(&mut live, "bundle1", 2);
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    for bundle in ["bundle0", "bundle1"] {
        assert!(matches!(
            live.instances[bundle].helper.as_ref().unwrap().dispatched.as_slice(),
            [Event::Message { channel, .. }] if channel == &format!("{bundle}.events0")
        ));
    }
    // Dispatch alone must not acknowledge either message.
    assert_eq!(pending_messages(&live, "bundle0"), 1);
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(
        live.instances
            .values()
            .all(|instance| instance.queue.entries.is_empty())
    );
}

#[test]
fn busy_recipient_retains_order_without_blocking_another_helper() {
    let mut live = fixture(2);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, 0).unwrap();
    receive(&mut live, "bundle0", 1);
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    // Hold its first callback open while later messages arrive for both recipients.
    let response = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response
        .take();
    receive(&mut live, "bundle0", 2);
    receive(&mut live, "bundle0", 3);
    receive(&mut live, "bundle1", 4);
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .len(),
        1
    );
    assert_eq!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .len(),
        1
    );
    assert_eq!(pending_messages(&live, "bundle0"), 3);
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response = response;
    for slice in 3..=5 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let records: Vec<Vec<Scalar>> = live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched
        .iter()
        .map(|event| {
            let Event::Message { record, .. } = event else {
                panic!("expected a message")
            };
            serde_json::from_slice(record).unwrap()
        })
        .collect();
    assert_eq!(
        records,
        [
            vec![Scalar::Bool(false)],
            vec![Scalar::Bool(true)],
            vec![Scalar::Bool(false)]
        ]
    );
    assert!(
        live.instances
            .values()
            .all(|instance| instance.queue.entries.is_empty())
    );
}

/// A newer coalesced state must survive acknowledgement of the in-flight older value.
#[test]
fn in_flight_state_acknowledgement_keeps_a_newer_value() {
    let mut live = opened();
    assert!(live.text_changed("bundle0.pick", "first"));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.text_changed("bundle0.pick", "latest"));
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert!(
        matches!(helper.dispatched.as_slice(), [Event::Text { text: first, .. }, Event::Text { text: latest, .. }]
        if first == "first" && latest == "latest")
    );
    assert!(live.instances["bundle0"].queue.entries.is_empty());
}

#[test]
fn initialization_state_replay_happens_only_once() {
    let mut live = negotiating(1);
    assert!(live.press("bundle0.pick", Some(1)));
    assert!(live.text_changed("bundle0.pick", "old"));
    assert!(live.press_secondary("bundle0.pick", Some(2)));
    assert!(live.text_changed("bundle0.pick", "latest"));
    enqueue(&mut live, 1, 1, "bundle0.registry", "confirmed");
    for slice in 1..=5 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"].helper.as_ref().unwrap().dispatched.as_slice(),
        [Event::Message { .. }, Event::Text { text, .. }, Event::Action { .. }, Event::SecondaryAction { .. }]
        if text == "latest"
    ));
    assert!(live.press("bundle0.pick", Some(3)));
    assert!(live.text_changed("bundle0.pick", "after initialization"));
    for slice in 6..=8 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let dispatched = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert!(matches!(
        &dispatched[4..],
        [Event::Action { index: Some(3), .. }, Event::Text { .. }]
    ));
}

/// Counts retained wire and media messages for one recipient, including in-flight work.
fn pending_messages(live: &Live<FakeWorker>, bundle: &str) -> usize {
    live.instances[bundle]
        .queue
        .entries
        .iter()
        .filter(|entry| matches!(entry.event, Event::Message { .. }))
        .count()
}
