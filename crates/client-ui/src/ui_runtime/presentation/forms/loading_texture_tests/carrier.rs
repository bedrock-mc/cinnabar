//! A loading frame must retain its images after installation removes the source pack.

use std::{io::Cursor, sync::Arc};

use assets::RuntimeUiAssets;

use super::{frame, png, snapshot};
use crate::ui_runtime::presentation::{LoadingStage, UiPresentationRuntime};

fn carrier() -> Arc<RuntimeUiAssets> {
    let root = super::pack_harness::scratch_dir("loading-carrier");
    for dir in ["ui", "textures/ui", "textures/blocks"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("ui/_global_variables.json"), "{}").unwrap();
    std::fs::write(
        root.join("ui/_ui_defs.json"),
        serde_json::json!({
            "ui_defs": ["ui/ui_art_assets_common.json", "ui/progress_screen.json"]
        })
        .to_string(),
    )
    .unwrap();
    let title = png([600, 100], [20, 160, 240, 255]);
    let mut bar = image::RgbaImage::new(640, 8);
    for (x, _, pixel) in bar.enumerate_pixels_mut() {
        *pixel = image::Rgba(if x < 64 {
            [40, 200, 80, 255]
        } else {
            [220, 40, 100, 255]
        });
    }
    let mut encoded = Cursor::new(Vec::new());
    bar.write_to(&mut encoded, image::ImageFormat::Png).unwrap();
    for (path, bytes) in [
        ("textures/blocks/dirt.png", png([16, 16], [90, 60, 30, 255])),
        ("textures/ui/title.png", title),
        ("textures/ui/loading_bar.png", encoded.into_inner()),
    ] {
        std::fs::write(root.join(path), bytes).unwrap();
    }
    // The texture names and ten-frame flipbook contract follow the pinned
    // progress_screen.json and ui_art_assets_common.json; geometry is a fixture.
    let progress = serde_json::json!({
        "namespace": "progress",
        "bar_animation": {
            "anim_type": "flip_book", "initial_uv": [0, 0],
            "frame_count": 10, "frame_step": 64, "fps": 10
        },
        "overworld_loading_progress_screen": {
            "type": "screen", "controls": [
                {"background": {
                    "type": "image", "texture": "textures/blocks/dirt",
                    "size": ["100%", "100%"], "tiled": true
                }},
                {"content@progress.world_convert_modal_progress_screen_content": {}}
            ]
        },
        "world_convert_modal_progress_screen_content": {
            "type": "panel", "controls": [
                {"title_panel_content@common_art.title_panel_content": {}},
                {"world_modal_progress_panel@progress.world_modal_progress_panel": {}}
            ]
        },
        "world_modal_progress_panel": {
            "type": "panel", "size": [290, 100], "controls": [
                {"dialog": {"type": "image", "texture": "textures/ui/white",
                    "keep_ratio": false, "color": [0.2, 0.2, 0.2]}},
                {"bar": {
                    "type": "image", "texture": "textures/ui/loading_bar",
                    "anchor_from": "center", "anchor_to": "center",
                    "size": [64, 8], "uv_size": [64, 8],
                    "uv": "@progress.bar_animation", "layer": 1
                }}
            ]
        }
    });
    std::fs::write(root.join("ui/progress_screen.json"), progress.to_string()).unwrap();
    std::fs::write(root.join("textures/ui/white.png"), png([1, 1], [255; 4])).unwrap();
    std::fs::write(
        root.join("ui/ui_art_assets_common.json"),
        serde_json::json!({
            "namespace": "common_art",
            "title_image": {"type": "image", "texture": "textures/ui/title", "layer": 1},
            "title_panel_content": {"type": "panel", "controls": [
                {"title@common_art.title_image": {"size": [160, 50]}}
            ]}
        })
        .to_string(),
    )
    .unwrap();
    let compiled = pack_compiler::compile_ui_assets(&root, br#"{"schema":1}"#).unwrap();
    let carrier = Arc::new(RuntimeUiAssets::decode(&compiled.bytes).unwrap());
    std::fs::remove_dir_all(root).unwrap();
    carrier
}

fn settled(
    player: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
) -> render_model::UiRenderInput {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        frame(player, presentation);
        let textures = &presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .textures;
        let atlas = textures.lock();
        let ready = (!atlas.has_image("textures/blocks/dirt")
            || atlas.placement("textures/blocks/dirt").is_some())
            && (!atlas.has_image(crate::ui_runtime::presentation::menu_artwork::TITLE_KEY)
                || atlas
                    .placement(crate::ui_runtime::presentation::menu_artwork::TITLE_KEY)
                    .is_some());
        drop(atlas);
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "carrier images were unresolved"
        );
        test_time::idle();
    }
    presentation.finish_menu_artwork();
    frame(player, presentation)
}

/// The native title's painted bounds, rather than the omitted JSON title control.
pub(super) fn title_bounds(presentation: &UiPresentationRuntime) -> ui::UiRect {
    let icon = presentation
        .form_presentation
        .engine
        .as_deref()
        .unwrap()
        .menu_title(&presentation.menu_artwork.refs)
        .unwrap();
    presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|node| {
            matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == icon.page && *uv == icon.uv)
        })
        .expect("the resolved title is painted in the loading frame")
        .bounds()
}

