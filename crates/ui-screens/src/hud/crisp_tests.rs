use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, UiFile, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiBlendMode, UiNode, UiVisual};
use ui_engine::{
    atlas::ServerAtlas,
    engine::{FormEngine, ScreenArt, screen::PaintFrame},
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

use super::{HudInput, HudPaint, HudScreens, ScreenRegistry};
use crate::hud_renderers::{CROSSHAIR_SIDE, CROSSHAIR_TEXTURE, SheetSprite};

struct NoDownloads;

impl RemoteSource for NoDownloads {
    /// Keeps the fixture independent of downloads.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Builds synthetic HUD screens and an empty carrier without installed game assets.
fn engine() -> FormEngine {
    let mut files = [
        ("ui/_global_variables.json", "{}"),
        (
            "ui/_ui_defs.json",
            r#"{"ui_defs":["ui/hud.json","ui/cursor.json"]}"#,
        ),
        (
            "ui/hud.json",
            r#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[
                {"image":{"type":"image","texture":"textures/ui/cross_hair",
                    "bilinear":true,"grayscale":true,"size":[16,16],
                    "anchor_from":"center","anchor_to":"center"}}
            ]}}"#,
        ),
        (
            "ui/cursor.json",
            r#"{"namespace":"hud_crosshair","hud_crosshair_screen":{"type":"screen","controls":[
                {"cursor":{"type":"custom","renderer":"cursor_renderer","size":[16,16],
                    "anchor_from":"center","anchor_to":"center"}}
            ]}}"#,
        ),
    ]
    .map(|(path, text)| UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let page = UiAtlasPage {
        width: 4,
        height: 4,
        rgba8: vec![255; 64].into(),
    };
    let bytes = encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    let carrier = Arc::new(RuntimeUiAssets::decode(&bytes).unwrap());
    let catalog = Catalog::from_files(
        files
            .iter()
            .map(|file| (file.path.as_ref(), file.bytes.as_ref())),
    )
    .unwrap();
    let registry = Box::leak(Box::new(ScreenRegistry::new(Context::desktop())));
    registry.screen(json_ui::vanilla::Reference::HudHudScreen.as_str());
    FormEngine::new(
        carrier,
        catalog,
        1,
        registry,
        Arc::new(NoDownloads),
        ImageWork::inline(),
    )
}

/// Alternates opaque and transparent texels so any resampling changes the fixture.
fn texture(side: u32) -> (Vec<u8>, Vec<u8>) {
    let image = image::RgbaImage::from_fn(side, side, |x, y| {
        image::Rgba(if (x + y) % 2 == 0 { [255; 4] } else { [0; 4] })
    });
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    (png, image.into_raw())
}

/// Replaces the whole atlas, as a transfer or texture-only pack reload does.
fn replace_texture(
    engine: &mut FormEngine,
    side: u32,
) -> (Vec<u8>, Option<(u16, render_model::UiTexturePage)>) {
    let (png, pixels) = texture(side);
    engine.set_server_atlas(
        ServerAtlas::new(&[(format!("{CROSSHAIR_TEXTURE}.png"), png)], None, 1),
        server_page(),
    );
    engine.textures.atlas_mut().require([CROSSHAIR_TEXTURE]);
    (pixels, publish_artwork(engine))
}

/// Publishes admitted oversized bytes through the artwork handoff that upgrades atlas previews.
fn publish_artwork(engine: &mut FormEngine) -> Option<(u16, render_model::UiTexturePage)> {
    let oversized = engine.textures.oversized();
    let (key, bytes) = oversized.first()?;
    assert_eq!(key, CROSSHAIR_TEXTURE);
    let image = image::load_from_memory(bytes).unwrap().into_rgba8();
    assert!(
        image.width() <= ui_engine::pages::WHOLE_ART_PAGE
            && image.height() <= ui_engine::pages::WHOLE_ART_PAGE,
        "larger sources must use the original native-sheet path"
    );
    let gutter = ui_engine::pages::ART_PAGE_GUTTER;
    let uv = [
        gutter,
        gutter,
        gutter + image.width(),
        gutter + image.height(),
    ]
    .map(|edge| u16::try_from(edge).unwrap());
    let mut page = image::RgbaImage::new(
        render_model::UI_ART_PAGE_SIDE,
        render_model::UI_ART_PAGE_SIDE,
    );
    image::imageops::overlay(&mut page, &image, i64::from(gutter), i64::from(gutter));
    let index = u16::try_from(
        usize::from(engine.textures.server_page) - ui_engine::pages::SERVER_UI_PAGE
            + render_model::MAX_UI_DYNAMIC_PAGES,
    )
    .unwrap();
    engine
        .textures
        .set_full_res(std::collections::HashMap::from([(
            key.clone(),
            ui::IconRef {
                page: index,
                uv,
                glint: false,
            },
        )]));
    Some((
        index,
        render_model::UiTexturePage::owned([page.width(), page.height()], page.into_raw().into())
            .unwrap(),
    ))
}

/// Leaves room before the server atlas for all of the presentation's reserved pages.
fn server_page() -> u16 {
    u16::try_from(ui_engine::pages::SERVER_UI_PAGE + 1).unwrap()
}

