//! The menus' code-drawn controls: the title splash and the player's gamerpic, drawn as
//! vanilla's renderers draw them.

use std::{
    f64::consts::PI,
    sync::{Arc, OnceLock},
};

use assets::RuntimeUiAssets;
use serde_json::Value;
use ui::{IconRef, UiVisual};

use ui_engine::engine::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
    text_paint::{UNWRAPPED_LOGICAL, scaled_request, width_64},
};

/// Title panels shaped to Cinnabar's logo (1022x282) rather than the pack's
/// title, so it draws unstretched and the splash meets its right edge.
const TITLE_PANEL_OVERLAY: &[u8] = br#"{
  "namespace": "common_art",
  "title_panel_win10": { "size": ["55%", "27.59%x"] },
  "title_panel_osx": { "size": ["55%", "27.59%x"] },
  "title_panel_pocket": { "size": ["55%", "27.59%x"] },
  "pause_logo_panel": { "variables": [
    { "requires": "($win10_edition or $osx_edition or $pocket_edition or $console_edition)",
      "$title_panel": "common_art.title_image",
      "$logo_max_size": [275, "27.59%x"], "$logo_size": ["90%", "27.59%x"] }
  ] }
}"#;

/// Drops the "©Mojang AB" footer from the start and play screens; Cinnabar is not a Mojang product.
const NO_COPYRIGHT_OVERLAYS: [(&str, &[u8]); 2] = [
    (
        "ui/start_screen.json",
        br#"{ "namespace": "start", "copyright": { "ignored": true } }"#,
    ),
    (
        "ui/play_screen.json",
        br#"{ "namespace": "play", "copyright": { "ignored": true } }"#,
    ),
];

/// Tilt of the splash: 20 degrees, rising to the right.
const SPLASH_ANGLE: f32 = -20.0 * std::f32::consts::PI / 180.0;
const SPLASH_COLOR: [u8; 4] = [255, 255, 0, 255];
/// A splash longer than this many characters wraps onto two lines.
const SPLASH_LINE_CHARS: usize = 20;
/// Font line height the renderer's geometry is written against, in GUI px.
const LINE_HEIGHT: f32 = 8.0;

/// Shapes title panels to Cinnabar's logo and drops the Mojang footer, beneath server packs.
pub fn install_brand_overlays(catalog: &mut json_ui::Catalog) {
    catalog.apply_pack(
        [("ui/ui_art_assets_common.json", TITLE_PANEL_OVERLAY)]
            .into_iter()
            .chain(NO_COPYRIGHT_OVERLAYS),
    );
}

/// The splash for this launch: the line of the pack's `splashes.json` that `seed` picks,
/// else `menu.beta` as the preview client (whose pack has none) shows.
pub fn pick_splash(
    assets: &RuntimeUiAssets,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
    seed: usize,
) -> Option<String> {
    let lines: Vec<String> = assets
        .ui_file("splashes.json")
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|json| {
            let lines = json.get("splashes")?.as_array()?;
            Some(
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
            )
        })
        .unwrap_or_default();
    if lines.is_empty() {
        return translate("menu.beta").map(|text| text.to_string());
    }
    let pick = seed % lines.len();
    lines.into_iter().nth(pick)
}

/// `text` split at the last space within `limit` characters, once.
fn split_sentence(text: &str, limit: usize) -> (&str, Option<&str>) {
    if text.chars().count() <= limit {
        return (text, None);
    }
    let cut = text
        .char_indices()
        .take(limit + 1)
        .filter(|(_, character)| *character == ' ')
        .map(|(index, _)| index)
        .last();
    match cut {
        Some(index) => (&text[..index], Some(&text[index + 1..])),
        None => (text, None),
    }
}

/// The title splash and the player's own gamerpic on menu screens.
pub struct MenuRenderer<'a> {
    pub splash: Option<&'a str>,
    pub portrait: Option<IconRef>,
    /// The menu clock, in seconds.
    pub now: f64,
    /// The menu clock when the splash first drew; its pulse runs from there.
    pub splash_epoch: &'a OnceLock<f64>,
}