pub(super) fn center(presentation: &UiPresentationRuntime) -> [u32; 2] {
    let bounds = title_bounds(presentation);
    [
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    ]
    .map(|value| value as u32)
}

fn layout_pixel_scale(presentation: &UiPresentationRuntime) -> f32 {
    crate::ui_runtime::presentation::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        presentation.gui_scale_preference,
    )
    .scale
    .get()
        * crate::ui_runtime::presentation::FONT_DESIGN_PIXEL_TEXELS as f32
}

#[test]
fn texture_only_pack_replacement_updates_loading_background_tile_sizes() {
    let player = player_state::PlayerState::new(1);
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.enable_json_ui(carrier()).unwrap();
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    settled(&player, &mut presentation);
    let catalog = Arc::clone(
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .catalog(),
    );
    let mut dirt = image::RgbaImage::new(32, 16);
    let colors = [[180, 30, 40, 255], [30, 50, 190, 255]];
    for (x, _, pixel) in dirt.enumerate_pixels_mut() {
        *pixel = image::Rgba(colors[usize::from(x >= 16)]);
    }
    let mut encoded = Cursor::new(Vec::new());
    dirt.write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        textures: vec![("textures/blocks/dirt.png".into(), encoded.into_inner())],
        ..Default::default()
    });
    assert!(Arc::ptr_eq(
        &catalog,
        presentation
            .form_presentation
            .engine
            .as_ref()
            .unwrap()
            .catalog(),
    ));
    let pixels = snapshot::rasterize(&settled(&player, &mut presentation));
    let px = layout_pixel_scale(&presentation);
    // The native tiled image uses the replacement's 32-pixel width. Reusing
    // the previous 16-pixel quads compresses the two colors into each old tile.
    for (x, color) in [(12.0, colors[0]), (24.0, colors[1]), (44.0, colors[0])] {
        let at = [(x * px) as u32, px as u32];
        assert_eq!(
            *pixels.get_pixel(at[0], at[1]),
            image::Rgba(snapshot::loading_backdrop_texel(&presentation, color, at)),
            "replacement texture kept the previous background tile period"
        );
    }
}

#[test]
fn loading_frame_draws_brand_backdrop_and_animation_without_source_files() {
    let player = player_state::PlayerState::new(1);
    let mut presentation = UiPresentationRuntime::new(crate::test_support::fixture_font()).unwrap();
    presentation.enable_json_ui(carrier()).unwrap();
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let input = settled(&player, &mut presentation);
    let pixels = snapshot::rasterize(&input);
    assert_eq!(
        *pixels.get_pixel(0, 0),
        image::Rgba(snapshot::loading_backdrop_texel(
            &presentation,
            [90, 60, 30, 255],
            [0, 0]
        ))
    );
    let title = title_bounds(&presentation);
    let width = title.max().x() - title.min().x();
    let height = title.max().y() - title.min().y();
    let logo = pixels.get_pixel(
        (title.min().x() + width * 0.05) as u32,
        (title.min().y() + height * 0.3) as u32,
    );
    assert!(
        logo[0] > 100 && logo[0] > logo[1] && logo[0] > logo[2],
        "{logo:?}"
    );
    let original =
        image::load_from_memory(crate::ui_runtime::presentation::menu_artwork::BUILT_IN_TITLE)
            .unwrap();
    let aspect = original.width() as f32 / original.height() as f32;
    assert!((width / height - aspect).abs() < 0.02);
    assert!(
        (title.min().x() + width * 0.5 - 640.0).abs() < 0.1,
        "loading title stays horizontally centered"
    );
    assert!(
        title.min().y() >= 0.0 && title.max().y() < 720.0,
        "loading title stays inside the viewport"
    );
    input.validate().unwrap();
    let animated = presentation
        .build(
            &player,
            &crate::ui_runtime::UiRuntime::new(1),
            250,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    animated.validate().unwrap();
    assert!(
        input.vertices != animated.vertices,
        "native progress advances independently of carrier flipbooks"
    );

    // A server override remains above both raw carrier art and atlas sprites.
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        textures: vec![
            (
                "textures/ui/title.png".into(),
                png([600, 100], [180, 40, 220, 255]),
            ),
            (
                "textures/blocks/dirt.png".into(),
                png([16, 16], [30, 70, 90, 255]),
            ),
        ],
        ..Default::default()
    });
    let overridden = snapshot::rasterize(&settled(&player, &mut presentation));
    assert_eq!(
        *overridden.get_pixel(0, 0),
        image::Rgba(snapshot::loading_backdrop_texel(
            &presentation,
            [30, 70, 90, 255],
            [0, 0]
        ))
    );
    let [x, y] = center(&presentation);
    assert_eq!(
        *overridden.get_pixel(x, y),
        image::Rgba([180, 40, 220, 255])
    );
}
