use {super::*, launcher::menu::MenuAction};

#[test]
fn server_glimmer_changes_only_with_selection_and_obeys_the_motion_setting() {
    let mut transitions = Transitions::default();
    transitions.begin_frame(None, false, 0.0);
    transitions.begin_servers(Some(4));
    assert_eq!(transitions.server_icon_frame(4), Some(0));
    transitions.end_frame();
    transitions.begin_frame(None, false, 1.0);
    transitions.begin_servers(Some(4));
    assert_eq!(
        transitions.server_icon_frame(4),
        Some(ICON_HIGHLIGHT_FRAMES - 1)
    );
    transitions.begin_servers(Some(5));
    assert_eq!(transitions.server_icon_frame(5), Some(0));
    assert_eq!(transitions.server_icon_frame(4), None);
    transitions.configure_motion(false);
    assert_eq!(transitions.server_icon_frame(5), None);
}

fn setting(value: i32) -> MenuAction {
    MenuAction::SettingsOption(7, value)
}

fn switch_started(on: bool) -> Transitions {
    let mut transitions = Transitions::default();
    transitions.switch(setting(i32::from(!on)), !on, None, -1.0);
    transitions.end_frame();
    transitions
}

#[test]
fn switch_travel_is_continuous_monotonic_and_finishes_quickly() {
    for on in [false, true] {
        let mut transitions = switch_started(on);
        let activation = Some((setting(i32::from(on)), 1));
        let from = switch_target(!on);
        let to = switch_target(on);
        let start = transitions.switch(setting(i32::from(on)), on, activation, 1.0);
        assert_eq!(start, from, "a toggle must start at its current position");
        let middle = transitions.switch(setting(i32::from(on)), on, activation, 1.03);
        assert!(middle > from.min(to) && middle < from.max(to));
        assert_eq!(
            transitions.switch(setting(i32::from(on)), on, activation, 1.12),
            to
        );
    }
}

#[test]
fn settings_icon_glimmer_finishes_in_under_a_quarter_second() {
    let mut transitions = Transitions::default();
    transitions.begin_frame(None, false, 1.0);
    transitions.begin_settings(2);
    assert_eq!(transitions.icon_frame(2), Some(0));
    transitions.begin_frame(None, false, 1.24);
    transitions.begin_settings(2);
    assert_eq!(transitions.icon_frame(2), Some(ICON_HIGHLIGHT_FRAMES - 1));
}

#[test]
fn world_category_glimmer_preserves_selection_and_respects_disabled_motion() {
    let mut transitions = Transitions::default();
    transitions.begin_frame(None, false, 0.0);
    transitions.begin_world(0);
    assert_eq!(transitions.world_icon_frame(0), Some(0));
    transitions.end_frame();
    transitions.begin_frame(None, false, 1.0);
    transitions.begin_world(0);
    assert_eq!(
        transitions.world_icon_frame(0),
        Some(ICON_HIGHLIGHT_FRAMES - 1)
    );
    transitions.begin_world(1);
    assert_eq!(transitions.world_icon_frame(1), Some(0));
    assert_eq!(transitions.world_icon_frame(0), None);
    transitions.configure_motion(false);
    assert_eq!(transitions.world_icon_frame(1), None);
}

#[test]
fn switch_click_never_overshoots_and_settles_without_a_tail() {
    for on in [false, true] {
        let mut transitions = switch_started(on);
        let activation = Some((setting(i32::from(on)), 1));
        let mut previous = switch_target(!on);
        for time in [0.0, 0.025, 0.050, 0.075, 0.100, 0.125] {
            let position = transitions.switch(setting(i32::from(on)), on, activation, time);
            assert!((0.0..=SWITCH_TRAVEL).contains(&position));
            assert!(if on {
                position >= previous
            } else {
                position <= previous
            });
            previous = position;
        }
        assert_eq!(previous, switch_target(on));
        assert_eq!(
            transitions.switch(setting(i32::from(on)), on, activation, 10.0),
            previous
        );
    }
}

#[test]
fn switch_initial_mount_and_external_changes_stay_static() {
    let mut transitions = Transitions::default();
    let activation = Some((setting(1), 91));
    assert_eq!(
        transitions.switch(setting(1), true, activation, 0.0),
        SWITCH_TRAVEL
    );
    transitions.end_frame();
    assert_eq!(transitions.switch(setting(0), false, activation, 0.1), 0.0);
    transitions.end_frame();
    assert_eq!(
        transitions.switch(setting(1), true, None, 0.2),
        SWITCH_TRAVEL
    );
}

