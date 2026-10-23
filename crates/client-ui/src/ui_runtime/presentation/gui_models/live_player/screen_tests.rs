use std::sync::Arc;

use ui::{IconRef, UiNode, UiNodeId, UiVisual};

use super::super::super::player_preview::PreviewView;
use super::tests::{player_catalog_geometry, player_stream};
use super::*;

/// Original geometry whose head carries a child layer, beside an arm and a leg.
fn headed_player() -> chunk_pipeline::WorldStream {
    let catalog = player_catalog_geometry(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
        {"name":"hat","parent":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[32,0],"inflate":0.5}]},
        {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
        {"name":"leftLeg","parent":"root","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]}]}]}"#
            .to_vec(),
    );
    player_stream(Arc::clone(&catalog), catalog)
}

/// Rig bone indices of the head and its child layer.
fn head_bones(stream: &chunk_pipeline::WorldStream) -> [u32; 2] {
    let rig = stream.authority().actor_rig(1).unwrap();
    ["head", "hat"].map(|name| {
        rig.bone_names
            .iter()
            .position(|bone| bone.as_ref() == name)
            .unwrap() as u32
    })
}

/// The screen capture's body vertices with `view`'s head turn applied.
fn turned(presentation: &UiPresentationRuntime, view: PreviewView) -> Vec<ActorVertex> {
    let live = &presentation.gui_models.screen_player;
    let turn = live
        .head_turn(view)
        .expect("a live renderer turns the head");
    live.vertices
        .iter()
        .map(|vertex| ActorVertex {
            position: if turn.moves(vertex.part) {
                turn.transform
                    .transform_point3(Vec3::from_array(vertex.position))
                    .to_array()
            } else {
                vertex.position
            },
            ..*vertex
        })
        .collect()
}

#[test]
fn live_renderer_turns_only_the_head_and_its_children_toward_the_pointer() {
    let stream = headed_player();
    let head = head_bones(&stream);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.capture_hud_player(Some(&stream), false);
    let hud = presentation.gui_models.live_player.vertices.clone();
    assert!(!hud.is_empty());
    presentation.capture_screen_player(Some(&stream));
    assert!(
        presentation
            .gui_models
            .live_player
            .head_turn(PreviewView::Hud)
            .is_none(),
        "the HUD keeps the actor's own look"
    );

    // A centred pointer leaves the actor's straight-ahead look unchanged.
    let centred = turned(&presentation, PreviewView::Live { offset: [0.0; 2] });
    assert_eq!(centred.len(), hud.len());
    for (screen, hud) in centred.iter().zip(&hud) {
        for axis in 0..3 {
            assert!((screen.position[axis] - hud.position[axis]).abs() < 1e-5);
        }
    }

    let turned = turned(
        &presentation,
        PreviewView::Live {
            offset: [40.0, -25.0],
        },
    );
    let in_head = |vertex: &ActorVertex| head.contains(&(vertex.part - 6));
    let mut moved = [false; 2];
    for (screen, hud) in turned.iter().zip(&hud) {
        let changed = (0..3).any(|axis| (screen.position[axis] - hud.position[axis]).abs() > 1e-3);
        if in_head(hud) {
            moved[usize::from(hud.part - 6 == head[1])] |= changed;
        } else {
            assert!(!changed, "the body, arm and leg keep the animated pose");
        }
    }
    assert_eq!(
        moved, [true; 2],
        "the head and its child layer turn together"
    );
    // The turn is rigid: the head's layers keep their distances from each other.
    let layer = |vertices: &[ActorVertex], bone: u32| {
        vertices
            .iter()
            .find(|vertex| vertex.part - 6 == bone)
            .map(|vertex| Vec3::from_array(vertex.position))
            .unwrap()
    };
    let span =
        |vertices: &[ActorVertex]| layer(vertices, head[0]).distance(layer(vertices, head[1]));
    assert!((span(&turned) - span(&hud)).abs() < 1e-4);
}

const PREVIEW: IconRef = IconRef {
    page: u16::MAX,
    uv: [0, 0, 96, 112],
    glint: false,
};

/// A presentation drawing model geometry for the [`PREVIEW`] placeholder.
fn model_presentation() -> UiPresentationRuntime {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    presentation.player_preview_icon = Some(PREVIEW);
    presentation
}

