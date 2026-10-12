use super::*;
use json_ui::{LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource, ViewState};
use server_experience::screen::Rect;

struct EmptyArt;
impl TextMeasure for EmptyArt {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for EmptyArt {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Lays out an original button with primary and secondary actions.
fn button_frame(name: &str, size: [f64; 2]) -> EngineFrame {
    let props = serde_json::json!({
        "size": size, "anchor_from": "top_left", "anchor_to": "top_left",
        "button_mappings": [
            {"from_button_id":"button.menu_select", "to_button_id":name, "mapping_type":"pressed"},
            {"from_button_id":"button.menu_secondary_select", "to_button_id":format!("{name}.secondary"), "mapping_type":"pressed"}
        ]
    });
    let root = ResolvedControl {
        name: name.into(),
        control_type: Some("button".into()),
        base: None,
        unresolved_base: None,
        properties: props
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
            .into(),
        children: Vec::new(),
        factory: None,
    };
    let (laid, report) = json_ui::layout_with(
        &root,
        [200.0, 100.0],
        &LayoutEnv {
            text: &EmptyArt,
            textures: &EmptyArt,
        },
        &ViewState::default(),
    );
    EngineFrame {
        hits: json_ui::hit_regions(&laid).into(),
        report,
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    }
}

/// Installs an overlay button beside a view that also has a full-screen dismiss hit.
fn overlapping_screens() -> ModUi {
    let mut presentation = ModUi::default();
    let files = Arc::new(screen::Files {
        namespace: "input_test".into(),
        templates: Default::default(),
        textures: Vec::new(),
    });
    presentation.set_mod_screens(
        Some(ModScreensInput {
            id: "input_test",
            files: &files,
            overlay: None,
            view: None,
            focus: None,
            data: &screen::Modal::default(),
        }),
        0,
    );
    let screens = presentation.mod_screens.as_mut().unwrap();
    screens.overlay.frame = Some(button_frame("overlay", [20.0, 20.0]));
    screens.view.frame = Some(button_frame("dismiss", [200.0, 100.0]));
    screens.layout = Some(ScreenLayout {
        screen: "inventory".into(),
        size: GuiSize {
            width: 200.0,
            height: 100.0,
            scale: 1.0,
        },
        gui: Rect {
            x: 50.0,
            y: 20.0,
            width: 100.0,
            height: 60.0,
        },
        exclusions: Vec::new(),
        view: Some(Rect {
            x: 50.0,
            y: 20.0,
            width: 100.0,
            height: 60.0,
        }),
    });
    presentation
}

#[test]
fn overlay_primary_and_secondary_clicks_win_over_full_screen_dismiss_hits() {
    for secondary in [false, true] {
        let mut presentation = overlapping_screens();
        let click = |p: &mut ModUi, point, pressed, released| {
            if secondary {
                p.secondary_press_mod_screens(point, pressed, released)
            } else {
                p.press_mod_screens(point, pressed, released)
            }
        };
        let target = if secondary {
            "overlay.secondary"
        } else {
            "overlay"
        };
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), false, true),
            Some((target.into(), None))
        );
        assert_eq!(
            click(&mut presentation, Some([5.0, 5.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), false, true),
            None
        );
        let target = if secondary {
            "dismiss.secondary"
        } else {
            "dismiss"
        };
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), true, false),
            None
        );
        assert_eq!(
            click(&mut presentation, Some([180.0, 80.0]), false, true),
            Some((target.into(), None))
        );
    }
}

#[test]
fn overlay_wheel_scrolls_blank_viewports_with_and_without_an_open_view() {
    for view_open in [false, true] {
        let mut presentation = overlapping_screens();
        let screens = presentation.mod_screens.as_mut().unwrap();
        if !view_open {
            screens.view.frame = None;
            screens.layout.as_mut().unwrap().view = None;
        }
        let frame = screens.overlay.frame.as_mut().unwrap();
        frame.hits = Arc::from([]);
        frame.report.scrolls.insert(
            "rows".into(),
            json_ui::ScrollMetrics {
                content: 100.0,
                viewport: 20.0,
                viewport_rect: Some([0.0, 0.0, 20.0, 20.0]),
                speed: 10.0,
                ..Default::default()
            },
        );
        presentation.hover_mod_screens(Some([5.0, 5.0]));
        assert_eq!(presentation.scroll_mod_screens([5.0, 5.0], 1.0), None);
        let screens = presentation.mod_screens.as_ref().unwrap();
        assert_eq!(screens.overlay.view.scroll.get("rows"), Some(&10.0));
    }
}
