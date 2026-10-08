use super::*;
use crate::test_support::{fixture_font, mini_carrier};
use crate::ui_runtime::presentation::forms::{
    engine::{EngineInputs, EngineOutput, FormEngine, ScreenArt},
    server_pack::ServerAtlas,
};
use crate::ui_runtime::presentation::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics};
use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode};

const FALLBACK: SheetSprite = SheetSprite {
    page: 1,
    uv: {
        let [width, height] = assets::HudTextureRole::Crosshair.expected_size();
        [0, 0, width as u16, height as u16]
    },
};

/// Builds a cursor screen with optional server texture overrides.
fn engine(files: &[(String, Vec<u8>)]) -> FormEngine {
    let mut catalog = Catalog::default();
    let (namespace, name) = json_ui::CROSSHAIR_SCREEN.split_once('.').unwrap();
    let definition = serde_json::json!({
        "namespace": namespace,
        (name): {"type": "screen", "controls": [{
            "cursor": {"type": "custom", "renderer": "cursor_renderer",
                "size": [CROSSHAIR_SIDE, CROSSHAIR_SIDE]}
        }]}
    });
    catalog.overlay_text("ui/crosshair_test.json", &definition.to_string());
    let mut engine = FormEngine::new(mini_carrier(), catalog, 2);
    engine.set_server_atlas(ServerAtlas::new(files, None, 1), 3);
    engine
}

/// Emits cursor geometry for one visibility and blending combination.
fn draw(engine: &FormEngine, visible: bool, blend: ui::UiBlendMode) -> (Vec<UiNode>, f32) {
    draw_custom(engine, visible, blend, None)
}

fn draw_custom(
    engine: &FormEngine,
    visible: bool,
    blend: ui::UiBlendMode,
    custom: Option<ui::mod_hud::Crosshair>,
) -> (Vec<UiNode>, f32) {
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let hud = HudPaint {
        crosshair: visible.then_some(FALLBACK),
        crosshair_blend: blend,
        custom_crosshair: custom,
        ..Default::default()
    };
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .render_screen(
            json_ui::CROSSHAIR_SCREEN,
            &DataSource::default(),
            &Context::default(),
            &ViewState::default(),
            ScreenArt {
                hud: Some(&hud),
                ..Default::default()
            },
            EngineInputs {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0],
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
            },
        )
        .unwrap()
        .expect("resolved cursor screen");
    (nodes, px)
}

/// Encodes a solid pack texture to check atlas selection.
fn texture(color: [u8; 4]) -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(32, 32, image::Rgba(color))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    png
}

#[test]
fn crosshair_uses_the_pack_texture_without_a_ui_override() {
    let color = [22, 33, 44, 255];
    let mut engine = engine(&[(format!("{CROSSHAIR_TEXTURE}.png"), texture(color))]);
    let (nodes, px) = draw(&engine, true, ui::UiBlendMode::Invert);
    let node = nodes
        .iter()
        .find(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
        .expect("inverting crosshair");
    let UiVisual::InvertedSprite { texture_page, uv } = node.visual() else {
        unreachable!()
    };
    assert_eq!(
        *texture_page, 3,
        "pack texture replaces the pinned HUD sprite"
    );
    assert_eq!(
        [uv[2] - uv[0], uv[3] - uv[1]],
        [32, 32],
        "full texture, not an icons-sheet crop"
    );
    assert_eq!(node.bounds().width(), CROSSHAIR_SIDE * px);
    assert_eq!(node.bounds().height(), CROSSHAIR_SIDE * px);
    let pages = engine
        .take_server_pages()
        .expect("crosshair atlas uploaded");
    let width = pages[0].dimensions()[0] as usize;
    let offset = (usize::from(uv[1]) * width + usize::from(uv[0])) * 4;
    assert_eq!(&pages[0].pixels()[offset..offset + 4], &color);
}

#[test]
fn resource_pack_icons_sheet_replaces_the_crosshair() {
    let role = assets::HudTextureRole::Crosshair;
    for scale in [1, 2] {
        let [native_width, native_height] = assets::HUD_ICONS_SHEET_SIZE;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(
            native_width * scale,
            native_height * scale,
            image::Rgba([22, 33, 44, 255]),
        )
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
        let mut engine = engine(&[(role.source_path().into(), png)]);
        if scale > 1 {
            // Oversized artwork upgrades the bounded atlas preview to a full-size page.
            engine
                .textures
                .set_full_res(std::collections::HashMap::from([(
                    "textures/gui/icons".into(),
                    crate::ui_runtime::presentation::IconRef {
                        page: 4,
                        uv: [
                            0,
                            0,
                            (native_width * scale) as u16,
                            (native_height * scale) as u16,
                        ],
                        glint: false,
                    },
                )]));
        }
        let (nodes, px) = draw(&engine, true, ui::UiBlendMode::Invert);
        let node = nodes
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
            .expect("crosshair");
        let UiVisual::InvertedSprite { texture_page, uv } = node.visual() else {
            unreachable!()
        };
        assert_eq!(
            *texture_page,
            if scale == 1 { 3 } else { 4 },
            "pack icons sheet replaces the carrier crop"
        );
        let [_, _, width, height] = role.source_crop().unwrap();
        assert_eq!(
            [uv[2] - uv[0], uv[3] - uv[1]],
            [(width * scale) as u16, (height * scale) as u16]
        );
        assert_eq!(node.bounds().width(), width as f32 * px);
    }
}

#[test]
fn crosshair_missing_or_invalid_texture_keeps_the_builtin_hud_sprite() {
    for files in [
        Vec::new(),
        vec![(format!("{CROSSHAIR_TEXTURE}.png"), vec![0])],
        vec![(
            assets::HudTextureRole::Crosshair.source_path().into(),
            vec![0],
        )],
    ] {
        let engine = engine(&files);
        let (nodes, _) = draw(&engine, true, ui::UiBlendMode::Invert);
        assert!(nodes.iter().any(|node| matches!(node.visual(),
            UiVisual::InvertedSprite { texture_page, uv } if *texture_page == FALLBACK.page && *uv == FALLBACK.uv
        )));
    }
}

#[test]
fn crosshair_pack_texture_does_not_bypass_visibility() {
    let engine = engine(&[(format!("{CROSSHAIR_TEXTURE}.png"), texture([255; 4]))]);
    let (nodes, _) = draw(&engine, false, ui::UiBlendMode::Invert);
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
    );
}