/// One node per id showing the [`PREVIEW`] placeholder.
fn placeholders(ids: &[u32]) -> Vec<UiNode> {
    let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
    ids.iter()
        .map(|&id| {
            UiNode::new(UiNodeId::new(id), None, bounds).with_visual(UiVisual::Sprite {
                texture_page: PREVIEW.page,
                uv: PREVIEW.uv,
                color: [255; 4],
            })
        })
        .collect()
}

/// Applies model geometry to a 96x112 frame whose first `hud` nodes the HUD painted.
fn apply(presentation: &UiPresentationRuntime, nodes: &mut [UiNode], hud: usize) {
    let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
    presentation.apply_gui_models_in(
        nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        0..hud,
    );
}

#[test]
fn a_live_renderer_outside_containers_keeps_a_current_capture_and_drops_it_when_gone() {
    let stream = headed_player();
    let mut presentation = model_presentation();
    let pointed = PreviewView::Live {
        offset: [40.0, -25.0],
    };
    presentation.refresh_screen_player(Some(&stream), false);
    assert!(
        presentation.live_capture(pointed).is_none(),
        "nothing drew it yet"
    );

    // With the inventory closed, a pack HUD draws a live renderer.
    presentation.hud_preview_view = pointed;
    presentation.player_preview_view = pointed;
    apply(&presentation, &mut placeholders(&[1]), 1);
    presentation.refresh_screen_player(Some(&stream), false);
    assert!(
        presentation.live_capture(pointed).is_some(),
        "the drawn renderer keeps a current capture"
    );

    // Once nothing draws the renderer, its last pose is not kept for a later control.
    apply(&presentation, &mut placeholders(&[]), 0);
    presentation.refresh_screen_player(Some(&stream), false);
    assert!(presentation.live_capture(pointed).is_none());
}

#[test]
fn each_live_renderer_turns_the_head_with_its_own_current_view() {
    let stream = headed_player();
    let mut presentation = model_presentation();
    let mesh = |presentation: &UiPresentationRuntime, view| {
        presentation
            .project_gui_player_mesh(
                &presentation.gui_player_sources(view).unwrap(),
                [None, None],
                view,
            )
            .unwrap()
    };
    let earlier = PreviewView::Live { offset: [0.0; 2] };
    let current = PreviewView::Live {
        offset: [40.0, -25.0],
    };
    // The capture runs before this frame's paint moves the pointer.
    presentation.player_preview_view = earlier;
    presentation.capture_screen_player(Some(&stream));
    presentation.player_preview_view = current;
    let painted = mesh(&presentation, current);
    // Capturing again after the paint draws the same head: no pose lags behind its body.
    presentation.capture_screen_player(Some(&stream));
    assert_eq!(painted.vertices(), mesh(&presentation, current).vertices());
    // A second renderer at another offset turns the same capture's head its own way.
    presentation.player_preview_view = earlier;
    let other = mesh(&presentation, current);
    assert_eq!(other.vertices(), painted.vertices());
}

#[test]
fn hud_doll_keeps_its_own_pose_while_a_screen_shows_the_live_renderer() {
    let stream = headed_player();
    let mut presentation = model_presentation();
    presentation.capture_hud_player(Some(&stream), false);
    // The HUD paints its doll first; the inventory's live renderer paints after it.
    presentation.hud_preview_view = PreviewView::Hud;
    presentation.player_preview_view = PreviewView::Live {
        offset: [40.0, -25.0],
    };
    presentation.capture_screen_player(Some(&stream));
    let mut nodes = placeholders(&[1, 2]);
    apply(&presentation, &mut nodes, 1);
    let [UiVisual::Mesh(hud), UiVisual::Mesh(screen)] = [nodes[0].visual(), nodes[1].visual()]
    else {
        panic!("both dolls draw model geometry");
    };
    let expected = |view| {
        presentation
            .project_gui_player_mesh(
                &presentation.gui_player_sources(view).unwrap(),
                [None, None],
                view,
            )
            .unwrap()
    };
    assert_eq!(
        hud.vertices(),
        expected(PreviewView::Hud).vertices(),
        "the HUD doll keeps the actor's animated HUD pose"
    );
    assert_eq!(
        screen.vertices(),
        expected(presentation.player_preview_view).vertices()
    );
    // Both dolls draw the session's animated actor, each posed by its own renderer.
    assert_eq!(screen.vertices().len(), hud.vertices().len());
    assert_ne!(screen.vertices(), hud.vertices());
}
