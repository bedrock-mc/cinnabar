use super::*;
use crate::menu::{MenuScreen, MenuView, settings_options::SETTINGS_OPTIONS};
use crate::ui_runtime::presentation::{
    TextMetrics, UiPresentationRuntime, forms::menu_screens::SETTINGS_SECTIONS, tests::fixture_font,
};

fn view(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .unwrap();
    view
}

fn paint(presentation: &mut UiPresentationRuntime, view: &MenuView, size: [f32; 2]) {
    let (mut nodes, mut next) = (Vec::new(), 1);
    let metrics = TextMetrics::for_viewport(
        [size[0] as u32, size[1] as u32],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    presentation.menu_hit_targets = presentation
        .append_oreui_screen(view, &mut nodes, &mut next, metrics, size, None, &|_| None)
        .unwrap()
        .unwrap();
}

fn setting(name: &str) -> u16 {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .unwrap() as u16
}

#[test]
fn focus_retains_offscreen_controls_and_explicit_group_delegation() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let view = view("accessibility_forced_index");
    paint(&mut presentation, &view, [1280.0, 420.0]);
    let (targets, landmarks) = presentation.settings_focus_geometry();
    let offscreen = targets
        .iter()
        .find(|target| {
            target.bounds.min().y() >= 420.0
                && !matches!(target.action, MenuAction::SettingsSection(_))
        })
        .expect("keyboard navigation retains clipped content rows");
    assert!(presentation.menu_action_bounds(offscreen.action).is_none());
    let parent = landmarks
        .iter()
        .find(|landmark| Some(landmark.id) == offscreen.landmark)
        .unwrap();
    assert_eq!(parent.scroll_axis, Some(SettingsFocusAxis::Vertical));
    assert!(parent.remember);
    let screen = landmarks
        .iter()
        .find(|landmark| landmark.parent.is_none())
        .unwrap();
    let delegated = landmarks
        .iter()
        .find(|landmark| Some(landmark.id) == screen.delegate_landmark)
        .expect("the screen delegates initial entry into its settings content");
    assert!(delegated.remember);
    let selected = MenuAction::SettingsSection(view.settings_section);
    let sidebar = landmarks
        .iter()
        .find(|landmark| landmark.delegate == Some(selected))
        .unwrap();
    assert_eq!(sidebar.scroll_axis, Some(SettingsFocusAxis::Vertical));
    assert!(
        sidebar.remember,
        "visited sidebar focus wins before its selected alias"
    );
}

#[test]
fn tab_memory_uses_distinct_scopes_for_shared_settings_controls() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let camera = setting("camera_shake");
    let parent_for = |presentation: &UiPresentationRuntime| {
        presentation
            .settings_focus_geometry()
            .0
            .iter()
            .find_map(|target| {
                matches!(target.action, MenuAction::SettingsOption(index, _) if index == camera)
                    .then_some(target.landmark)
                    .flatten()
            })
            .expect("both tabs expose camera shake")
    };
    paint(
        &mut presentation,
        &view("accessibility_forced_index"),
        [1280.0, 720.0],
    );
    let accessibility = parent_for(&presentation);
    paint(
        &mut presentation,
        &view("video_forced_index"),
        [1280.0, 720.0],
    );
    let video = parent_for(&presentation);
    assert_ne!(
        accessibility, video,
        "shared controls preserve each tab's own focus memory"
    );
    assert!(
        !presentation
            .settings_focus_geometry()
            .1
            .iter()
            .any(|group| group.id == accessibility)
    );
    paint(
        &mut presentation,
        &view("accessibility_forced_index"),
        [1280.0, 720.0],
    );
    assert_eq!(parent_for(&presentation), accessibility);
}

#[test]
fn native_picker_isolates_focus_and_slider_capture_and_delegates_selected_value() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = view("video_forced_index");
    let camera = setting("third_person");
    view.settings_dropdown = Some(camera);
    paint(&mut presentation, &view, [600.0, 420.0]);
    let (targets, landmarks) = presentation.settings_focus_geometry();
    let trap = landmarks
        .iter()
        .find(|landmark| landmark.trap)
        .expect("the picker traps focus");
    assert!(trap.remember);
    assert_eq!(
        trap.delegate,
        Some(MenuAction::SettingsOption(
            camera,
            view.settings_options.get(usize::from(camera))
        ))
    );
    assert!(targets.iter().all(|target| matches!(target.action,
        MenuAction::SettingsOption(index, _) | MenuAction::SettingsDropdown(index) if index == camera
    )));
    assert!(landmarks.iter().any(
        |group| group.scroll_axis == Some(SettingsFocusAxis::Vertical)
            && group.focus_control_disabled
    ));
    assert!(
        presentation
            .form_presentation
            .oreui_slider_tracks
            .is_empty(),
        "the modal owns input even where an underlying animated thumb is painted"
    );
    view.settings_dropdown = None;
    paint(&mut presentation, &view, [600.0, 420.0]);
    assert!(
        !presentation
            .settings_focus_geometry()
            .1
            .iter()
            .any(|group| group.trap)
    );
    assert!(
        presentation
            .settings_focus_geometry()
            .0
            .iter()
            .any(|target| matches!(target.action, MenuAction::SettingsSection(_)))
    );
}

#[test]
fn a_new_form_frame_releases_all_native_focus_geometry() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    paint(
        &mut presentation,
        &view("accessibility_forced_index"),
        [1280.0, 720.0],
    );
    assert!(!presentation.settings_focus_geometry().0.is_empty());
    presentation.begin_form_frame();
    assert!(presentation.settings_focus_geometry().0.is_empty());
    assert!(presentation.settings_focus_geometry().1.is_empty());
}

#[test]
fn slider_focus_uses_its_inset_track_anchor_while_pointer_steps_keep_distinct_actions() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let option = setting("field_of_view");
    paint(
        &mut presentation,
        &view("video_forced_index"),
        [1280.0, 720.0],
    );
    let targets: Vec<_> = presentation.settings_focus_geometry().0.iter().filter(|target| {
        matches!(target.action, MenuAction::SettingsOption(index, _) if index == option)
    }).collect();
    assert_eq!(targets.len(), 1);
    let steps: Vec<_> = presentation.menu_hit_targets.iter().filter(|(action, _)| {
        matches!(action, MenuAction::SettingsOption(index, _) if *index == option)
    }).collect();
    assert!(steps.len() > 1);
    let track = presentation
        .form_presentation
        .oreui_slider_tracks
        .iter()
        .find_map(|(index, track, _)| (*index == option).then_some(*track))
        .unwrap();
    assert_eq!(targets[0].bounds, track);
    assert_eq!(targets[0].bounds.height(), steps[0].1.height());
    assert!(targets[0].bounds.width() > steps[0].1.width());
}
