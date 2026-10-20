use super::*;
use std::sync::Arc;

use crate::{test_support::mini_engine_presentation, ui_runtime::UiRuntime};
use mod_ui::PanelUiOwner;
use serde_json::json;
use ui::{DpiScale, SafeArea, mod_hud::HudBindings, mod_panel::SurfaceValue};

const KEEP_OUT_TEXTURE: &str = "textures/ui/editor_fixture";

/// Builds real inert HUD roots, including nested transforms, hidden and ambiguous names.
fn presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    let panel = |name: &str, visible: bool| json!({name:{"type":"input_panel","size":[20,10],"visible":visible}});
    presentation.set_server_ui_pack(&super::super::super::super::ServerUiPack {
        ui_layers:vec![vec![
            ("ui/_ui_defs.json".into(),br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec()),
            ("ui/hud_screen.json".into(),serde_json::to_vec(&json!({
                "namespace":"hud","hud_screen":{"type":"screen","controls":[
                    {"workspace":{"type":"panel","size":[500,300],"offset":[20,10],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "cinnabar_transform":{"scale":1.25,"origin":[0,0]},
                        "controls":[{"extension_hud_fixture":{"type":"input_panel",
                            "size":[80,27],"offset":[100,80],
                            "anchor_from":"top_left","anchor_to":"top_left",
                            "cinnabar_transform":{"scale":1.5,"origin":[0,0]},
                            "controls":[{"label":{"type":"label","text":"HUD","size":[80,27]}}]
                        }}]}},
                    {"first":{"type":"panel","controls":[panel("ambiguous",true)]}},
                    {"second":{"type":"panel","controls":[panel("ambiguous",true)]}},
                    panel("hidden",false),
                    {"mapped":{"type":"input_panel","size":[20,10],"button_mappings":[{
                        "from_button_id":"button.menu_select","to_button_id":"example.action","mapping_type":"pressed"
                    }]}}
                ]}
            })).unwrap())
        ]],..Default::default()
    });
    presentation
        .set_mod_hud_bindings(
            PanelUiOwner::PacketSession(1),
            Some(HudBindings(BTreeMap::from([(
                "#mod_geometry_enabled".into(),
                SurfaceValue::Bool(true),
            )]))),
        )
        .unwrap();
    presentation
}

