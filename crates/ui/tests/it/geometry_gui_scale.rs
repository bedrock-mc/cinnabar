use ui::{DesktopGuiScale, gui_scale};

#[test]
fn desktop_modifier_choices_match_lens_viewport_thresholds() {
    for (physical, optimal, offsets) in [
        ([375, 249], 1, vec![0]),
        ([752, 500], 2, vec![-1, 0]),
        ([1128, 750], 3, vec![-1, 0]),
        ([1504, 1000], 4, vec![-2, -1, 0]),
        ([3840, 2160], 8, vec![-4, -3, -2, -1, 0]),
    ] {
        let desktop = DesktopGuiScale::for_window(physical);
        assert_eq!(gui_scale(physical, None), optimal);
        assert_eq!(desktop.offsets().collect::<Vec<_>>(), offsets);
        assert_eq!(desktop.scale_for_offset(0), optimal as u8);
        assert_eq!(desktop.scale_for_offset(i8::MAX), optimal as u8);
        assert_eq!(desktop.scale_for_offset(i8::MIN), optimal.div_ceil(2) as u8);
    }
}

#[test]
fn desktop_modifier_is_relative_to_the_current_optimal_scale() {
    assert_eq!(
        DesktopGuiScale::for_window([1920, 1080]).scale_for_offset(-1),
        3
    );
    assert_eq!(
        DesktopGuiScale::for_window([1280, 720]).scale_for_offset(-1),
        1
    );
    assert_eq!(
        DesktopGuiScale::for_window([1920, 1080]).scale_for_offset(-2),
        2
    );
    assert_eq!(
        DesktopGuiScale::for_window([1280, 720]).scale_for_offset(-2),
        1
    );
}

#[test]
fn desktop_option_percentages_use_the_optimal_scale_even_when_offsets_match() {
    let half = DesktopGuiScale::for_window([752, 500])
        .choices()
        .collect::<Vec<_>>();
    let thirds = DesktopGuiScale::for_window([1128, 750])
        .choices()
        .collect::<Vec<_>>();
    assert_eq!(
        half.iter().map(|choice| choice.offset).collect::<Vec<_>>(),
        thirds
            .iter()
            .map(|choice| choice.offset)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        half.iter()
            .map(|choice| choice.percentage)
            .collect::<Vec<_>>(),
        vec![50, 100]
    );
    assert_eq!(
        thirds
            .iter()
            .map(|choice| choice.percentage)
            .collect::<Vec<_>>(),
        vec![67, 100]
    );
}
