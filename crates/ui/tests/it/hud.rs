use std::sync::Arc;

use ui::{
    BoundedStat, HudStore, HudViewRole, MAX_TOASTS, TOAST_DISPLAY_MILLIS, TOAST_SLIDE_IN_MILLIS,
    TOAST_SLIDE_OUT_MILLIS, TitleDurations, Toast,
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
    hud.set_tip(Arc::from("tip"), 4, 0);

    hud.reset_titles();

    assert!(hud.title().is_none());
    assert!(hud.subtitle().is_none());
    assert!(hud.actionbar().is_none());
    assert!(hud.tip().is_none());
    assert_eq!(hud.durations(), TitleDurations::default());
}

// The tip text has its own slot: it does not overwrite the action bar.
#[test]
fn a_tip_text_does_not_replace_the_action_bar() {
    let mut hud = HudStore::default();
    hud.set_actionbar(Arc::from("bar"), 1, 0);
    hud.set_tip(Arc::from("tip"), 2, 0);

    assert_eq!(hud.actionbar().unwrap().text.as_ref(), "bar");
    assert_eq!(hud.tip().unwrap().text.as_ref(), "tip");
    let roles: Vec<HudViewRole> = hud.view_nodes(0).iter().map(|n| n.role).collect();
    assert_eq!(roles, [HudViewRole::ActionBar, HudViewRole::Tip]);
    // Expiry removes only what expired.
    hud.expire(4_999);
    assert_eq!(hud.actionbar().unwrap().text.as_ref(), "bar");
    assert_eq!(hud.tip().unwrap().text.as_ref(), "tip");
    hud.expire(5_000);
    assert!(hud.actionbar().is_none());
    assert!(hud.tip().is_none());

    // A longer-lived tip outlives the action bar beside it.
    let mut hud = HudStore::default();
    hud.set_durations(TitleDurations::from_wire(0, 10, 0).unwrap());
    hud.set_actionbar(Arc::from("bar"), 1, 0);
    hud.set_tip(Arc::from("tip"), 2, 1_000);
    hud.expire(900);
    assert!(hud.actionbar().is_none());
    assert_eq!(hud.tip().unwrap().text.as_ref(), "tip");
    hud.expire(1_500);
    assert!(hud.tip().is_none());
}

#[test]
fn toast_queue_is_bounded_and_view_nodes_preserve_fifo_order() {
    let mut hud = HudStore::default();
    for sequence in 1..=(MAX_TOASTS as u64 + 1) {
        hud.push_toast(Toast::new(
            Arc::from(format!("title {sequence}")),
            Arc::from(format!("message {sequence}")),
            sequence,
            sequence,
        ));
    }

    assert_eq!(hud.toasts().len(), MAX_TOASTS);
    assert_eq!(hud.toasts().front().unwrap().fifo_sequence, 2);
    // One toast shows at a time; the rest wait their turn.
    let nodes = hud.view_nodes(hud.toasts().front().unwrap().started_millis);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].source_sequence, 2);
    assert_eq!(nodes[0].role, HudViewRole::ToastTitle);
    assert_eq!(nodes[1].role, HudViewRole::ToastMessage);
}

const TOAST_MILLIS: u64 = TOAST_DISPLAY_MILLIS + TOAST_SLIDE_OUT_MILLIS;

#[test]
fn a_toast_slides_in_stays_and_slides_out_on_the_vanilla_timings() {
    let mut hud = HudStore::default();
    hud.push_toast(Toast::new(Arc::from("hello"), Arc::from("world"), 9, 1_000));
    let toast = hud.toasts().front().unwrap().clone();
    assert_eq!(toast.expires_millis, 1_000 + TOAST_MILLIS);
    assert_eq!(toast.slide(1_000), 0.0);
    assert_eq!(toast.slide(1_000 + TOAST_SLIDE_IN_MILLIS / 2), 0.5);
    assert_eq!(toast.slide(1_000 + TOAST_SLIDE_IN_MILLIS), 1.0);
    assert_eq!(toast.slide(1_000 + TOAST_DISPLAY_MILLIS), 1.0);
    assert_eq!(
        toast.slide(1_000 + TOAST_DISPLAY_MILLIS + TOAST_SLIDE_OUT_MILLIS / 2),
        0.5
    );
    assert!(!hud.view_nodes(1_000 + TOAST_MILLIS - 1).is_empty());
    // Exactly at expiry it stops rendering, and expire() removes it with its bytes.
    assert!(hud.view_nodes(1_000 + TOAST_MILLIS).is_empty());
    hud.expire(1_000 + TOAST_MILLIS);
    assert!(hud.toasts().is_empty());
}