/// Paints the actual HUD for one session without game assets or a network connection.
fn build(presentation: &mut UiPresentationRuntime, session: u64) {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    presentation
        .build(
            &player,
            &UiRuntime::new(session),
            0,
            [2560, 1600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

#[test]
fn geometry_tracks_actual_root_bounds_across_hud_scales_and_excludes_safe_area_origin() {
    let requests = HudGeometryRequests(vec!["extension_hud_fixture".into()]);
    let mut reported = Vec::new();
    for preference in [1, 4] {
        let mut presentation = presentation();
        presentation.set_safe_area(SafeArea::new(20.0, 10.0, 40.0, 30.0).unwrap());
        presentation.set_gui_scale_preference(Some(preference));
        build(&mut presentation, 7);
        let passes = presentation.hud_passes();
        let geometry = presentation.mod_hud_geometry(&requests, 7);
        let root = &geometry["extension_hud_fixture"];
        let frame = presentation.screens.touch.hud.as_ref().unwrap();
        assert_eq!(frame.origin, [20.0, 10.0]);
        let unit = frame.scale / FONT_DESIGN_PIXEL_TEXELS as f32;
        assert_eq!(
            root.rect,
            [145.0, 110.0, 150.0, 50.625].map(|value| value * unit)
        );
        assert_eq!(root.size, [80.0, 27.0].map(|value| value * unit));
        assert_eq!(
            presentation.hud_passes(),
            passes,
            "reading geometry never lays out again"
        );
        reported.push(root.clone());
    }
    assert_ne!(
        reported[0], reported[1],
        "HUD GUI preferences must reach the panel's coordinate space"
    );
}

#[test]
fn hidden_missing_mapped_ambiguous_and_old_session_roots_are_not_reported() {
    let mut presentation = presentation();
    let requests = HudGeometryRequests(vec![
        "extension_hud_fixture".into(),
        "hidden".into(),
        "missing".into(),
        "mapped".into(),
        "ambiguous".into(),
    ]);
    assert!(
        presentation.mod_hud_geometry(&requests, 1).is_empty(),
        "there is no completed HUD frame"
    );
    build(&mut presentation, 1);
    let geometry = presentation.mod_hud_geometry(&requests, 1);
    assert_eq!(
        geometry.keys().map(String::as_str).collect::<Vec<_>>(),
        ["extension_hud_fixture"]
    );
    assert!(presentation.mod_hud_geometry(&requests, 2).is_empty());
    build(&mut presentation, 2);
    assert!(presentation.mod_hud_geometry(&requests, 1).is_empty());
    assert_eq!(presentation.mod_hud_geometry(&requests, 2).len(), 1);
    presentation.screens.touch.begin_frame();
    assert!(
        presentation.mod_hud_geometry(&requests, 2).is_empty(),
        "unpainted HUD geometry is retired with its frame"
    );
}

/// Builds real custom HUD controls with original white texture pixels.
fn keep_out_presentation(
    controls: serde_json::Value,
    definitions: serde_json::Value,
) -> UiPresentationRuntime {
    let carrier = crate::test_support::mini_carrier();
    let mut files = carrier.ui_files().to_vec();
    files
        .iter_mut()
        .find(|file| file.path.as_ref() == "ui/_ui_defs.json")
        .unwrap()
        .bytes = br#"{"ui_defs":["ui/server_form.json","ui/hud_screen.json"]}"#
        .as_slice()
        .into();
    files.push(assets::UiFile {
        path: "ui/hud_screen.json".into(),
        bytes: br#"{"namespace":"hud","hud_screen":{"type":"screen"}}"#
            .as_slice()
            .into(),
    });
    let bytes =
        assets::encode_ui_catalog([1; 32], carrier.atlas_pages(), &[], &[], &files).unwrap();
    let mut p = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    p.enable_json_ui(Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(9, 9, image::Rgba([255; 4]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    let mut hud = json!({"namespace":"hud","hud_screen":{"type":"screen","controls":controls},
        "hotbar_hud_item_icon":{"type":"custom","renderer":"inventory_item_renderer","size":[16,16]},
        "item_durability":{"type":"panel","size":[0,0]}});
    hud.as_object_mut()
        .unwrap()
        .extend(definitions.as_object().unwrap().clone());
    p.set_server_ui_pack(&super::super::super::super::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json","ui/common.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                serde_json::to_vec(&hud).unwrap(),
            ),
            (
                "ui/common.json".into(),
                br#"{"namespace":"common","stack_count_label":{"type":"label","text":""}}"#
                    .to_vec(),
            ),
        ]],
        textures: vec![
            ("textures/ui/hotbar_0.png".into(), png.clone()),
            (format!("{KEEP_OUT_TEXTURE}.png"), png),
        ],
        ..Default::default()
    });
    p
}

/// Keep-out input follows actual native painting across GUI scales, DPI and safe-area origins.
#[test]
fn editor_keep_out_uses_the_completed_native_layout_and_rejects_old_sessions() {
    let mut p = keep_out_presentation(
        json!([{"bar":{"type":"custom","renderer":"hotbar_renderer",
        "size":[182,22],"offset":[30,200],"anchor_from":"top_left","anchor_to":"top_left",
        "cinnabar_transform":{"scale":1.5,"origin":[0,0]}}}]),
        json!({}),
    );
    p.set_safe_area(SafeArea::new(20., 10., 30., 15.).unwrap());
    p.mod_ui_mut()
        .set_mod_panel(
            PanelUiOwner::Environment(0),
            Some(&mod_ui::test_support::sample_panel()),
        )
        .unwrap();
    let runtime = UiRuntime::new(7);
    let player = player_state::PlayerState::new(1);
    for (gui, dpi) in [(1, 1.), (2, 1.), (4, 1.), (4, 2.), (2, 1.5)] {
        p.set_gui_scale_preference(Some(gui));
        p.mod_ui_mut().set_mod_panel_open(false);
        let native_output = p
            .build(
                &player,
                &runtime,
                0,
                [2560, 1600],
                DpiScale::new(dpi).unwrap(),
            )
            .unwrap();
        assert!(
            !native_output.vertices.is_empty(),
            "original native sprite paints"
        );
        let painted: [[f32; 2]; 2] = std::array::from_fn(|axis| {
            native_output
                .batches
                .iter()
                .flat_map(|batch| {
                    &native_output.indices[batch.first_index as usize
                        ..(batch.first_index + batch.index_count) as usize]
                })
                .map(|index| &native_output.vertices[*index as usize])
                .fold([f32::INFINITY, f32::NEG_INFINITY], |[low, high], vertex| {
                    let at = vertex.position[axis] / vertex.clip_w / dpi;
                    [low.min(at), high.max(at)]
                })
        });
        p.mod_ui_mut().set_mod_panel_open(true);
        p.mod_ui_mut()
            .open_mod_hud_editor(&ui::mod_hud::Hud::default())
            .unwrap();
        p.build(
            &player,
            &runtime,
            0,
            [2560, 1600],
            DpiScale::new(dpi).unwrap(),
        )
        .unwrap();
        let native = p.screens.touch.hud.as_ref().unwrap();
        assert!(
            native.hits.iter().all(|hit| hit.renderer.is_none()),
            "ordinary custom artwork has no input hits"
        );
        let editor = p.mod_ui().mod_hud_editor_state().unwrap().frame.unwrap();
        let reported = p.mod_hud_editor_native_keep_out(7).unwrap();
        for axis in 0..2 {
            let actual_start = painted[axis][0];
            let actual_size = painted[axis][1] - painted[axis][0];
            let editor_start = editor.origin[axis] + reported[axis] as f32 * editor.scale;
            let editor_size = reported[axis + 2] as f32 * editor.scale;
            assert!(
                (actual_start - editor_start).abs() < 0.001,
                "gui {gui}, dpi {dpi}, axis {axis}"
            );
            assert!(
                (actual_size - editor_size).abs() < 0.001,
                "gui {gui}, dpi {dpi}, axis {axis}"
            );
        }
        let passes = p.hud_passes();
        assert!(p.mod_hud_editor_native_keep_out(7).is_some());
        assert_eq!(p.hud_passes(), passes);
    }
    assert_eq!(p.mod_hud_editor_native_keep_out(8), None);
    p.screens.touch.begin_frame();
    assert_eq!(p.mod_hud_editor_native_keep_out(7), None);
}

/// Status artwork is observed from actual native paints, including control overflow and clipping.
#[test]
fn editor_keep_out_captures_standalone_status_controls_without_input_hits() {
    use ui_screens::hud_renderers::{Cell, HeartPaint, HudPaint, HungerPaint};
    let renderers = [
        "heart_renderer",
        "hunger_renderer",
        "armor_renderer",
        "bubbles_renderer",
        "horse_heart_renderer",
    ];
    let controls: Vec<_> = renderers
        .into_iter()
        .map(|renderer| {
            json!({renderer:{
                "type":"custom","renderer":renderer,"size":[1,1],"offset":[60,15],
                "anchor_from":"top_left","anchor_to":"top_left"
            }})
        })
        .collect();
    let mut p = keep_out_presentation(
        json!([{"clip":{
            "type":"panel","size":[100,30],"offset":[140,85],"clip_children":true,
            "anchor_from":"top_left","anchor_to":"top_left","controls":controls
        }}]),
        json!({}),
    );
    let icon = Cell::icon([0., -10.], KEEP_OUT_TEXTURE);
    let paint = HudPaint {
        hearts: HeartPaint {
            current: 40,
            health_hearts: 21,
            total: 21,
            sprites: [Some(icon.clone()), None, None, None, None, None, None],
            ..Default::default()
        },
        hunger: HungerPaint {
            current: 20,
            sprites: Some([KEEP_OUT_TEXTURE; 3]),
            ..Default::default()
        },
        armor: vec![icon.clone()],
        bubbles: vec![icon.clone()],
        mount_hearts: vec![icon],
        ..Default::default()
    };
    let (bounds, drawn, nodes) = paint_keep_out(&mut p, &paint, json_ui::DataSource::new());
    assert!(drawn.hits.iter().all(|hit| hit.renderer.is_none()));
    assert_eq!(bounds, Some([140., 85., 100., 24.]));
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Sprite { .. })),
        "real native sprites paint"
    );
}

