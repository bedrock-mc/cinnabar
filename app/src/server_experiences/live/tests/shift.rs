//! Shift callback transitions, focus changes, and guest recovery.

use super::*;

/// Shift reaches the open bundle only when it changes, a change still waiting being replaced, so
/// a quick tap queues its release alone.
#[test]
fn shift_reaches_the_open_bundle_when_it_changes() {
    let mut live = opened();
    assert!(!live.shift_changed(false));
    assert!(live.shift_changed(true));
    assert!(!live.shift_changed(true));
    assert!(live.shift_changed(false));
    assert_eq!(
        live.instances["bundle0"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::Shift { held: false }]
    );
    live.close_modal();
    assert!(!live.shift_changed(true));
}

/// A recoverable callback failure replaces the guest, which must learn that Shift is still held.
#[test]
fn held_shift_is_delivered_again_after_a_failed_callback() {
    let mut live = opened();
    assert!(live.shift_changed(true));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    assert!(live.press("bundle0.pick", None));
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert!(instance.stopped.is_none());
    assert_eq!(instance.strikes.len(), 1);
    instance.helper.as_mut().unwrap().fail = false;

    assert!(
        live.shift_changed(true),
        "the replacement guest needs held Shift"
    );
    assert!(!live.shift_changed(true));
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched,
        [
            Event::Shift { held: true },
            Event::Action {
                id: "bundle0.pick".into(),
                index: None,
            },
            Event::Shift { held: true },
        ]
    );
}

/// A failed release must be retried so retained UI no longer shows Shift as held.
#[test]
fn failed_shift_release_is_delivered_again_and_clears_retained_ui() {
    let mut live = opened();
    let binding = "#held";
    assert!(live.shift_changed(true));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap();
    helper
        .response
        .as_mut()
        .unwrap()
        .commands
        .push(Command::Value {
            name: binding.into(),
            value: screen::Value::Bool(true),
        });
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();

    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    assert!(live.shift_changed(false));
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert!(instance.stopped.is_none());
    assert_eq!(
        instance.contributions.modal.values[binding],
        screen::Value::Bool(true)
    );
    instance.helper.as_mut().unwrap().fail = false;

    assert!(
        live.shift_changed(false),
        "the retained UI still needs the release"
    );
    assert!(!live.shift_changed(false));
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    let helper = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap();
    assert_eq!(
        helper.dispatched.last(),
        Some(&Event::Shift { held: false })
    );
    helper
        .response
        .as_mut()
        .unwrap()
        .commands
        .push(Command::Value {
            name: binding.into(),
            value: screen::Value::Bool(false),
        });
    live.poll(1, 3 * CALLBACK_INTERVAL_MS).unwrap();
    assert_eq!(
        live.instances["bundle0"].contributions.modal.values[binding],
        screen::Value::Bool(false)
    );
}

/// A Shift transition already queued for the replacement guest needs no duplicate notification.
#[test]
fn queued_shift_survives_a_failed_callback_without_a_duplicate() {
    let mut live = opened();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = true;
    assert!(live.press("bundle0.pick", None));
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.shift_changed(true));
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .fail = false;
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();

    assert!(!live.shift_changed(true));
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched,
        [
            Event::Action {
                id: "bundle0.pick".into(),
                index: None,
            },
            Event::Shift { held: true },
        ]
    );
}

/// Closing while held queues release, so reopening can deliver a fresh press.
#[test]
fn closing_a_held_modal_releases_shift_before_reopening() {
    let mut live = opened();
    assert!(live.shift_changed(true));
    live.close_modal();
    assert_eq!(live.instances["bundle0"].shift, Some(false));
    assert_eq!(
        live.instances["bundle0"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::Shift { held: false }]
    );
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .contributions
        .modal
        .open(Some(SCREEN.into()));
    assert!(
        live.shift_changed(true),
        "reopening while held needs a fresh transition"
    );
}

/// Moving modal focus retires the previous bundle's held modifier state.
#[test]
fn replacing_a_held_modal_releases_the_previous_bundle() {
    let mut live = fixture(2);
    live.ready = true;
    for (order, bundle) in ["bundle0", "bundle1"].into_iter().enumerate() {
        let instance = live.instances.get_mut(bundle).unwrap();
        instance.contributions.modal.open(Some(SCREEN.into()));
        instance.opened = order as u64 + 1;
    }
    assert!(live.shift_changed(true));
    assert_eq!(live.instances["bundle1"].shift, Some(true));
    live.instances.get_mut("bundle0").unwrap().opened = 3;
    assert!(live.shift_changed(true));
    assert_eq!(live.instances["bundle0"].shift, Some(true));
    assert_eq!(live.instances["bundle1"].shift, Some(false));
    assert_eq!(
        live.instances["bundle1"]
            .events
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        [Event::Shift { held: false }]
    );
}

/// A closed bundle retries its pending release after other events free queue capacity.
#[test]
fn a_full_queue_retries_shift_release_after_modal_close() {
    let mut live = opened();
    assert!(live.shift_changed(true));
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.events.clear();
    while live.press("bundle0.pick", None) {}
    live.close_modal();
    assert_eq!(
        live.instances["bundle0"].shift,
        Some(true),
        "the full queue still owes release"
    );
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .events
        .pop_front();
    assert!(live.shift_changed(false));
    assert_eq!(live.instances["bundle0"].shift, Some(false));
    assert!(matches!(
        live.instances["bundle0"].events.back(),
        Some(Event::Shift { held: false })
    ));
}