#[test]
fn switch_reversal_starts_at_its_current_position() {
    let mut transitions = switch_started(true);
    let activation = Some((setting(1), 1));
    transitions.switch(setting(1), true, activation, 0.0);
    let prior = transitions.switch(setting(1), true, activation, 0.03);
    assert!(prior > 0.0 && prior < SWITCH_TRAVEL);
    assert_eq!(
        transitions.switch(setting(0), false, Some((setting(0), 2)), 0.03),
        prior
    );
    let later = transitions.switch(setting(0), false, Some((setting(0), 2)), 0.055);
    assert!(later > 0.0 && later < prior);
    assert_eq!(
        transitions.switch(setting(0), false, Some((setting(0), 2)), 0.14),
        0.0
    );
}

#[test]
fn repeated_paint_and_activation_at_the_same_clock_do_not_restart_switch() {
    let mut transitions = switch_started(true);
    transitions.switch(setting(1), true, Some((setting(1), 1)), 0.0);
    let first = transitions.switch(setting(1), true, Some((setting(1), 1)), 0.03);
    assert_eq!(
        transitions.switch(setting(1), true, Some((setting(1), 3)), 0.03),
        first
    );
    assert!(transitions.switch(setting(1), true, Some((setting(1), 3)), 0.06) > first);
    assert_eq!(
        transitions.switch(setting(1), true, Some((setting(1), 3)), 0.12),
        SWITCH_TRAVEL
    );
}

#[test]
fn coalesced_clicks_with_an_unchanged_final_value_do_not_wobble() {
    let mut transitions = switch_started(true);
    let last_click = Some((setting(0), 2));
    for time in [0.0, 0.03, 0.12] {
        assert_eq!(transitions.switch(setting(1), false, last_click, time), 0.0);
    }
}

#[test]
fn mounted_tabs_preserve_values_and_cancel_hidden_control_motion() {
    let mut transitions = Transitions::default();
    transitions.begin_frame(None, false, 0.0);
    transitions.begin_settings(1);
    transitions.switch(setting(1), false, None, 0.0);
    transitions.slider(7, 0.0, false, 0.0);
    transitions.end_frame();
    let activation = Some((setting(1), 1));
    transitions.begin_frame(activation, false, 1.0);
    transitions.begin_settings(1);
    transitions.switch(setting(0), true, activation, 1.0);
    transitions.slider(7, 1.0, false, 1.0);
    transitions.end_frame();
    transitions.begin_frame(activation, false, 1.1);
    transitions.begin_settings(2);
    assert_eq!(transitions.switch(setting(0), true, activation, 1.1), 2.8);
    assert_eq!(transitions.slider(7, 1.0, false, 1.1), 1.0);
    transitions.end_frame();
    assert!(transitions.switches.contains_key(&(1, Control::Setting(7))));
    assert!(transitions.sliders.contains_key(&(1, 7)));
    transitions.begin_frame(activation, false, 2.0);
    transitions.begin_settings(1);
    assert_eq!(
        transitions.switch(setting(0), true, activation, 2.0),
        SWITCH_TRAVEL
    );
    assert_eq!(transitions.slider(7, 1.0, false, 2.0), 1.0);
    transitions.end_frame();
    transitions.begin_frame(activation, false, 3.0);
    transitions.end_frame();
    assert!(transitions.switches.is_empty());
    assert!(transitions.sliders.is_empty());
}

#[test]
fn controls_removed_from_the_current_mounted_tab_are_retired() {
    let mut transitions = Transitions::default();
    transitions.begin_settings(1);
    transitions.switch(setting(1), true, None, 0.0);
    transitions.slider(7, 1.0, false, 0.0);
    transitions.end_frame();
    transitions.begin_settings(1);
    transitions.end_frame();
    assert!(transitions.switches.is_empty());
    assert!(transitions.sliders.is_empty());
}

#[test]
fn slider_native_curve_keeps_overshoot_and_settles_exactly() {
    let mut transitions = Transitions::default();
    assert_eq!(transitions.slider(7, 0.0, false, 0.0), 0.0);
    assert_eq!(transitions.slider(7, 1.0, false, 1.0), 0.0);
    assert!(transitions.slider(7, 1.0, false, 1.225) > 1.0);
    assert_eq!(transitions.slider(7, 1.0, false, 1.301), 1.0);
    assert_eq!(transitions.slider(7, 1.0, false, 1000.0), 1.0);
}

#[test]
fn slider_mouse_drag_is_immediate_and_release_moves_to_rounded_anchor() {
    let mut transitions = Transitions::default();
    transitions.slider(7, 0.0, false, 0.0);
    assert_eq!(transitions.slider(7, 0.37, true, 0.1), 0.37);
    assert_eq!(transitions.slider(7, 0.5, false, 0.2), 0.37);
    assert!(transitions.slider(7, 0.5, false, 0.3) > 0.37);
    assert_eq!(transitions.slider(7, 0.5, false, 0.501), 0.5);
}

