//! A locally supplied pack witnesses startup icon retention through the rendered hotbar.

use super::startup_fixture::{ICON, IDENTIFIER, NETWORK_ID, local_view};
use super::*;
use assets::{RuntimeUiAssets, UiAtlasPage, UiFile, UiTexturePlacement, encode_ui_catalog};
use client_ui::ui_runtime::presentation::{UiPresentationRuntime, refresh_hud_frame};
use serde_json::json;
use ui::DpiScale;

const VIEWPORT: [u32; 2] = [800, 600];

#[test]
fn startup_custom_sword_survives_empty_registry_duplicate_in_rendered_hotbar() {
    let Some(view) =
        local_view("startup_custom_sword_survives_empty_registry_duplicate_in_rendered_hotbar")
    else {
        return;
    };
    let startup = protocol::test_support::items::startup_with_item_icons(&[
        (IDENTIFIER, NETWORK_ID, Some(ICON)),
        (IDENTIFIER, NETWORK_ID, None),
    ]);
    let registry = startup
        .items
        .registry
        .expect("compatible duplicate registry");
    let super::startup_fixture::SelectedInventory {
        stream,
        player,
        mut runtime,
    } = super::startup_fixture::selected_inventory(registry);
    let icons = compile_session_icons(&view, &startup.items.icon_keys, BlockIcons::default());
    let has_icons = icons.is_some();
    runtime.set_session_icons(icons);
    let mut presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
    )
    .unwrap();
    presentation.enable_json_ui(hotbar_carrier()).unwrap();
    presentation.set_gui_scale_preference(Some(4));
    let icon_generation = presentation.session_icon_generation();
    presentation
        .build(&player, &runtime, 0, VIEWPORT, DpiScale::new(1.).unwrap())
        .unwrap();
    if has_icons {
        test_time::eventually("the compiled icon atlas to be admitted", || {
            presentation
                .build(&player, &runtime, 0, VIEWPORT, DpiScale::new(1.).unwrap())
                .unwrap();
            presentation.session_icon_generation() != icon_generation
        });
    }
    refresh_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        semantic_input::PerspectiveMode::FirstPerson,
        1_000,
    );
    let frame = presentation.hud_frame().clone();
    let input = presentation
        .build(
            &player,
            &runtime,
            1_000,
            VIEWPORT,
            DpiScale::new(1.).unwrap(),
        )
        .unwrap();
    client_ui::test_support::write_snapshot(&input, "custom-item-startup");
    assert_eq!(
        frame.hotbar_stacks[0]
            .as_ref()
            .map(|stack| stack.network_id),
        Some(i32::from(NETWORK_ID))
    );
    let icon =
        frame.hotbar_icons[0].expect("the startup icon must survive an empty registry duplicate");
    assert!(
        input
            .batches
            .iter()
            .any(|batch| batch.texture_page == u32::from(icon.page)),
        "the resolved sword texture must reach the rendered hotbar"
    );
}

/// Builds an original minimal HUD whose cells use the production item and hotbar renderers.
fn hotbar_carrier() -> Arc<RuntimeUiAssets> {
    let controls = (0..protocol::HOTBAR_SLOT_COUNT).map(|slot| json!({
        format!("slot_{slot}"): {
            "type": "panel", "size": [20,22], "offset": [2 + u32::from(slot) * 20,0],
            "anchor_from": "top_left", "anchor_to": "top_left", "collection_index": slot,
            "controls": [
                {"background": {"type":"custom", "renderer":"hotbar_renderer", "size":[20,22], "collection_index":slot}},
                {"icon": {"type":"custom", "renderer":"inventory_item_renderer", "size":[16,16],
                    "anchor_from":"top_left", "anchor_to":"top_left", "offset":[2,3], "layer":1,
                    "bindings":[{"binding_type":"collection", "binding_collection_name":"hotbar_items", "binding_name":"#item_renderer_data"}]}}
            ]
        }
    })).collect::<Vec<_>>();
    let hud = json!({"namespace":"hud", "hud_screen": {
        "type":"screen", "controls":[{"fixture_item_closeup": {
            "type":"panel", "size":[120,120],
            "anchor_from":"center", "anchor_to":"center", "offset":[0,-20],
            "collection_name":"hotbar_items", "collection_index":0,
            "controls":[
                {"background":{"type":"custom", "renderer":"hotbar_renderer", "size":[120,120], "collection_index":0}},
                {"icon":{"type":"custom", "renderer":"inventory_item_renderer", "size":[112,112], "collection_index":0,
                    "anchor_from":"top_left", "anchor_to":"top_left", "offset":[4,4], "layer":1,
                    "bindings":[{"binding_type":"collection", "binding_collection_name":"hotbar_items", "binding_name":"#item_renderer_data"}]}}
            ]
        }}, {"fixture_bar": {
            "type":"panel", "size":[182,22], "offset":[0,-12],
            "anchor_from":"bottom_middle", "anchor_to":"bottom_middle",
            "collection_name":"hotbar_items", "controls":controls
        }}]
    }});
    let files = [
        UiFile {
            path: "ui/_global_variables.json".into(),
            bytes: Arc::from(&b"{}"[..]),
        },
        UiFile {
            path: "ui/_ui_defs.json".into(),
            bytes: Arc::from(&br#"{"ui_defs":["ui/hud_screen.json"]}"#[..]),
        },
        UiFile {
            path: "ui/hud_screen.json".into(),
            bytes: serde_json::to_vec(&hud).unwrap().into(),
        },
    ];
    let pixels = (0..22)
        .flat_map(|y| {
            (0..20).flat_map(move |x| {
                if x == 0 || x == 19 || y == 0 || y == 21 {
                    [176, 180, 187, 255]
                } else {
                    [34, 38, 46, 255]
                }
            })
        })
        .collect::<Vec<_>>();
    let page = UiAtlasPage {
        width: 20,
        height: 22,
        rgba8: pixels.into(),
    };
    let textures = (0..protocol::HOTBAR_SLOT_COUNT)
        .map(|slot| UiTexturePlacement {
            path: format!("textures/ui/hotbar_{slot}").into(),
            page: 0,
            x: 0,
            y: 0,
            width: 20,
            height: 22,
        })
        .collect::<Vec<_>>();
    let bytes = encode_ui_catalog([1; 32], &[page], &textures, &[], &files).unwrap();
    Arc::new(RuntimeUiAssets::decode(&bytes).unwrap())
}