impl CustomRenderer for MenuRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        match node.renderer {
            "splash_text_renderer" => {
                if let Some(text) = self.splash {
                    let seconds = self.now - *self.splash_epoch.get_or_init(|| self.now);
                    splash(canvas, text, node, seconds);
                }
                Ok(true)
            }
            // A friend row names its own gamerpic, which the engine draws.
            "profile_image_renderer"
                if !matches!(
                    node.data.get("#profile_image_options"),
                    Some(Value::String(_))
                ) =>
            {
                if let Some(portrait) = self.portrait {
                    let visual = UiVisual::Sprite {
                        texture_page: portrait.page,
                        uv: portrait.uv,
                        color: node.alpha([255; 4]),
                    };
                    canvas.push(visual, node.dest)?;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

/// Yellow text tilted up to the right around the control's position,
/// pulsing up to 4% larger and shrunk to stay below the top of the screen.
fn splash(canvas: &mut Canvas<'_>, text: &str, node: &CustomDraw<'_>, seconds: f64) -> Option<()> {
    let dest = node.dest;
    let (px, metrics, font, screen) = (
        canvas.px(),
        canvas.metrics(),
        canvas.font(),
        canvas.screen(),
    );
    let mut scale = 1.0 + 0.04 * (seconds * PI * 2.3).sin().powi(4) as f32;
    let (first, second) = split_sentence(text, SPLASH_LINE_CHARS);
    let mut width = |line: &str| {
        let request = metrics.request(line, width_64(UNWRAPPED_LOGICAL), font);
        canvas
            .layouts()
            .layout(request)
            .map(|layout| layout.size_64()[0] as f32 / 64.0 / px)
            .unwrap_or(0.0)
    };
    let widest = width(first).max(second.map_or(0.0, &mut width));
    let (sin, cos) = (-SPLASH_ANGLE).sin_cos();
    let lines = if second.is_some() { 2.5 } else { 1.0 };
    let origin = [dest[0] / px, dest[1] / px];
    let room = (origin[1] / sin - lines * LINE_HEIGHT * cos * 0.5)
        .min(((screen[2] / px).floor() * 0.95).floor());
    if room.floor() < widest && widest > 0.0 {
        scale *= room.floor().max(0.0) / widest;
    }
    let step = LINE_HEIGHT * 0.6 * scale;
    let offset = if second.is_some() {
        [step * sin, step * cos]
    } else {
        [0.0, 0.0]
    };
    let placed = [(first, -1.0), (second.unwrap_or_default(), 1.0)];
    for (line, sign) in placed.into_iter().filter(|(line, _)| !line.is_empty()) {
        let centre = [
            (origin[0] + sign * offset[0]) * px,
            (origin[1] + sign * offset[1] + LINE_HEIGHT * 0.5) * px,
        ];
        let request = scaled_request(&metrics, line, width_64(UNWRAPPED_LOGICAL), font, scale);
        let Ok(layout) = canvas.layouts().layout(request) else {
            continue;
        };
        let [w, h] = layout.size_64().map(|size| size as f32 / 64.0);
        let visual = UiVisual::RotatedText {
            layout,
            color: node.alpha(SPLASH_COLOR),
            shadow: metrics.shadow(),
            angle_radians: SPLASH_ANGLE,
        };
        let bounds = [
            centre[0] - w * 0.5,
            centre[1] - h * 0.5,
            centre[0] + w * 0.5,
            centre[1] + h * 0.5,
        ];
        // The tilt reaches past the control, so it draws unclipped.
        canvas.group(screen).ok()?;
        canvas.push(visual, bounds).ok()?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::split_sentence;

    // The vanilla footer shape (a panel embedding `start.copyright`) resolves without it.
    #[test]
    fn the_mojang_copyright_footer_is_dropped() {
        let screen = br#"{
          "namespace": "start",
          "copyright": { "type": "label", "text": "menu.copyright" },
          "text_panel": { "type": "panel", "controls": [ { "copyright@start.copyright": {} } ] }
        }"#;
        let mut catalog = json_ui::Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            (
                "ui/_ui_defs.json",
                br#"{"ui_defs":["ui/start_screen.json"]}"#.as_slice(),
            ),
            ("ui/start_screen.json", screen.as_slice()),
        ])
        .unwrap();
        let context = json_ui::Context::default();
        let before = json_ui::resolve(&catalog, "start.text_panel", &context);
        assert_eq!(before.control.unwrap().children.len(), 1);
        catalog.apply_pack(super::NO_COPYRIGHT_OVERLAYS);
        let after = json_ui::resolve(&catalog, "start.text_panel", &context);
        assert!(after.control.unwrap().children.is_empty());
    }

    // The overlay's title aspect matches the logo it frames.
    #[test]
    fn the_title_panel_matches_the_logo_aspect() {
        let logo =
            image::load_from_memory(include_bytes!("../../../assets/branding/title.png")).unwrap();
        let percent = f64::from(logo.height()) / f64::from(logo.width()) * 100.0;
        let overlay = std::str::from_utf8(super::TITLE_PANEL_OVERLAY).unwrap();
        assert!(
            overlay.contains(&format!("\"{percent:.2}%x\"")),
            "{percent:.2}"
        );
    }

    #[test]
    fn long_splashes_wrap_once_at_a_space() {
        assert_eq!(
            split_sentence("Haley loves Elan!", 20),
            ("Haley loves Elan!", None)
        );
        assert_eq!(
            split_sentence("Now with more than twenty letters", 20),
            ("Now with more than", Some("twenty letters"))
        );
        assert_eq!(split_sentence("abcdefghijklmnopqrstuvwxyz", 20).1, None);
    }
}
