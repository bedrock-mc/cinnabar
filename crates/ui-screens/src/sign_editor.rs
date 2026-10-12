//! The sign editor through vanilla `sign.sign_screen`: the wood's sign art
//! behind a multiline edit box holding the four lines with a caret, over an
//! input panel that closes the editor when pressed outside the sign.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{DataSource, HitRegion, Scalar, ViewState};
use serde_json::Value;
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiPoint, UiRect};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};

use crate::regions::window_rect;

pub const SIGN_SCREEN: &str = "sign.sign_screen";
/// The edit box caret's on and off time.
const CARET_BLINK_MILLIS: u64 = 500;

/// The sign screen's cached layout and last frame's pressable regions.
#[derive(Default)]
pub struct SignScreen {
    screen: CachedScreen,
    /// Window-logical rects, bottom to top, with where each routes when pressed.
    hits: Vec<(UiRect, Option<String>)>,
}

/// The sign being edited, borrowed from the client's editor.
pub struct SignInput<'a> {
    /// The wood's `textures/ui` art.
    pub texture: &'a str,
    pub hanging: bool,
    pub lines: &'a [String],
    /// The caret's line and character.
    pub cursor: (usize, usize),
    /// The text colour.
    pub color: [u8; 4],
    pub now_millis: u64,
    pub clocks: &'a BTreeMap<String, f64>,
}

impl SignScreen {
    /// Whether a press at `point` lands outside the sign, which closes the editor.
    pub fn exit_hit(&self, point: UiPoint) -> bool {
        self.hits
            .iter()
            .rev()
            .find(|(bounds, _)| bounds.contains(point))
            .is_some_and(|(_, pressed)| pressed.as_deref() == Some("button.menu_exit"))
    }

    /// Drops the hidden editor's hit regions.
    pub fn hide(&mut self) {
        self.hits.clear();
    }

    /// Paints the sign and keeps its pressable regions for the next input.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        input: SignInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let multiline = if input.hanging {
            "sign.hanging_sign_text_multiline"
        } else {
            "sign.regular_sign_text_multiline"
        };
        // Vanilla sign screen variables: the wood's art and edit box.
        let context = engine
            .context()
            .clone()
            .with_var("sign_texture", Value::from(input.texture.to_owned()))
            .with_var("sign_text_multiline", Value::from(multiline));
        let data = sign_data(&input);
        let catalog = Arc::clone(engine.catalog());
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let screen = &mut self.screen;
        let view = ViewState::default();
        let drawn = engine.draw(
            ScreenArt {
                clocks: Some(input.clocks),
                ..ScreenArt::default()
            },
            frame,
            |env, root| {
                screen.render_with(
                    SIGN_SCREEN,
                    &catalog,
                    &context,
                    data,
                    (root, px, language),
                    env,
                    &view,
                )
            },
        )?;
        if let Some(drawn) = drawn {
            self.hits = drawn
                .hits
                .iter()
                .filter_map(|region: &HitRegion| {
                    let bounds = window_rect(region, drawn.scale, drawn.origin)?;
                    Some((bounds, region.pressed.clone()))
                })
                .collect();
        }
        Ok(())
    }

    /// The sign screen's last laid-out draw nodes, in virtual px.
    pub fn nodes(&self) -> &[json_ui::DrawNode] {
        self.screen.nodes()
    }
}

/// What the sign controller binds: the lines with a caret, in the sign's colour.
fn sign_data(edit: &SignInput<'_>) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    let (cursor_line, cursor_column) = edit.cursor;
    let caret = (edit.now_millis / CARET_BLINK_MILLIS).is_multiple_of(2);
    let lines: Vec<String> = edit
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index != cursor_line || !caret {
                return line.clone();
            }
            let at = line
                .char_indices()
                .nth(cursor_column)
                .map_or(line.len(), |(at, _)| at);
            format!("{}|{}", &line[..at], &line[at..])
        })
        .collect();
    data.set_global("#sign_text", Scalar::Text(lines.join("\n")));
    let [r, g, b, _] = edit.color;
    data.set_global(
        "#edit_box_text_color",
        Scalar::Text(format!("#{r:02x}{g:02x}{b:02x}")),
    );
    data.set_global("#close_button_visible", Scalar::Bool(true));
    data
}
