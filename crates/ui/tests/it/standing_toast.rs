//! A standing toast holds the toast slot while its cause lasts, gives way to queued toasts and
//! comes back after them.

use std::sync::Arc;

use ui::{
    HudStore, StandingToast, TOAST_DISPLAY_MILLIS, TOAST_SLIDE_IN_MILLIS, TOAST_SLIDE_OUT_MILLIS,
    Toast, ToastPress,
};

const UNTIL: u64 = 30_000;

fn standing(id: u64, since: u64, until: u64) -> StandingToast {
    StandingToast {
        id,
        title: Arc::from("Alex wants to join"),
        message: Arc::from("Press N to respond"),
        press: ToastPress::JoinRequests,
        since_millis: since,
        until_millis: until,
    }
}

fn shown(hud: &HudStore, now: u64) -> Option<(String, Option<ToastPress>, f32)> {
    hud.showing_toast(now)
        .map(|toast| (toast.title.to_owned(), toast.press, toast.slide))
}

#[test]
fn a_standing_toast_stays_until_its_cause_ends_then_slides_out() {
    let mut hud = HudStore::default();
    hud.stand_toast(standing(1, 0, UNTIL));
    assert_eq!(shown(&hud, 0).map(|toast| toast.2), Some(0.0));
    let held = shown(&hud, UNTIL - 1).unwrap();
    assert_eq!((held.1, held.2), (Some(ToastPress::JoinRequests), 1.0));
    const {
        assert!(
            UNTIL - 1 > TOAST_DISPLAY_MILLIS * 3,
            "outlives an ordinary toast"
        )
    };
    assert_eq!(
        shown(&hud, UNTIL + TOAST_SLIDE_OUT_MILLIS / 2).map(|toast| toast.2),
        Some(0.5)
    );
    hud.expire(UNTIL + TOAST_SLIDE_OUT_MILLIS);
    assert!(hud.showing_toast(UNTIL + TOAST_SLIDE_OUT_MILLIS).is_none());
    assert!(hud.standing_toast().is_none());
}

#[test]
fn retiring_a_standing_toast_slides_it_out_at_once() {
    let mut hud = HudStore::default();
    hud.stand_toast(standing(1, 0, UNTIL));
    hud.retire_standing_toast(5_000);
    assert_eq!(
        shown(&hud, 5_000 + TOAST_SLIDE_OUT_MILLIS / 2).map(|toast| toast.2),
        Some(0.5)
    );
    assert!(hud.showing_toast(5_000 + TOAST_SLIDE_OUT_MILLIS).is_none());
}

#[test]
fn a_server_toast_takes_the_slot_on_time_and_the_standing_toast_returns() {
    let mut hud = HudStore::default();
    hud.stand_toast(standing(1, 0, UNTIL));
    hud.push_toast(Toast::new(Arc::from("Welcome"), Arc::from(""), 1, 5_000));
    // The standing toast slides out first, then the server toast holds its full duration.
    assert_eq!(
        shown(&hud, 5_000 + TOAST_SLIDE_OUT_MILLIS / 2).map(|toast| (toast.0, toast.2)),
        Some(("Alex wants to join".to_owned(), 0.5))
    );
    let server = hud.toasts()[0].clone();
    assert_eq!(server.started_millis, 5_000 + TOAST_SLIDE_OUT_MILLIS);
    let middle = shown(&hud, server.started_millis + TOAST_SLIDE_IN_MILLIS).unwrap();
    assert_eq!((middle.0.as_str(), middle.1), ("Welcome", None));
    hud.expire(server.expires_millis);
    let back = shown(&hud, server.expires_millis).unwrap();
    assert_eq!((back.0.as_str(), back.2), ("Alex wants to join", 0.0));
    assert_eq!(
        shown(&hud, server.expires_millis + TOAST_SLIDE_IN_MILLIS).map(|toast| toast.2),
        Some(1.0)
    );
}

#[test]
fn standing_again_for_the_same_cause_keeps_it_on_screen() {
    let mut hud = HudStore::default();
    hud.stand_toast(standing(1, 0, UNTIL));
    hud.stand_toast(standing(1, 10_000, 10_000 + UNTIL));
    assert_eq!(shown(&hud, 10_000).map(|toast| toast.2), Some(1.0));
    assert!(hud.showing_toast(UNTIL + TOAST_SLIDE_OUT_MILLIS).is_some());
    // Another cause slides in fresh.
    hud.stand_toast(standing(2, 12_000, 12_000 + UNTIL));
    assert_eq!(shown(&hud, 12_000).map(|toast| toast.2), Some(0.0));
}