/// Paints through the production HUD entry point or the same image screen as a menu.
fn paint(
    engine: &FormEngine,
    screens: Option<&mut HudScreens>,
    crosshair: bool,
    blend: UiBlendMode,
    physical: [u32; 2],
    dpi: f32,
    scale: u8,
) -> Vec<UiNode> {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let metrics = TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), Some(scale));
    let role = assets::HudTextureRole::Crosshair;
    let [width, height] = role.expected_size();
    let hud = HudPaint {
        crosshair: Some(SheetSprite {
            page: 0,
            uv: [0, 0, width as u16, height as u16],
        }),
        crosshair_blend: blend,
        ..Default::default()
    };
    let context = Context::desktop();
    let mut nodes = Vec::new();
    let mut next = 1;
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: physical.map(|side| side as f32 / dpi),
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    match screens {
        Some(screens) => {
            screens
                .paint(
                    engine,
                    HudInput {
                        capture_keep_out: false,
                        crosshair,
                        catalog: engine.catalog(),
                        context: &context,
                        data: Arc::new(DataSource::default()),
                        paint: &hud,
                        icons: &[],
                    },
                    &mut frame,
                )
                .unwrap();
        }
        None => {
            engine
                .render_screen(
                    json_ui::vanilla::Reference::HudHudScreen.as_str(),
                    &DataSource::default(),
                    &context,
                    &json_ui::ViewState::default(),
                    ScreenArt::default(),
                    &mut frame,
                )
                .unwrap()
                .unwrap();
        }
    }
    nodes
}

#[test]
fn hud_pack_swaps_keep_nearest_sampling_and_source_texels() {
    let mut engine = engine();
    let mut screens = HudScreens::default();
    for side in [16, 128, 512, ui_engine::pages::WHOLE_ART_PAGE + 1, 16] {
        let (pixels, artwork) = replace_texture(&mut engine, side);
        for crosshair in [false, true] {
            let nodes = paint(
                &engine,
                Some(&mut screens),
                crosshair,
                UiBlendMode::Invert,
                [1367, 769],
                1.5,
                3,
            );
            let node = nodes
                .iter()
                .find(|node| !matches!(node.visual(), UiVisual::None))
                .unwrap();
            let (page, uv) = match node.visual() {
                UiVisual::StyledSprite {
                    texture_page,
                    uv,
                    style,
                    ..
                } => {
                    assert_eq!(*style & ui::UI_STYLE_BILINEAR, 0);
                    assert_ne!(*style & ui::UI_STYLE_GRAYSCALE, 0);
                    (*texture_page, *uv)
                }
                UiVisual::InvertedSprite { texture_page, uv } => (*texture_page, uv.map(f32::from)),
                other => panic!("expected a textured HUD sprite, got {other:?}"),
            };
            assert_eq!([uv[2] - uv[0], uv[3] - uv[1]], [side as f32; 2]);
            let atlas = engine.textures.lock();
            let image = if let Some((index, image)) = &artwork {
                assert_eq!(page, *index, "original artwork replaces its atlas preview");
                image
            } else if side > ui_engine::pages::WHOLE_ART_PAGE {
                &atlas.native_pages()[0]
            } else {
                &atlas.images()[usize::from(page - engine.textures.server_page)]
            };
            let stride = image.dimensions()[0] as usize * 4;
            for row in 0..side as usize {
                let offset = (uv[1] as usize + row) * stride + uv[0] as usize * 4;
                let source = row * side as usize * 4;
                assert_eq!(
                    &image.pixels()[offset..offset + side as usize * 4],
                    &pixels[source..source + side as usize * 4]
                );
            }
        }
    }
    let menu = paint(
        &engine,
        None,
        false,
        UiBlendMode::Alpha,
        [1367, 769],
        1.5,
        3,
    );
    assert!(menu.iter().any(|node| matches!(node.visual(), UiVisual::StyledSprite { style, .. } if *style & ui::UI_STYLE_BILINEAR != 0)));
}

#[test]
fn native_crosshair_stays_on_physical_pixels_after_pack_swaps() {
    let mut engine = engine();
    let mut screens = HudScreens::default();
    for side in [None, Some(128), Some(16), None] {
        if let Some(side) = side {
            let _ = replace_texture(&mut engine, side);
        } else {
            engine.set_server_atlas(ServerAtlas::new(&[], None, 1), server_page());
        }
        for blend in [UiBlendMode::Invert, UiBlendMode::Alpha] {
            for (dpi, scale) in [(1.0, 1), (1.0, 3), (1.5, 3), (2.0, 3)] {
                let physical = [1367, 769];
                let nodes = paint(
                    &engine,
                    Some(&mut screens),
                    true,
                    blend,
                    physical,
                    dpi,
                    scale,
                );
                let node = nodes
                    .iter()
                    .find(|node| {
                        matches!(
                            node.visual(),
                            UiVisual::Sprite { .. } | UiVisual::InvertedSprite { .. }
                        )
                    })
                    .unwrap();
                let bounds = node.bounds();
                assert!(matches!(
                    (blend, node.visual()),
                    (UiBlendMode::Invert, UiVisual::InvertedSprite { .. })
                        | (UiBlendMode::Alpha, UiVisual::Sprite { .. })
                ));
                let edges = [
                    bounds.min().x(),
                    bounds.min().y(),
                    bounds.max().x(),
                    bounds.max().y(),
                ]
                .map(|edge| edge * dpi);
                for edge in edges {
                    assert!((edge - edge.round()).abs() < 1e-3, "{edges:?}");
                }
                let gui_side = side.map_or(
                    assets::HudTextureRole::Crosshair.expected_size()[0] as f32,
                    |_| CROSSHAIR_SIDE,
                );
                assert!((edges[2] - edges[0] - gui_side * f32::from(scale)).abs() < 1e-3);
                for axis in 0..2 {
                    let center = (edges[axis] + edges[axis + 2]) * 0.5;
                    assert!((center - physical[axis] as f32 * 0.5).abs() <= 1.0 + 1e-3);
                }
            }
        }
    }
}
