//! The menus' code-drawn controls: the title splash, the player preview and its
//! name tag, drawn as vanilla's renderers draw them.

use std::{
    collections::BTreeMap,
    f64::consts::PI,
    sync::{Arc, OnceLock},
    time::Instant,
};

use assets::RuntimeUiAssets;
use serde_json::Value;
use ui::{TextShadow, UiVisual};

use super::{
    Painter,
    text_paint::{UNWRAPPED_LOGICAL, scaled_request, width_64},
};

/// Title panels shaped to Cinnabar's logo (1022x282) rather than the pack's
/// title, so it draws unstretched and the splash meets its right edge.
pub(super) const TITLE_PANEL_OVERLAY: &[u8] = br#"{
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
pub(super) const NO_COPYRIGHT_OVERLAYS: [(&str, &[u8]); 2] = [
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
/// Name tag backing is black at alpha 0.25.
const NAME_TAG_BACKGROUND: [u8; 4] = [0, 0, 0, 64];

/// The splash for this launch: a random line of the pack's `splashes.json`,
/// else `menu.beta` as the preview client (whose pack has none) shows.
pub(super) fn pick_splash(
    assets: &RuntimeUiAssets,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
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
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
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

impl Painter<'_> {
    /// Messaging art, drawn as its first frame.
    pub(super) fn animated_gif(
        &mut self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &impl Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let path = data.get("#gif_path")?.as_str()?;
        let image = self.art.images?.get(path)?;
        let opacity = data
            .get("#alpha")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let tint = alpha([255, 255, 255, (255.0 * opacity) as u8]);
        Some((
            UiVisual::Sprite {
                texture_page: image.page,
                uv: image.uv,
                color: tint,
            },
            dest,
        ))
    }

    /// The preview raster posed and framed as this vanilla player renderer
    /// asks; the pose is requested for the next raster.
    pub(super) fn player_preview(
        &self,
        renderer: &str,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let preview = self.art.preview?;
        let (view, frame) = crate::ui_runtime::presentation::player_preview::renderer_frame(
            renderer,
            data,
            dest,
            self.px,
            self.art.pointer,
        );
        let view = view.with_menu_rotation(self.art.preview_rotation);
        if let Some(request) = self.art.preview_view {
            request.set(Some(view));
        }
        if let Some(control) = self.art.preview_control
            && data.get("rotation").and_then(Value::as_str) == Some("gesture_x")
        {
            control.set(Some(
                crate::ui_runtime::presentation::player_preview::controller::PreviewControl {
                    bounds: crate::ui_runtime::presentation::rect(
                        dest[0], dest[1], dest[2], dest[3],
                    )
                    .ok()?,
                    gui_pixel: self.px,
                },
            ));
        }
        Some((
            UiVisual::Sprite {
                texture_page: preview.page,
                uv: preview.uv,
                color: alpha([255; 4]),
            },
            frame,
        ))
    }

    /// Yellow text tilted up to the right around the control's position,
    /// pulsing up to 4% larger and shrunk to stay below the top of the screen.
    pub(super) fn splash(
        &mut self,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<()> {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        let text = self.art.splash?;
        let px = self.px;
        let seconds = EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64();
        let mut scale = 1.0 + 0.04 * (seconds * PI * 2.3).sin().powi(4) as f32;
        let (first, second) = split_sentence(text, SPLASH_LINE_CHARS);
        let width = |line: &str, painter: &mut Self| {
            let request = painter
                .metrics
                .request(line, width_64(UNWRAPPED_LOGICAL), painter.font);
            painter
                .layouts
                .layout(request)
                .map(|layout| layout.size_64()[0] as f32 / 64.0 / px)
                .unwrap_or(0.0)
        };
        let widest = width(first, self).max(second.map_or(0.0, |line| width(line, self)));
        let (sin, cos) = (-SPLASH_ANGLE).sin_cos();
        let lines = if second.is_some() { 2.5 } else { 1.0 };
        let origin = [dest[0] / px, dest[1] / px];
        let room = (origin[1] / sin - lines * LINE_HEIGHT * cos * 0.5)
            .min(((self.screen[2] / px).floor() * 0.95).floor());
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
            let request = scaled_request(
                &self.metrics,
                line,
                width_64(UNWRAPPED_LOGICAL),
                self.font,
                scale,
            );
            let Ok(layout) = self.layouts.layout(request) else {
                continue;
            };
            let [w, h] = layout.size_64().map(|size| size as f32 / 64.0);
            let visual = UiVisual::RotatedText {
                layout,
                color: alpha(SPLASH_COLOR),
                shadow: self.metrics.shadow(),
                angle_radians: SPLASH_ANGLE,
            };
            let bounds = [
                centre[0] - w * 0.5,
                centre[1] - h * 0.5,
                centre[0] + w * 0.5,
                centre[1] + h * 0.5,
            ];
            // The tilt reaches past the control, so it draws unclipped.
            self.group(self.screen).ok()?;
            self.push(visual, bounds).ok()?;
        }
        Some(())
    }

    /// `#playername` centred on the control over a backing one GUI px wider
    /// than the text, plus `#x_padding`/`#y_padding`.
    pub(super) fn name_tag(
        &mut self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let name = data
            .get("#playername")?
            .as_str()
            .filter(|name| !name.is_empty())?;
        let px = self.px;
        let padding = |key: &str| data.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32 * px;
        let (pad_x, pad_y) = (padding("#x_padding"), padding("#y_padding"));
        let request = self
            .metrics
            .request(name, width_64(UNWRAPPED_LOGICAL), self.font);
        let layout = self.layouts.layout(request).ok()?;
        let [w, h] = layout.size_64().map(|size| size as f32 / 64.0);
        let centre = [(dest[0] + dest[2]) * 0.5, (dest[1] + dest[3]) * 0.5];
        let left = centre[0] - w * 0.5;
        let top = centre[1] - h * 0.5;
        self.group(self.screen).ok()?;
        self.solid(
            [
                left - px - pad_x,
                top - px - pad_y,
                left + w + pad_x,
                top + h + pad_y,
            ],
            alpha(NAME_TAG_BACKGROUND),
        )
        .ok()?;
        Some((
            UiVisual::Text {
                layout,
                color: alpha([255; 4]),
                shadow: TextShadow::None,
            },
            [left, top, left + w, top + h],
        ))
    }
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
        let logo = image::load_from_memory(include_bytes!(
            "../../../../../../../assets/branding/title.png"
        ))
        .unwrap();
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
