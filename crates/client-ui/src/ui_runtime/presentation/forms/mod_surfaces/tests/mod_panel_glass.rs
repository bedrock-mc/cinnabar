//! Glass panels and the world backdrop end to end: the published backdrop geometry, its paint
//! under transitions and transforms, and the tint drawn above it.

use std::collections::BTreeMap;

use mod_ui::PanelUiOwner;
use render_api::UiBackdropPaint;
use render_model::{UI_BLEND_BACKDROP, UiRenderInput};
use serde_json::json;
use ui::{
    DpiScale,
    mod_panel::{
        Panel, Surface, SurfaceTransition, SurfaceValue, TransitionEasing,
        backdrop::{WORLD_BLUR_BINDING, WORLD_BRIGHTNESS_BINDING},
    },
};

use super::super::super::snapshot;
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::test_support::sample_panel as panel;

const SIZE: [u32; 2] = [1280, 720];

/// A 100x40 glass pane at (60, 60) blurring 8 GUI pixels under an opaque blue tint, turned by
/// `rotation` degrees, over a world backdrop that eases to `world` blur and 0.62 brightness.
fn glass_panel(world: Option<f64>, rotation: f64) -> Panel {
    let mut panel = panel();
    panel.motion = true;
    let mut bindings = BTreeMap::new();
    let mut transitions = BTreeMap::new();
    if let Some(blur) = world {
        bindings.insert(WORLD_BLUR_BINDING.into(), SurfaceValue::Number(blur));
        bindings.insert(WORLD_BRIGHTNESS_BINDING.into(), SurfaceValue::Number(0.62));
        transitions.insert(
            WORLD_BLUR_BINDING.into(),
            SurfaceTransition {
                duration: 0.25,
                easing: TransitionEasing::default(),
                from: Some(SurfaceValue::Number(0.0)),
                exit: Some(SurfaceValue::Number(0.0)),
            },
        );
    }
    panel.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[{"pane":{
                "type":"custom","renderer":"cinnabar_glass","size":[100,40],"offset":[60,60],
                "anchor_from":"top_left","anchor_to":"top_left","radius":8,"blur":8,
                "saturation":1.5,"color":[0,0,1,1],"highlight_color":[1,1,1,0.5],
                "cinnabar_transform":{"rotation":rotation}
            }}]
        }})
        .to_string(),
        bindings,
        transitions,
    });
    panel
}

/// Opens `panel` and builds a frame at `millis`.
fn open(panel: &Panel) -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    presentation
}

fn frame_at(presentation: &mut UiPresentationRuntime, millis: u64) -> UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            millis,
            SIZE,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Each consecutive backdrop paint and its physical bounds, independent of batch coalescing.
fn backdrops(input: &UiRenderInput) -> Vec<(UiBackdropPaint, [f32; 4])> {
    let mut paints: Vec<(UiBackdropPaint, [f32; 4])> = Vec::new();
    for batch in input
        .batches
        .iter()
        .filter(|batch| batch.blend_mode == UI_BLEND_BACKDROP)
    {
        let indices = &input.indices
            [batch.first_index as usize..(batch.first_index + batch.index_count) as usize];
        for triangle in indices.as_chunks::<3>().0 {
            let paint =
                UiBackdropPaint::from_overlay(input.vertices[triangle[0] as usize].overlay_color);
            if paints.last().is_none_or(|(previous, _)| *previous != paint) {
                paints.push((paint, [f32::MAX, f32::MAX, f32::MIN, f32::MIN]));
            }
            let bounds = &mut paints.last_mut().unwrap().1;
            for &index in triangle {
                let [x, y] = input.vertices[index as usize].position;
                *bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x),
                    bounds[3].max(y),
                ];
            }
        }
    }
    paints
}

/// Physical pixels per GUI pixel in the open panel.
fn gui_pixels(presentation: &UiPresentationRuntime) -> f32 {
    presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap()
        .scale
}

