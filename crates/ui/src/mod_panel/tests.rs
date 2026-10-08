use super::*;

fn panel() -> Panel {
    Panel {
        theme: Default::default(),
        style: Default::default(),
        title: "Personal controls".into(),
        toggle_key: "ShiftRight".into(),
        dark: true,
        controls: vec![Control::Slider {
            id: "distance".into(),
            label: "Distance".into(),
            value: 3.0,
            min: 1.0,
            max: 6.0,
            step: 0.1,
        }],
        sections: Vec::new(),
        capture_key: false,
    }
}

#[test]
fn finite_slider_and_physical_key_are_accepted() {
    assert!(panel().validate().is_ok());
}

#[test]
fn invalid_slider_never_reaches_presentation() {
    for invalid in [f32::NAN, f32::INFINITY, -1.0, 8.0] {
        let mut panel = panel();
        if let Control::Slider { value, .. } = &mut panel.controls[0] {
            *value = invalid;
        }
        assert!(panel.validate().is_err());
    }
    for invalid in [0.0, -0.1, 10.0, f32::NAN, f32::INFINITY] {
        let mut panel = panel();
        if let Control::Slider { step, .. } = &mut panel.controls[0] {
            *step = invalid;
        }
        assert!(panel.validate().is_err());
    }
}

#[test]
fn ambiguous_identifiers_and_unbounded_text_are_rejected() {
    let mut panel = panel();
    panel.controls.push(panel.controls[0].clone());
    assert!(panel.validate().is_err());
    panel.controls.pop();
    panel.title.push('\n');
    assert!(panel.validate().is_err());
    panel.title = "x".repeat(MAX_PANEL_TEXT_BYTES + 1);
    assert!(panel.validate().is_err());
    panel.title = "Controls".into();
    panel.toggle_key = "Shift Right".into();
    assert!(panel.validate().is_err());
}

#[test]
fn choices_require_a_valid_selection_and_bounded_options() {
    let mut panel = panel();
    panel.controls = vec![Control::Choice {
        id: "mode".into(),
        label: "Mode".into(),
        index: 0,
        options: vec!["Continuous".into(), "While clicking".into()],
    }];
    assert!(panel.validate().is_ok());
    if let Control::Choice { index, .. } = &mut panel.controls[0] {
        *index = 2;
    }
    assert!(panel.validate().is_err());
    if let Control::Choice { index, options, .. } = &mut panel.controls[0] {
        *index = 0;
        options.clear();
    }
    assert!(panel.validate().is_err());
}

#[test]
fn sections_require_complete_unique_control_references() {
    let mut panel = panel();
    panel.sections = vec![Section {
        icon: Default::default(),
        id: "distance_section".into(),
        label: "Distance".into(),
        category: "Tools".into(),
        toggle: None,
        controls: vec!["distance".into()],
    }];
    assert!(panel.validate().is_ok());
    panel.sections[0].controls.push("distance".into());
    assert!(panel.validate().is_err());
    panel.sections[0].controls = vec!["missing".into()];
    assert!(panel.validate().is_err());
    panel.sections[0].controls.clear();
    assert!(panel.validate().is_err());
    panel.sections[0].toggle = Some("distance".into());
    assert!(panel.validate().is_err());
}

#[test]
fn compact_style_and_keybind_labels_are_bounded_and_opt_in() {
    let mut settings = panel();
    assert_eq!(settings.style, Style::Standard);
    settings.style = Style::Compact;
    settings.controls = vec![Control::Keybind {
        id: "binding".into(),
        label: "Keybind".into(),
        key: "KeyR".into(),
        capturing: true,
    }];
    assert!(settings.validate().is_ok());
    if let Control::Keybind { key, .. } = &mut settings.controls[0] {
        *key = "a".repeat(MAX_PANEL_ID_BYTES + 1);
    }
    assert!(settings.validate().is_err());
    if let Control::Keybind { key, .. } = &mut settings.controls[0] {
        *key = "R\n".into();
    }
    assert!(settings.validate().is_err());
}

#[test]
fn expanded_panels_still_reject_controls_above_the_capacity() {
    let mut panel = panel();
    panel.controls = (0..MAX_PANEL_CONTROLS)
        .map(|index| Control::Toggle {
            id: format!("toggle_{index}"),
            label: format!("Toggle {index}"),
            value: false,
        })
        .collect();
    assert!(panel.validate().is_ok());
    panel.controls.push(Control::Button {
        id: "extra".into(),
        label: "Extra".into(),
    });
    assert!(panel.validate().is_err());
}