#[test]
fn queued_toasts_show_one_after_another_and_expire_in_order() {
    let mut hud = HudStore::default();
    for (sequence, received) in [(1, 0), (2, 10), (3, 20)] {
        hud.push_toast(Toast::new(
            Arc::from("t"),
            Arc::from("m"),
            sequence,
            received,
        ));
    }
    let starts: Vec<u64> = hud
        .toasts()
        .iter()
        .map(|toast| toast.started_millis)
        .collect();
    assert_eq!(starts, [0, TOAST_MILLIS, 2 * TOAST_MILLIS]);
    let nodes = hud.view_nodes(TOAST_MILLIS + 5);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].source_sequence, 2);
    hud.expire(TOAST_MILLIS + 5);
    assert_eq!(hud.toasts().front().unwrap().fifo_sequence, 2);
    hud.expire(3 * TOAST_MILLIS);
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

#[test]
fn title_alpha_ramps_in_holds_and_ramps_out() {
    let mut hud = HudStore::default();
    hud.set_durations(TitleDurations::from_wire(10, 20, 10).unwrap());
    hud.set_title(Arc::from("t"), 1, 1_000);
    let title = hud.title().unwrap();

    assert_eq!(title.alpha_at(1_000), 0);
    assert_eq!(title.alpha_at(1_250), 127);
    assert_eq!(title.alpha_at(1_500), 255);
    assert_eq!(title.alpha_at(2_500), 255);
    assert_eq!(title.alpha_at(2_750), 127);
    assert_eq!(title.alpha_at(3_000), 0);
}

#[test]
fn zero_length_fades_are_fully_opaque_for_the_stay() {
    let mut hud = HudStore::default();
    hud.set_durations(TitleDurations::from_wire(0, 4, 0).unwrap());
    hud.set_actionbar(Arc::from("a"), 1, 0);
    let bar = hud.actionbar().unwrap();

    assert_eq!(bar.alpha_at(0), 255);
    assert_eq!(bar.alpha_at(199), 255);
    assert_eq!(bar.alpha_at(200), 0);
}

#[test]
fn review_non_decimal_scales_format_the_actual_rational_value() {
    for (current, scale, expected) in [(3, 2, "1.5/2"), (7, 4, "1.75/2"), (3, 5, "0.6/2")] {
        let mut hud = HudStore::default();
        hud.set_stats(
            BoundedStat::new_scaled(current, scale * 2, scale),
            None,
            None,
            None,
        );
        assert_eq!(hud.view_nodes(0)[0].text.as_ref(), expected);
    }
}

#[test]
fn review_toast_expiry_keeps_notifications_that_have_not_started() {
    let mut hud = HudStore::default();
    for sequence in 0..=MAX_TOASTS as u64 {
        hud.push_toast(Toast::new(
            Arc::from("Title"),
            Arc::from("Message"),
            sequence,
            0,
        ));
    }
    assert!(hud.toasts().front().unwrap().started_millis > 0);
    hud.expire(0);
    assert_eq!(hud.toasts().len(), MAX_TOASTS);
    assert!(hud.view_nodes(0).is_empty());
    let first = hud.toasts().front().unwrap().started_millis;
    assert!(!hud.view_nodes(first).is_empty());
}

#[test]
fn absorption_points_round_up_and_zero_remains_authoritative() {
    let mut hud = HudStore::default();
    for (points, expected) in [(0.001, 1), (3.25, 4), (0.0, 0)] {
        hud.set_absorption(BoundedStat::from_absorption_points(points));
        assert_eq!(hud.absorption().unwrap().current(), expected);
    }
    for points in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(BoundedStat::from_absorption_points(points).is_none());
    }
}
