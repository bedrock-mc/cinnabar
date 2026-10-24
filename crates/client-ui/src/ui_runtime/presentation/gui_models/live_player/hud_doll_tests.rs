use std::sync::Arc;

use ui::{IconRef, UiNode, UiNodeId, UiVisual};

use super::super::super::player_preview::PreviewView;
use super::tests::{player_catalog_geometry, player_stream};
use super::*;

/// Original geometry whose body, head and limbs differ from the standard biped preview.
fn session_player() -> chunk_pipeline::WorldStream {
    let catalog = player_catalog_geometry(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"body","parent":"root","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
        {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
        {"name":"leftLeg","parent":"root","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]}]}]}"#
            .to_vec(),
    );
    player_stream(Arc::clone(&catalog), catalog)
}

#[test]
fn hud_doll_keeps_its_live_pose_while_the_inventory_shows_its_own_doll() {
    let stream = session_player();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.gui_models.enabled = true;
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    let preview = IconRef {
        color_mask: None,
        page: u16::MAX,
        uv: [0, 0, 96, 112],
        glint: false,
    };
    presentation.player_preview_icon = Some(preview);
    presentation.capture_hud_player(Some(&stream), false);
    // The HUD paints its doll first; the inventory's live renderer paints after it.
    presentation.hud_preview_view = PreviewView::Hud;
    presentation.player_preview_view = PreviewView::Live {
        offset: [40.0, -25.0],
    };
    let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
    let placeholder = |id| {
        UiNode::new(UiNodeId::new(id), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: preview.page,
            uv: preview.uv,
            color: [255; 4],
        })
    };
    let mut nodes = vec![placeholder(1), placeholder(2)];
    presentation.apply_gui_models_in(
        &mut nodes,
        bounds,
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
        0..1,
    );
    let [UiVisual::Mesh(hud), UiVisual::Mesh(inventory)] = [nodes[0].visual(), nodes[1].visual()]
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
        inventory.vertices(),
        expected(presentation.player_preview_view).vertices(),
        "the inventory doll keeps its pointer-facing preview"
    );
    // The HUD draws the session's live geometry, not the inventory's preview model.
    assert_ne!(hud.vertices().len(), inventory.vertices().len());
}