#[test]
fn surfaces_without_glass_or_world_bindings_publish_no_backdrop() {
    let mut presentation = open(&panel());
    assert!(backdrops(&frame_at(&mut presentation, 1_000)).is_empty());
}

#[test]
fn glass_publishes_its_backdrop_beneath_its_tint() {
    let mut presentation = open(&glass_panel(None, 0.0));
    let input = frame_at(&mut presentation, 1_000);
    let px = gui_pixels(&presentation);
    let [(paint, bounds)] = backdrops(&input)[..] else {
        panic!("one glass backdrop");
    };
    assert_eq!(paint.blur, 8.0 * px);
    assert_eq!((paint.saturation, paint.brightness), (1.5, 1.0));
    let width = (bounds[2] - bounds[0]) / px;
    let height = (bounds[3] - bounds[1]) / px;
    assert!((width - 100.0).abs() < 1.0 && (height - 40.0).abs() < 1.0);
    // The backdrop draws into the scene; the tint above it still reaches the UI layer.
    let centre = [
        ((bounds[0] + bounds[2]) * 0.5) as u32,
        ((bounds[1] + bounds[3]) * 0.5) as u32,
    ];
    let image = snapshot::rasterize(&input);
    assert_eq!(image.get_pixel(centre[0], centre[1]).0, [0, 0, 255, 255]);
}

#[test]
fn the_world_backdrop_eases_in_and_glass_sees_it_beneath() {
    let mut presentation = open(&glass_panel(Some(3.0), 0.0));
    let opening = frame_at(&mut presentation, 1_000);
    let px = gui_pixels(&presentation);
    let [(world, world_bounds), (glass, _)] = backdrops(&opening)[..] else {
        panic!("world then glass backdrops");
    };
    assert_eq!(world_bounds, [0.0, 0.0, SIZE[0] as f32, SIZE[1] as f32]);
    assert_eq!((world.blur, world.brightness), (0.0, 0.62));
    assert_eq!((glass.blur, glass.brightness), (8.0 * px, 0.62));

    let half = backdrops(&frame_at(&mut presentation, 1_125));
    assert_eq!(half[0].0.blur, 1.5 * px);
    let open = backdrops(&frame_at(&mut presentation, 1_250));
    assert_eq!(open[0].0.blur, 3.0 * px);
    let seen = (8.0 * px).hypot(3.0 * px);
    assert!((open[1].0.blur - seen).abs() < 1e-3, "{:?}", open[1].0);
    assert!(!presentation.mod_ui().mod_panel_motion_active());

    // Closing eases the world back out before the panel stops drawing.
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert_eq!(
        backdrops(&frame_at(&mut presentation, 1_500))[0].0.blur,
        3.0 * px
    );
    assert_eq!(
        backdrops(&frame_at(&mut presentation, 1_625))[0].0.blur,
        1.5 * px
    );
    frame_at(&mut presentation, 1_750);
    assert!(backdrops(&frame_at(&mut presentation, 1_800)).is_empty());
}

#[test]
fn rotated_glass_samples_the_backdrop_where_it_is_drawn() {
    let upright = backdrops(&frame_at(&mut open(&glass_panel(None, 0.0)), 1_000))[0].1;
    let turned = backdrops(&frame_at(&mut open(&glass_panel(None, 90.0)), 1_000))[0].1;
    let size = |bounds: [f32; 4]| [bounds[2] - bounds[0], bounds[3] - bounds[1]];
    let [width, height] = size(upright);
    let [turned_width, turned_height] = size(turned);
    assert!((turned_width - height).abs() < 2.0, "{turned:?}");
    assert!((turned_height - width).abs() < 2.0, "{turned:?}");
    let centre = |bounds: [f32; 4]| [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let [x, y] = centre(upright);
    let [turned_x, turned_y] = centre(turned);
    assert!((x - turned_x).abs() < 1.0 && (y - turned_y).abs() < 1.0);
}