#[test]
fn crosshair_color_toggle_preserves_pack_art_and_geometry() {
    for files in [
        Vec::new(),
        vec![(
            format!("{CROSSHAIR_TEXTURE}.png"),
            texture([22, 33, 44, 255]),
        )],
    ] {
        let engine = engine(&files);
        let (inverted, _) = draw(&engine, true, ui::UiBlendMode::Invert);
        let (normal, _) = draw(&engine, true, ui::UiBlendMode::Alpha);
        let inverted = inverted
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
            .unwrap();
        let normal = normal
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
            .unwrap();
        assert_eq!(inverted.bounds(), normal.bounds());
        let UiVisual::InvertedSprite { texture_page, uv } = inverted.visual() else {
            unreachable!()
        };
        assert_eq!(
            normal.visual(),
            &UiVisual::Sprite {
                texture_page: *texture_page,
                uv: *uv,
                color: [255; 4]
            }
        );
        let (hidden, _) = draw(&engine, false, ui::UiBlendMode::Alpha);
        assert!(!hidden.iter().any(|node| matches!(
            node.visual(),
            UiVisual::Sprite { .. } | UiVisual::InvertedSprite { .. }
        )));
    }
}

#[test]
fn cosmetic_crosshair_replaces_only_visible_cursor_art_and_restores_on_clear() {
    let engine = engine(&[]);
    let vanilla = draw(&engine, true, ui::UiBlendMode::Invert).0;
    for shape in [
        ui::mod_hud::CrosshairShape::Cross,
        ui::mod_hud::CrosshairShape::Dot,
        ui::mod_hud::CrosshairShape::Circle,
    ] {
        let spec = ui::mod_hud::Crosshair {
            shape,
            ..Default::default()
        };
        let (custom, _) = draw_custom(&engine, true, ui::UiBlendMode::Invert, Some(spec.clone()));
        let painted: Vec<_> = custom
            .iter()
            .filter(|node| !matches!(node.visual(), UiVisual::None))
            .collect();
        assert_eq!(painted.len(), 1, "custom cursor paints one visual");
        assert!(matches!(painted[0].visual(), ui::UiVisual::Mesh(_)));
        let bounds = painted[0].bounds();
        assert_eq!((bounds.min().x() + bounds.max().x()) * 0.5, 640.);
        assert_eq!((bounds.min().y() + bounds.max().y()) * 0.5, 360.);
        assert!(
            draw_custom(&engine, false, ui::UiBlendMode::Invert, Some(spec))
                .0
                .iter()
                .all(|node| matches!(node.visual(), UiVisual::None)),
            "a hidden native cursor also hides the custom cursor"
        );
        assert!(
            draw_custom(&engine, true, ui::UiBlendMode::Invert, None).0 == vanilla,
            "clearing the cosmetic cursor restores native geometry"
        );
    }
}
