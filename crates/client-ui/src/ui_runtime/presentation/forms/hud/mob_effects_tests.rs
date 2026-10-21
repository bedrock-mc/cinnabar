use crate::test_support::mini_carrier;
use crate::ui_runtime::presentation::forms::screen_registry::registry;
use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiVisual};
use ui_engine::atlas::ServerAtlas;
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::hud_renderers::{EffectIcon, HudPaint, HudRenderer, MobEffects};

const BACKGROUND: &str = "textures/ui/hud_mob_effect_background";
const ICONS: [&str; 3] = [
    "textures/ui/speed_effect",
    "textures/ui/haste_effect",
    "textures/ui/strength_effect",
];

fn png(side: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(side, side, image::Rgba([255; 4]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

/// A screen holding one effects control with `control` JSON-UI geometry.
fn engine(control: serde_json::Value) -> FormEngine {
    let mut definition = serde_json::json!({"type": "custom", "renderer": "mob_effects_renderer"});
    definition
        .as_object_mut()
        .unwrap()
        .extend(control.as_object().unwrap().clone());
    // An allow-listed screen name hosts the control.
    let (namespace, name) = json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen
        .as_str()
        .split_once('.')
        .unwrap();
    let screen = serde_json::json!({
        "namespace": namespace,
        (name): {"type": "screen", "controls": [{"effects": definition}]}
    });
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/mob_effects_test.json", &screen.to_string());
    let mut engine = FormEngine::new(
        mini_carrier(),
        catalog,
        2,
        registry(),
        crate::ui_runtime::presentation::forms::texture_sources::downloads(),
        // Layout assertions need all fixture pixels on the first paint.
        ui_engine::sources::ImageWork::inline(),
    );
    let files: Vec<_> = std::iter::once((format!("{BACKGROUND}.png"), png(24)))
        .chain(ICONS.iter().map(|icon| (format!("{icon}.png"), png(18))))
        .collect();
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    engine
}

/// One drawn effect's background and icon bounds, in physical px.
type EffectBounds = ([f32; 4], [f32; 4]);

/// Bounds per drawn effect and the GUI scale.
fn draw(engine: &FormEngine, effects: usize, size: [u32; 2]) -> (Vec<EffectBounds>, f32) {
    let metrics = TextMetrics::for_viewport(size, DpiScale::new(1.0).unwrap(), None);
    let hud = HudPaint {
        effects: MobEffects {
            icons: ICONS[..effects]
                .iter()
                .map(|icon| EffectIcon {
                    background: BACKGROUND,
                    icon,
                    alpha: 255,
                })
                .collect(),
            status_rows: 1,
        },
        ..Default::default()
    };
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes: Vec<UiNode> = Vec::new();
    let mut next = 1;
    engine
        .render_screen(
            json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen.as_str(),
            &DataSource::default(),
            &Context::default(),
            &ViewState::default(),
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: size.map(|edge| edge as f32),
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [&mut HudRenderer {
                    hud: &hud,
                    hunger_update: None,
                    bounds: None,
                }],
            },
        )
        .unwrap()
        .expect("resolved effects screen");
    let bounds: Vec<[f32; 4]> = nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
        .map(|node| {
            let rect = node.bounds();
            [
                rect.min().x(),
                rect.min().y(),
                rect.max().x(),
                rect.max().y(),
            ]
        })
        .collect();
    let pairs = bounds
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[background, icon]| (background, icon))
        .collect();
    (pairs, metrics.gui_scale)
}

fn vanilla_control() -> serde_json::Value {
    serde_json::json!({"size": ["100%", "75%"], "offset": [0, 24],
        "anchor_from": "center", "anchor_to": "center"})
}

/// Vanilla stacks effects down one column at the right edge, below the top
/// band, instead of along a row from the control's corner.
#[test]
fn effects_stack_down_the_right_edge_below_the_top_band() {
    let size = [1280, 720];
    let (drawn, scale) = draw(&engine(vanilla_control()), 3, size);
    assert_eq!(drawn.len(), 3, "one background and icon per effect");
    let side = 18.0 * scale;
    let band = (18.0 * scale + 4.0 + 20.0 * scale).trunc();
    for (index, (background, icon)) in drawn.iter().enumerate() {
        assert_eq!(icon[2] - icon[0], side, "icon side is 18 GUI px");
        assert_eq!(icon[2], size[0] as f32 - 16.0, "16 px from the right edge");
        let top = band + side + 4.0 + index as f32 * (side + 4.0);
        assert_eq!(icon[1], top, "effect {index} sits one step below the last");
        assert_eq!(
            *background,
            [icon[0] - 2.0, icon[1] - 2.0, icon[2] + 2.0, icon[3] + 2.0],
            "background frames the icon by 2 px"
        );
    }
}

/// A full column wraps leftward; a control too short for one icon draws nothing.
#[test]
fn full_columns_wrap_left_and_short_controls_draw_nothing() {
    let size = [1280, 720];
    let scale = TextMetrics::for_viewport(size, DpiScale::new(1.0).unwrap(), None).gui_scale;
    let side = 18.0 * scale;
    let step = side + 4.0;
    // Room for exactly two icons: three sides of margin plus two steps.
    let height = (3.0 * side + 12.0 + 2.0 * step) / scale;
    let (drawn, _) = draw(
        &engine(serde_json::json!({"size": ["100%", height]})),
        3,
        size,
    );
    let icons: Vec<_> = drawn.iter().map(|(_, icon)| *icon).collect();
    assert_eq!(icons.len(), 3);
    assert_eq!(icons[1][0], icons[0][0]);
    assert_eq!(icons[1][1], icons[0][1] + step);
    assert_eq!(
        icons[2][0],
        icons[0][0] - step,
        "third effect starts a column"
    );
    assert_eq!(icons[2][1], icons[0][1]);

    let (drawn, _) = draw(
        &engine(serde_json::json!({"size": ["100%", 3.0 * side / scale]})),
        3,
        size,
    );
    assert!(drawn.is_empty(), "{drawn:?}");
}
