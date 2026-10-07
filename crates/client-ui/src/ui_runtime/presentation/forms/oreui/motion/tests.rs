use super::*;

fn sample(motion: &mut Motion, state: Interaction, seconds: f64) -> Feedback {
    motion.feedback(
        Surface::Screen(MenuScreen::Pause),
        Some(MenuAction::PauseResume),
        Kind::Button,
        Feedback::immediate(state, true, false),
        seconds,
    )
}

fn warm() -> Motion {
    let mut motion = Motion::default();
    motion.end_frame(0.0);
    motion
}

#[test]
fn highlight_reverses_continuously_and_settles_without_a_tail() {
    let mut motion = warm();
    let hovered = Interaction {
        hovered: true,
        ..Default::default()
    };
    assert_eq!(sample(&mut motion, hovered, 1.0).hover, 0.0);
    let halfway = sample(&mut motion, hovered, 1.035).hover;
    assert!(halfway > 0.5 && halfway < 1.0);
    assert_eq!(
        sample(&mut motion, Interaction::default(), 1.035).hover,
        halfway
    );
    assert!(sample(&mut motion, Interaction::default(), 1.055).hover < halfway);
    assert_eq!(
        sample(&mut motion, Interaction::default(), 1.125).hover,
        0.0
    );
    motion.end_frame(1.125);
    assert!(motion.controls.is_empty());
}

#[test]
fn server_group_disclosure_mounts_at_saved_state_and_collapses_continuously() {
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    let mut motion = warm();
    let action = Some(MenuAction::ServerList(ServerListAction::Toggle(
        ServerGroup::Featured,
    )));
    let sample = |motion: &mut Motion, expanded: bool, seconds| {
        motion
            .feedback(
                Surface::Play(2),
                action,
                Kind::Disclosure,
                Feedback {
                    selected: u8::from(expanded) as f32,
                    ..Default::default()
                },
                seconds,
            )
            .selected
    };
    assert_eq!(sample(&mut motion, true, 1.0), 1.0);
    assert_eq!(sample(&mut motion, false, 2.0), 1.0);
    let middle = sample(&mut motion, false, 2.035);
    assert!(middle > 0.0 && middle < 1.0);
    assert_eq!(sample(&mut motion, false, 2.2), 0.0);
    motion.configure(false);
    assert_eq!(sample(&mut motion, true, 3.0), 1.0);
    assert_eq!(sample(&mut motion, false, 3.001), 0.0);
}

#[test]
fn presses_are_fast_and_repeated_paint_never_restarts_motion() {
    let mut motion = warm();
    let pressed = Interaction {
        pressed: true,
        ..Default::default()
    };
    sample(&mut motion, pressed, 1.0);
    let midway = sample(&mut motion, pressed, 1.020);
    assert!(midway.press > 0.75 && midway.press < 1.0);
    assert_eq!(sample(&mut motion, pressed, 1.020), midway);
    assert_eq!(sample(&mut motion, pressed, 1.050).press, 1.0);
    assert_eq!(
        sample(&mut motion, Interaction::default(), 1.050).press,
        1.0
    );
    assert_eq!(
        sample(&mut motion, Interaction::default(), 1.140).press,
        0.0
    );
}

#[test]
fn disabling_cancels_motion_immediately_without_replaying_visible_surfaces() {
    let mut motion = warm();
    let surface = Surface::Screen(MenuScreen::Settings);
    let hovered = Interaction {
        hovered: true,
        ..Default::default()
    };
    assert_eq!(motion.entrance(surface, 1.0), 0.0);
    sample(&mut motion, hovered, 1.0);
    motion.configure(false);
    assert_eq!(sample(&mut motion, hovered, 1.001).hover, 1.0);
    assert_eq!(motion.entrance(surface, 1.001), 1.0);
    motion.configure(true);
    assert_eq!(motion.entrance(surface, 1.002), 1.0);
    assert!(motion.controls.is_empty());
}

#[test]
fn entrances_finish_quickly_and_replay_only_after_unmount() {
    let mut motion = warm();
    let surface = Surface::Dialog(7);
    assert_eq!(motion.entrance(surface, 1.0), 0.0);
    assert!(motion.entrance(surface, 1.060) > 0.85);
    assert_eq!(motion.entrance(surface, 1.125), 1.0);
    motion.end_frame(1.125);
    assert_eq!(motion.entrance(surface, 2.0), 1.0);
    motion.end_frame(2.0);
    motion.end_frame(2.1);
    assert_eq!(motion.entrance(surface, 3.0), 0.0);
}

#[test]
fn idle_controls_need_no_retained_entries_and_state_is_retired_on_unmount() {
    let mut motion = warm();
    for index in 0..1000 {
        motion.feedback(
            Surface::Screen(MenuScreen::Servers),
            Some(MenuAction::SelectSaved(index)),
            Kind::Surface,
            Feedback::default(),
            1.0,
        );
    }
    assert_eq!(motion.controls.capacity(), 0);
    sample(
        &mut motion,
        Interaction {
            hovered: true,
            ..Default::default()
        },
        2.0,
    );
    motion.end_frame(2.0);
    motion.end_frame(2.1);
    assert!(motion.controls.is_empty());
}