/// Paints actual native controls and returns their observed region, input frame and primitives.
fn paint_keep_out(
    p: &mut UiPresentationRuntime,
    paint: &ui_screens::hud_renderers::HudPaint,
    data: json_ui::DataSource,
) -> (
    Option<[f64; 4]>,
    ui_engine::frame::EngineFrame,
    Vec<ui::UiNode>,
) {
    use ui_engine::{engine::screen::PaintFrame, text_metrics::TextMetrics};
    use ui_screens::hud::{HudInput, HudScreens};
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.).unwrap(), Some(1));
    let mut screens = HudScreens::default();
    let mut nodes = Vec::new();
    let mut next = 1;
    let engine = p.engine.as_deref().unwrap();
    let mut no_renderers = [];
    let mut frame = PaintFrame {
        layouts: &mut p.layouts,
        font: &p.font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: [1280., 720.],
        translate: &|_| None,
        language: [0; 3],
        now: 0.,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut no_renderers,
    };
    let drawn = screens
        .paint(
            engine,
            HudInput {
                crosshair: false,
                capture_keep_out: true,
                catalog: engine.catalog(),
                context: &json_ui::Context::desktop(),
                data: Arc::new(data),
                paint,
                icons: &[],
            },
            &mut frame,
        )
        .unwrap()
        .unwrap();
    (screens.editor_keep_out, drawn, nodes)
}

