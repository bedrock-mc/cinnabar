use super::*;
use crate::ui_runtime::{
    oreui_assets::{OreUiImages, OreUiSprite},
    presentation::{TextMetrics, forms::oreui::paint::Originals, tests::fixture_font},
};
use std::{collections::HashMap, sync::Arc};

#[test]
fn native_faces_crossfade_and_depress_without_moving_their_hit_area() {
    use super::super::{Variant, button};
    use crate::ui_runtime::presentation::forms::oreui::transitions::Transitions;
    use launcher::menu::{MenuAction, MenuView};
    let sprites = Arc::new(
        artwork(Variant::Primary)
            .into_iter()
            .enumerate()
            .map(|(index, key)| {
                (
                    key.to_owned(),
                    OreUiSprite {
                        page: index as u16,
                        bounds: [0, 0, 16, 16],
                    },
                )
            })
            .collect::<HashMap<_, _>>(),
    );
    let originals = Originals {
        page: 10,
        images: OreUiImages {
            pages: Vec::new(),
            sprites: sprites.clone(),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        },
        sprites,
        masks: HashMap::new(),
        loading_frames: Default::default(),
        animations: Default::default(),
    };
    let font = fixture_font();
    let mut layouts = ui::TextLayoutCache::new(16, 4096);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut transitions = Transitions::default();
    transitions.end_frame();
    let mut view = MenuView::new(true, "Player".into());
    view.hovered = Some(MenuAction::PauseResume);
    let bounds = [100.0, 100.0, 400.0, 164.0];
    let mut draw = |view: &MenuView, seconds: f64| {
        let (mut nodes, mut next) = (Vec::new(), 1);
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        canvas.seconds = seconds;
        canvas.transitions = Some(&mut transitions);
        button(
            &mut canvas,
            view,
            bounds,
            Variant::Primary,
            "",
            Some(MenuAction::PauseResume),
        )
        .unwrap();
        let hits = canvas.hits.clone();
        let rem = canvas.rem;
        drop(canvas);
        (nodes, hits, rem)
    };
    draw(&view, 1.0);
    let (hovered, hits, rem) = draw(&view, 1.04);
    assert!(hovered.iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page: 11, color, .. } if color[3] > 0 && color[3] < 255)));
    assert_eq!(hits[0].1.min().y(), bounds[1]);
    view.pressed = Some(MenuAction::PauseResume);
    draw(&view, 2.0);
    let (pressed, press_hits, _) = draw(&view, 2.02);
    assert_eq!(press_hits, hits);
    assert!(pressed[0].bounds().min().y() > bounds[1]);
    assert!(pressed[0].bounds().min().y() < bounds[1] + rem * 0.4);
    let (settled, settled_hits, _) = draw(&view, 2.10);
    assert_eq!(settled_hits, hits);
    assert!(settled.iter().all(|node| matches!(
        node.visual(),
        ui::UiVisual::Sprite {
            texture_page: 13,
            color: [255, 255, 255, 255],
            ..
        }
    )));
}

#[test]
fn native_button_focus_beats_hover_and_pressed_and_disabled_keep_precedence() {
    let keys = artwork(Variant::Primary);
    let sprites = Arc::new(
        keys.into_iter()
            .chain([BUTTON_DISABLED_IMAGE])
            .enumerate()
            .map(|(index, key)| {
                (
                    key.to_owned(),
                    OreUiSprite {
                        page: index as u16,
                        bounds: [0, 0, 16, 16],
                    },
                )
            })
            .collect::<HashMap<_, _>>(),
    );
    let originals = Originals {
        page: 10,
        images: OreUiImages {
            pages: Vec::new(),
            sprites: sprites.clone(),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        },
        sprites,
        masks: HashMap::new(),
        loading_frames: Default::default(),
        animations: Default::default(),
    };
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    for (state, enabled, expected) in [
        (
            Interaction {
                action: None,
                hovered: true,
                focused: true,
                pressed: false,
            },
            true,
            12,
        ),
        (
            Interaction {
                action: None,
                hovered: true,
                focused: true,
                pressed: true,
            },
            true,
            13,
        ),
        (
            Interaction {
                action: None,
                hovered: true,
                focused: true,
                pressed: true,
            },
            false,
            14,
        ),
    ] {
        let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        assert!(
            elevated(
                &mut canvas,
                [100.0, 100.0, 300.0, 150.0],
                Variant::Primary,
                state,
                enabled
            )
            .unwrap()
        );
        assert!(
            nodes.iter().all(|node| matches!(node.visual(),
                ui::UiVisual::Sprite { texture_page, .. } if *texture_page == expected
            )),
            "the highest-priority native state owns every border-image slice"
        );
    }
}