#[test]
fn setting_value_changes_keep_the_same_interaction_timeline() {
    let mut motion = warm();
    let surface = Surface::Settings(2);
    let hovered = Feedback {
        hover: 1.0,
        ..Default::default()
    };
    motion.feedback(
        surface,
        Some(MenuAction::SettingsOption(7, 0)),
        Kind::Thumb,
        hovered,
        1.0,
    );
    let before = motion.feedback(
        surface,
        Some(MenuAction::SettingsOption(7, 0)),
        Kind::Thumb,
        hovered,
        1.04,
    );
    let after = motion.feedback(
        surface,
        Some(MenuAction::SettingsOption(7, 1)),
        Kind::Thumb,
        hovered,
        1.04,
    );
    assert_eq!(before, after);
    assert_eq!(motion.controls.len(), 1);
}

#[test]
fn setting_choices_keep_selection_and_press_on_their_own_value() {
    let mut motion = Motion::default();
    let surface = Surface::Settings(2);
    for value in 0..3 {
        let target = Feedback {
            selected: u8::from(value == 0) as f32,
            ..Default::default()
        };
        let actual = motion.feedback(
            surface,
            Some(MenuAction::SettingsOption(7, value)),
            Kind::Button,
            target,
            0.0,
        );
        assert_eq!(
            actual, target,
            "choice {value} shares another choice's state"
        );
    }
    motion.end_frame(0.0);
    let pressed = Feedback {
        press: 1.0,
        ..Default::default()
    };
    let action = Some(MenuAction::SettingsOption(7, 2));
    motion.feedback(surface, action, Kind::Button, pressed, 1.0);
    assert_eq!(
        motion
            .feedback(surface, action, Kind::Button, pressed, 1.050)
            .press,
        1.0
    );
    let selected = motion.feedback(
        surface,
        Some(MenuAction::SettingsOption(7, 0)),
        Kind::Button,
        Feedback {
            selected: 1.0,
            ..Default::default()
        },
        1.050,
    );
    assert_eq!(selected.press, 0.0);
    assert_eq!(selected.selected, 1.0);
}

#[test]
fn persisted_screen_animation_setting_controls_all_oreui_timelines() {
    use crate::menu::{MenuView, settings_options::SETTINGS_OPTIONS};
    use crate::ui_runtime::presentation::{UiPresentationRuntime, tests::fixture_font};
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "screen_animations")
        .unwrap();
    std::sync::Arc::make_mut(&mut view.settings_options).set(index, 0);
    runtime.set_menu_view(Some(view));
    runtime.configure_oreui_motion();
    let transitions = &mut runtime.form_presentation.oreui_transitions;
    assert!(!transitions.motion.enabled());
    assert_eq!(transitions.motion.entrance(Surface::Loading, 1.0), 1.0);
    assert_eq!(transitions.slider(4, 0.7, false, 1.0), 0.7);
    assert_eq!(
        transitions.switch(
            MenuAction::SettingsOption(7, 1),
            true,
            Some((MenuAction::SettingsOption(7, 1), 1)),
            1.0
        ),
        2.8
    );
    assert_eq!(transitions.icon_frame(1), None);
    assert!(!transitions.pressed(MenuAction::PauseResume, 1.0));
}

#[test]
fn disclosure_reopening_keeps_a_timeline_after_idle_collapsed_frames() {
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    let mut motion = Motion::default();
    let action = Some(MenuAction::ServerList(ServerListAction::Toggle(
        ServerGroup::Featured,
    )));
    let surface = Surface::Play(2);
    for seconds in [0.0, 0.1, 0.2] {
        assert_eq!(
            motion
                .feedback(
                    surface,
                    action,
                    Kind::Disclosure,
                    Feedback::default(),
                    seconds
                )
                .selected,
            0.0
        );
        motion.end_frame(seconds);
    }
    let open = Feedback {
        selected: 1.0,
        ..Default::default()
    };
    assert_eq!(
        motion
            .feedback(surface, action, Kind::Disclosure, open, 1.0)
            .selected,
        0.0
    );
    motion.end_frame(1.0);
    let middle = motion
        .feedback(surface, action, Kind::Disclosure, open, 1.04)
        .selected;
    assert!(middle > 0.0 && middle < 1.0);
    assert_eq!(
        motion
            .feedback(surface, action, Kind::Disclosure, open, 1.3)
            .selected,
        1.0
    );
    motion.configure(false);
    assert_eq!(
        motion
            .feedback(surface, action, Kind::Disclosure, Feedback::default(), 2.0)
            .selected,
        0.0
    );
}
