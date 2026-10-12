//! Server and client toasts through vanilla `toast_screen.toast_screen`: the
//! showing toast is the `toast_factory`'s `popup`, as 26.30 creates one for a
//! ToastRequest. The popup's offset animation (from above the top edge down
//! 32 px and back) is evaluated here and handed over as its offset.

use std::sync::Arc;

use json_ui::{DataSource, FactoryItem, Scalar};
use serde_json::{Value, json};
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiRect};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};
use ui_engine::frame::EngineFrame;

use crate::regions::window_rect;
use crate::text::bounded_visible_text;

/// Where the popup's `button.menu_select` routes.
const TOAST_PRESS: &str = "button.toast_interaction";

pub const TOAST_SCREEN: &str = "toast_screen.toast_screen";
/// How far the popup slides down from above the top edge.
const TOAST_DISTANCE: f64 = 32.0;

/// Paints the showing `toast` over its cached `screen`.
pub fn paint(
    screen: &mut CachedScreen,
    engine: &FormEngine,
    toast: &ui::ShownToast<'_>,
    frame: &mut PaintFrame<'_>,
) -> Result<Option<EngineFrame>, PaintError> {
    let data = toast_data(toast);
    // Vanilla toast screen variables.
    let context = engine
        .context()
        .clone()
        .with_var("popup_size", json!(["100% - 50px", 32]));
    let catalog = Arc::clone(engine.catalog());
    let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let language = frame.language;
    engine.draw(
        ScreenArt {
            ..ScreenArt::default()
        },
        frame,
        |env, root| {
            screen.render_with(
                TOAST_SCREEN,
                &catalog,
                &context,
                data,
                (root, px, language),
                env,
                &json_ui::ViewState::default(),
            )
        },
    )
}

/// Where the painted popup takes presses, in window-logical px.
pub fn press_bounds(frame: &EngineFrame, origin: [f32; 2]) -> Option<UiRect> {
    frame
        .hits
        .iter()
        .filter(|region| region.enabled && region.pressed.as_deref() == Some(TOAST_PRESS))
        .find_map(|region| window_rect(region, frame.scale, origin))
}

/// What the toast controller binds for the showing `toast`.
fn toast_data(toast: &ui::ShownToast<'_>) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    let title = bounded_visible_text(toast.title).to_owned();
    let subtitle = bounded_visible_text(toast.message).to_owned();
    data.set_global(
        "#toast_subtitle_visible",
        Scalar::Bool(!subtitle.is_empty()),
    );
    data.set_global("#toast_title", Scalar::Text(title));
    data.set_global("#toast_subtitle", Scalar::Text(subtitle));
    data.set_global("#toast_icon_section_content", Scalar::Num(0.0));
    let offset = TOAST_DISTANCE * f64::from(toast.slide);
    data.set_factory(
        "toast_factory",
        vec![
            FactoryItem::new("popup", toast.started_millis as f64 / 1_000.0)
                .named("popup")
                .var("toast_offset", json!([0.0, offset]))
                .var("offset_anims", Value::Array(Vec::new())),
        ],
    );
    data
}