#[test]
fn slider_touch_retargets_from_its_current_visual_position() {
    let mut transitions = Transitions::default();
    transitions.slider(7, 0.0, false, 0.0);
    transitions.slider(7, 0.5, false, 1.0);
    let prior = transitions.slider(7, 0.5, false, 1.1);
    assert!(prior > 0.0 && prior < 0.5);
    assert_eq!(transitions.slider(7, 0.8, false, 1.1), prior);
    assert!(transitions.slider(7, 0.8, false, 1.2) > prior);
    assert_eq!(transitions.slider(7, 0.8, false, 1.401), 0.8);
}

#[test]
fn slider_transition_duration_change_without_a_value_change_preserves_running_transition() {
    let mut transitions = Transitions::default();
    transitions.slider(7, 0.0, false, 0.0);
    transitions.slider(7, 1.0, false, 1.0);
    let prior = transitions.slider(7, 1.0, false, 1.1);
    assert_eq!(transitions.slider(7, 1.0, true, 1.1), prior);
    assert_eq!(transitions.slider(7, 0.7, true, 1.1), 0.7);
}

#[test]
fn slider_reversing_transition_uses_the_css_shortened_duration() {
    let mut transitions = Transitions::default();
    transitions.slider(7, 0.0, false, 0.0);
    transitions.slider(7, 1.0, false, 1.0);
    let current = transitions.slider(7, 1.0, false, 1.06);
    assert_eq!(transitions.slider(7, 0.0, false, 1.06), current);
    let duration = transitions.sliders[&(0, 7)].duration;
    assert!((duration - SLIDER_DURATION * f64::from(current)).abs() < 0.000001);
    assert_eq!(
        transitions.slider(7, 0.0, false, 1.06 + duration + 0.0001),
        0.0
    );
}

#[test]
fn unmounted_controls_retire_and_remount_without_replaying_old_activation() {
    let mut transitions = switch_started(true);
    let activation = Some((setting(1), 1));
    transitions.switch(setting(1), true, activation, 0.0);
    transitions.slider(7, 0.0, false, 0.0);
    transitions.slider(7, 1.0, false, 0.1);
    transitions.end_frame();
    transitions.end_frame();
    assert!(transitions.switches.is_empty());
    assert!(transitions.sliders.is_empty());
    assert_eq!(transitions.switch(setting(1), true, activation, 0.15), 2.8);
    assert_eq!(transitions.slider(7, 1.0, false, 0.15), 1.0);
}

#[test]
fn navigation_press_lasts_150ms_and_pointer_click_does_not_create_it() {
    let mut transitions = Transitions::default();
    transitions.begin_frame(None, false, 0.0);
    transitions.end_frame();
    let activation = Some((setting(1), 1));
    transitions.begin_frame(activation, true, 1.0);
    assert!(transitions.pressed(setting(1), 1.0));
    transitions.end_frame();
    transitions.begin_frame(activation, true, 1.1499);
    assert!(transitions.pressed(setting(1), 1.1499));
    transitions.end_frame();
    transitions.begin_frame(activation, true, 1.1501);
    assert!(!transitions.pressed(setting(1), 1.1501));
    transitions.end_frame();
    assert!(transitions.presses.is_empty());
    transitions.begin_frame(Some((setting(0), 2)), false, 1.2);
    assert!(!transitions.pressed(setting(0), 1.2));
}

#[test]
fn navigation_press_consumes_existing_activation_on_first_mount() {
    let mut transitions = Transitions::default();
    let activation = Some((setting(1), 1));
    transitions.begin_frame(activation, true, 0.0);
    assert!(!transitions.pressed(setting(1), 0.0));
    transitions.end_frame();
    transitions.begin_frame(Some((setting(1), 2)), true, 1.0);
    assert!(transitions.pressed(setting(1), 1.0));
    transitions.end_frame();
    transitions.end_frame();
    transitions.begin_frame(Some((setting(1), 2)), true, 1.1);
    assert!(!transitions.pressed(setting(1), 1.1));
}
#[test]
fn play_icon_glimmers_restart_on_selection_and_obey_motion_configuration() {
    let mut transitions = super::Transitions::default();
    transitions.begin_frame(None, false, 0.0);
    transitions.begin_play(0);
    assert_eq!(transitions.play_icon_frame(0), Some(0));
    assert_eq!(transitions.play_icon_frame(1), None);
    transitions.end_frame();
    transitions.begin_frame(None, false, 0.1);
    transitions.begin_play(0);
    assert_eq!(transitions.play_icon_frame(0), Some(4));
    transitions.end_frame();
    transitions.begin_frame(None, false, 1.0);
    transitions.begin_play(2);
    assert_eq!(transitions.play_icon_frame(2), Some(0));
    transitions.configure_motion(false);
    assert_eq!(transitions.play_icon_frame(2), None);
    transitions.end_frame();
    transitions.end_frame();
    assert_eq!(transitions.play_icon_frame(2), None);
}
