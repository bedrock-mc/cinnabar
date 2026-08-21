use std::sync::Arc;

use ui::{
    BoundedStat, HudStore, HudViewRole, MAX_TOASTS, PROVISIONAL_TOAST_DURATION_MILLIS,
    TitleDurations, Toast,
};

#[test]
fn title_durations_expire_from_monotonic_arrival_time() {
    let mut hud = HudStore::default();
    let durations = TitleDurations::from_wire(1, 2, 1).unwrap();
    hud.set_durations(durations);
    hud.set_title(Arc::from("Round one"), 7, 1_000);

    assert_eq!(hud.view_nodes(1_199)[0].role, HudViewRole::Title);
    assert!(hud.view_nodes(1_200).is_empty());
    hud.expire(1_200);
    assert!(hud.title().is_none());
}

#[test]
fn title_reset_clears_text_and_restores_vanilla_durations() {
    let mut hud = HudStore::default();
    hud.set_durations(TitleDurations::from_wire(1, 1, 1).unwrap());
    hud.set_title(Arc::from("title"), 1, 0);
    hud.set_subtitle(Arc::from("subtitle"), 2, 0);
    hud.set_actionbar(Arc::from("action"), 3, 0);

    hud.reset_titles();

    assert!(hud.title().is_none());
    assert!(hud.subtitle().is_none());
    assert!(hud.actionbar().is_none());
    assert_eq!(hud.durations(), TitleDurations::default());
}

#[test]
fn toast_queue_is_bounded_and_view_nodes_preserve_fifo_order() {
    let mut hud = HudStore::default();
    for sequence in 1..=(MAX_TOASTS as u64 + 1) {
        hud.push_toast(Toast {
            title: Arc::from(format!("title {sequence}")),
            message: Arc::from(format!("message {sequence}")),
            fifo_sequence: sequence,
            received_millis: sequence,
            expires_millis: u64::MAX,
        });
    }

    assert_eq!(hud.toasts().len(), MAX_TOASTS);
    assert_eq!(hud.toasts().front().unwrap().fifo_sequence, 2);
    let nodes = hud.view_nodes(0);
    assert_eq!(nodes.len(), MAX_TOASTS * 2);
    assert_eq!(nodes[0].source_sequence, 2);
    assert_eq!(nodes[0].role, HudViewRole::ToastTitle);
    assert_eq!(nodes[1].role, HudViewRole::ToastMessage);
}

#[test]
fn toasts_expire_after_the_provisional_duration() {
    let mut hud = HudStore::default();
    hud.push_toast(Toast::new(Arc::from("hello"), Arc::from("world"), 9, 1_000));

    assert_eq!(
        hud.toasts().front().unwrap().expires_millis,
        1_000 + PROVISIONAL_TOAST_DURATION_MILLIS
    );
    assert!(!hud.view_nodes(5_999).is_empty());

    // Exactly at expiry the toast stops rendering, and expire() removes it
    // together with its retained-byte accounting.
    assert!(
        hud.view_nodes(1_000 + PROVISIONAL_TOAST_DURATION_MILLIS)
            .is_empty()
    );
    hud.expire(1_000 + PROVISIONAL_TOAST_DURATION_MILLIS);
    assert!(hud.toasts().is_empty());
}

#[test]
fn toast_expiry_prunes_only_expired_fronts_in_order() {
    let mut hud = HudStore::default();
    let mut first = Toast::new(Arc::from("one"), Arc::from("m"), 1, 0);
    first.expires_millis = 100;
    let mut second = Toast::new(Arc::from("two"), Arc::from("m"), 2, 10);
    second.expires_millis = 200;
    let third = Toast::new(Arc::from("three"), Arc::from("m"), 3, 20);
    hud.push_toast(first);
    hud.push_toast(second);
    hud.push_toast(third);

    hud.expire(150);

    assert_eq!(hud.toasts().len(), 2);
    assert_eq!(hud.toasts().front().unwrap().fifo_sequence, 2);
    let nodes = hud.view_nodes(150);
    assert_eq!(nodes.len(), 4);
    assert_eq!(nodes[0].source_sequence, 2);

    // The provisional duration keeps the last toast alive well past 250 ms.
    hud.expire(20 + PROVISIONAL_TOAST_DURATION_MILLIS);
    assert!(hud.toasts().is_empty());
}

#[test]
fn expired_toasts_release_their_retained_byte_budget() {
    let mut hud = HudStore::default();
    let large_title = Arc::from("t".repeat(100_000).into_boxed_str());
    let large_message = Arc::from("m".repeat(50_000).into_boxed_str());

    let mut first = Toast::new(Arc::clone(&large_title), Arc::clone(&large_message), 1, 0);
    first.expires_millis = 100;
    let mut second = Toast::new(Arc::clone(&large_title), Arc::clone(&large_message), 2, 0);
    second.expires_millis = 200;
    hud.push_toast(first);
    // The second toast exceeds the shared byte budget, so the first is
    // evicted from the front.
    let forced_evictions = hud.push_toast(second);
    assert_eq!(forced_evictions, 1);

    hud.expire(300);
    let third = Toast::new(Arc::clone(&large_title), Arc::clone(&large_message), 3, 0);
    assert_eq!(
        hud.push_toast(third),
        0,
        "expired toasts must release their retained bytes"
    );
}

#[test]
fn bounded_stats_reject_invalid_ranges_and_clear_atomically() {
    assert!(BoundedStat::new(21, 20).is_none());
    assert!(BoundedStat::new(0, 0).is_none());
    let health = BoundedStat::new(19, 20).unwrap();
    let mut hud = HudStore::default();
    hud.set_stats(Some(health), None, None, None);
    assert_eq!(hud.health(), Some(health));
    let nodes = hud.view_nodes(0);
    assert_eq!(nodes[0].role, HudViewRole::Health);
    assert_eq!(nodes[0].text.as_ref(), "19/20");

    hud.clear();
    assert_eq!(hud.health(), None);
    assert!(hud.toasts().is_empty());
}

#[test]
fn scaled_stats_render_native_units_without_exposing_storage_scale() {
    let mut hud = HudStore::default();
    hud.set_stats(BoundedStat::new_scaled(1_750, 2_000, 100), None, None, None);

    let nodes = hud.view_nodes(0);
    assert_eq!(nodes[0].role, HudViewRole::Health);
    assert_eq!(nodes[0].text.as_ref(), "17.5/20");
}