/// The shipped offhand frame is an ordinary image under the bottom HUD, outside the hotbar.
#[test]
fn editor_keep_out_includes_the_shipped_offhand_image_frame() {
    let mut p = keep_out_presentation(
        json!([{"bottom_alias@hud.centered_gui_elements_at_bottom_middle":{
            "type":"panel","size":[200,22],"offset":[100,100],"anchor_from":"top_left","anchor_to":"top_left",
            "controls":[
                {"bar":{"type":"custom","renderer":"hotbar_renderer","size":[200,22],"anchor_from":"top_left","anchor_to":"top_left"}},
                {"java_offhand@hud.java_offhand_slot":{"$offhand_texture":KEEP_OUT_TEXTURE}}
            ]
        }}]),
        json!({}),
    );
    let mut data = json_ui::DataSource::new();
    data.set_global("#hud_visible_centered", json_ui::Scalar::Bool(true));
    data.set_global("#offhand_visible", json_ui::Scalar::Bool(true));
    let (bounds, _, nodes) = paint_keep_out(&mut p, &Default::default(), data);
    let bounds = bounds.expect("native bottom HUD paints");
    assert!(
        bounds[0] < 100.,
        "offhand image extends left of the main hotbar"
    );
    assert!(
        bounds[2] > 200.,
        "region includes both the offhand frame and main hotbar"
    );
    assert!(
        nodes
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::Sprite { .. }))
            .count()
            >= 2
    );
}

/// Aliases inherit the resolved bottom scope without capturing unrelated same-named children.
#[test]
fn editor_keep_out_resolves_and_qualifies_bottom_subtrees() {
    for dynamic in [false, true] {
        let children =
            json!([{"panel":{"type":"image","texture":KEEP_OUT_TEXTURE,"size":["100%","100%"]}}]);
        let mut p = keep_out_presentation(
            json!([
                {"bottom_alias@hud.centered_gui_elements_at_bottom_middle":{
                    "size":[40,20],"offset":[100,100],"anchor_from":"top_left","anchor_to":"top_left"}},
                {"outside":{"type":"panel","size":[60,20],"offset":[900,20],
                    "anchor_from":"top_left","anchor_to":"top_left","controls":children}}
            ]),
            json!({
                "alternative_base":{"type":"panel","$contents":children},
                "centered_gui_elements_at_bottom_middle@hud.alternative_base":{
                    "controls":if dynamic {json!("$contents")} else {children}
                }
            }),
        );
        let (bounds, _, _) =
            paint_keep_out(&mut p, &Default::default(), json_ui::DataSource::new());
        assert_eq!(
            bounds,
            Some([100., 100., 40., 20.]),
            "dynamic controls: {dynamic}"
        );
    }
}
