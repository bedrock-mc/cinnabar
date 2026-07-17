use std::sync::Arc;

use ui::{BoundedStat, HudStore, HudViewRole, MAX_TOASTS, TitleDurations, Toast};

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
