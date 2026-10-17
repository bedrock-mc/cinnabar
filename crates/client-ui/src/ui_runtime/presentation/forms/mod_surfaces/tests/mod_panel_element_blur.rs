//! Element blur end to end: a filtered subtree publishes its draws in one filter group closed
//! by a composite carrying its blur, and an entrance transition eases the blur away.

use std::collections::BTreeMap;

use mod_ui::PanelUiOwner;
use render_api::UiBackdropPaint;
use render_model::{UI_BLEND_ALPHA, UI_BLEND_BACKDROP, UI_BLEND_FILTER, UiRenderInput};
use serde_json::json;
use ui::{
    DpiScale,
    mod_panel::{Panel, Surface, SurfaceTransition, SurfaceValue, TransitionEasing},
};

use super::super::super::snapshot;
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::test_support::sample_panel as panel;

/// A glass card at (60, 60) holding a green fill and a label, blurred by `#entering`, which eases
/// in from 5 GUI pixels to 0 over a quarter second, beside an unfiltered red marker.
fn entering_panel() -> Panel {
    let fill = |color: [f64; 4], offset: [f64; 2]| {
        json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[40,20],
            "offset":offset,"anchor_from":"top_left","anchor_to":"top_left","radius":0,
            "color":color})
    };
    let mut panel = panel();
    panel.surface = Some(Surface { collections: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[
                {"card":{"type":"panel","size":[100,60],"offset":[60,60],
                    "anchor_from":"top_left","anchor_to":"top_left","cinnabar_filter":{},
                    "bindings":[{"binding_name":"#entering","binding_name_override":"#cinnabar_blur"}],
                    "controls":[{"glass":{"type":"custom","renderer":"cinnabar_glass",
                            "size":["100%","100%"],"blur":8,"color":[0,0,0,0.5]}},
                        {"fill":fill([0.0, 1.0, 0.0, 1.0], [0.0, 0.0])},
                        {"title":{"type":"label","text":"Modules","offset":[0,30],
                            "anchor_from":"top_left","anchor_to":"top_left","color":[1,1,1,1]}}]}},
                {"marker":fill([1.0, 0.0, 0.0, 1.0], [300.0, 60.0])}
            ]
        }})
        .to_string(),
        bindings: BTreeMap::from([("#entering".into(), SurfaceValue::Number(0.0))]),
        transitions: BTreeMap::from([(
            "#entering".into(),
            SurfaceTransition {
                duration: 0.25,
                easing: TransitionEasing::default(),
                from: Some(SurfaceValue::Number(5.0)),
                exit: None,
            },
        )]),
    });
    panel
}

fn frame_at(presentation: &mut UiPresentationRuntime, millis: u64) -> UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &UiRuntime::new(1),
            millis,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Each filter group's batch count and its composite's blur, in draw order.
fn groups(input: &UiRenderInput) -> Vec<(usize, f32)> {
    let mut groups = Vec::new();
    let mut members = 0;
    for batch in input.batches.iter() {
        if batch.blend_mode == UI_BLEND_BACKDROP {
            // Glass inside the group still samples the world, not the group.
            assert_eq!(batch.filter_group, None);
        } else if batch.filter_group.is_some() {
            assert_eq!(batch.blend_mode, UI_BLEND_ALPHA);
            members += 1;
        } else if batch.blend_mode == UI_BLEND_FILTER {
            let vertex = input.vertices[input.indices[batch.first_index as usize] as usize];
            groups.push((
                members,
                UiBackdropPaint::from_overlay(vertex.overlay_color).blur,
            ));
            members = 0;
        } else {
            assert_eq!(
                members, 0,
                "a group's draws are contiguous before its composite"
            );
        }
    }
    groups
}

#[test]
fn a_filtered_subtree_blurs_as_one_group_while_its_entrance_eases() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&entering_panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let opening = frame_at(&mut presentation, 1_000);
    let px = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap()
        .scale;
    let [(members, blur)] = groups(&opening)[..] else {
        panic!("one filter group: {:?}", groups(&opening));
    };
    assert!(members > 0, "the filtered group contains drawing");
    for rgb in [[0, 0, 0], [0, 255, 0], [255, 255, 255]] {
        assert!(
            opening
                .batches
                .iter()
                .filter(|batch| batch.filter_group.is_some())
                .any(|batch| opening.indices
                    [batch.first_index as usize..(batch.first_index + batch.index_count) as usize]
                    .iter()
                    .any(|index| opening.vertices[*index as usize].color[..3] == rgb)),
            "glass tint, fill and text colors must remain inside the filter group: {rgb:?}"
        );
    }
    assert!(
        opening
            .batches
            .iter()
            .any(|batch| batch.blend_mode == UI_BLEND_BACKDROP)
    );
    assert_eq!(blur, 5.0 * px);
    // Unblurred, the CPU snapshot still shows the group's draws and the marker outside it.
    let image = snapshot::rasterize(&opening);
    let at = |x: f32, y: f32| image.get_pixel((x * px) as u32, (y * px) as u32).0;
    assert_eq!(at(80.0, 70.0), [0, 255, 0, 255]);
    assert_eq!(at(320.0, 70.0), [255, 0, 0, 255]);

    assert_eq!(groups(&frame_at(&mut presentation, 1_125))[0].1, 2.5 * px);
    let settled = frame_at(&mut presentation, 1_250);
    assert!(groups(&settled).is_empty(), "a zero blur draws straight");
    assert!(
        settled
            .batches
            .iter()
            .all(|batch| batch.filter_group.is_none())
    );
}
