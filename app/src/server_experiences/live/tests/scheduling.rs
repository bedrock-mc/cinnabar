use super::*;

#[test]
fn an_action_keeps_its_shift_state_until_it_runs() {
    let mut live = opened();
    assert!(live.shift_changed(true));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.press("bundle0.pick", None));
    assert!(live.shift_changed(false));
    for slice in 2..=4 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .as_slice(),
        [
            Event::Shift { held: true },
            Event::Action { .. },
            Event::Shift { held: false }
        ]
    ));
}

#[test]
fn repeated_scroll_updates_cannot_postpone_an_earlier_action() {
    let mut live = opened();
    assert!(live.press("bundle0.pick", None));
    for slice in 1..=4 {
        assert!(live.scroll_changed(
            "bundle0.pick",
            screen::ScrollRange {
                offset: slice as f64,
                viewport: 10.0,
                content: 100.0,
            }
        ));
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .first(),
        Some(Event::Action { .. })
    ));
}

#[test]
fn independent_media_recipients_dispatch_in_one_poll() {
    let mut live = fixture(2);
    for bundle in ["bundle0", "bundle1"] {
        complete(&mut live, bundle);
        let owner = live.instances[bundle].owner.clone();
        live.media_mut()
            .queue(
                &owner,
                INTRO.into(),
                server_experience::runtime::MediaOperation::Play,
                0,
            )
            .unwrap();
    }
    live.poll(1, 0).unwrap();
    live.service_media(0, 0, true);
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    for bundle in ["bundle0", "bundle1"] {
        assert!(matches!(
            live.instances[bundle].helper.as_ref().unwrap().dispatched.as_slice(),
            [Event::Message { channel, .. }] if channel == super::super::super::media::EVENT_CHANNEL
        ));
    }
}

#[test]
fn messages_media_and_input_share_arrival_order() {
    let mut live = opened();
    receive(&mut live, "bundle0", 1);
    assert!(live.press("bundle0.pick", Some(1)));
    let owner = live.instances["bundle0"].owner.clone();
    live.media_mut()
        .queue(
            &owner,
            INTRO.into(),
            server_experience::runtime::MediaOperation::Play,
            0,
        )
        .unwrap();
    live.service_media(0, 0, true);
    assert!(live.text_changed("bundle0.pick", "later"));
    receive(&mut live, "bundle0", 2);
    for slice in 1..=6 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"].helper.as_ref().unwrap().dispatched.as_slice(),
        [Event::Message { channel: first, .. }, Event::Action { .. },
         Event::Message { channel: media, .. }, Event::Text { .. },
         Event::Message { channel: last, .. }]
        if first == "bundle0.events0" && last == first && media == super::super::super::media::EVENT_CHANNEL
    ));
}

#[test]
fn state_on_each_side_of_an_action_keeps_both_values() {
    let mut live = opened();
    assert!(live.shift_changed(true));
    assert!(live.press("bundle0.pick", None));
    assert!(live.shift_changed(false));
    for slice in 1..=4 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .as_slice(),
        [
            Event::Shift { held: true },
            Event::Action { .. },
            Event::Shift { held: false }
        ]
    ));
}

#[test]
fn recipients_rotate_when_the_callback_budget_runs_out() {
    let mut live = fixture(3);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    complete(&mut live, "bundle2");
    live.poll(1, CALLBACK_INTERVAL_MS + 1).unwrap();
    for (index, bundle) in ["bundle0", "bundle0", "bundle1", "bundle1", "bundle2"]
        .into_iter()
        .enumerate()
    {
        receive(&mut live, bundle, index as u64 + 1);
    }
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(
        live.instances["bundle2"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .is_empty()
    );
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(
        live.instances["bundle2"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .len(),
        1
    );
}

#[test]
fn new_world_messages_arriving_before_poll_survive_the_epoch_change() {
    let mut live = fixture_v2(1);
    sent(&mut live, "bundle0", vec![Scalar::List(Vec::new())], 1);
    live.poll(1, 0).unwrap();
    let message = Envelope {
        version: live.grant.wire.version,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: 0,
        bundle: "bundle0".into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: "bundle0.events0".into(),
        schema: API_VERSION,
        sequence: 1,
        world_epoch: 2,
        payload: vec![Scalar::List(Vec::new())],
    };
    live.receive(&serde_json::to_vec(&message).unwrap(), 1, 2)
        .unwrap();
    for slice in 1..=3 {
        live.poll(2, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(matches!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .as_slice(),
        [Event::Epoch, Event::Message { .. }]
    ));
    assert_eq!(live.ingress.stale, 0);
}

#[test]
fn observing_a_world_before_input_preserves_new_world_actions_and_state() {
    let mut live = opened();
    live.grant.wire = Wire {
        version: MAX_WIRE_VERSION,
        limits: Limits::host(),
    };
    live.set_modal_size(Some(size(320.0)));
    assert!(live.shift_changed(true));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    let before = live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched
        .len();
    assert!(live.press("bundle0.pick", Some(1)));
    live.set_epoch(2).unwrap();
    // Unchanged host state still needs delivery in the new world.
    live.set_modal_size(Some(size(320.0)));
    assert!(live.shift_changed(true));
    assert!(live.press("bundle0.pick", Some(2)));
    for slice in 4..=8 {
        live.poll(2, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    let dispatched = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert!(matches!(
        &dispatched[before..],
        [
            Event::Epoch,
            Event::Resized { .. },
            Event::Shift { held: true },
            Event::Action { index: Some(2), .. }
        ]
    ));
}
