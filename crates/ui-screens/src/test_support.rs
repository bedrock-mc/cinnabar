//! Layout fixtures for tests that lay screens out without a carrier's fonts or atlas.

/// Measures text at six px per character and nine px tall.
pub struct FixedText;
impl json_ui::TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

/// Answers every texture as a plain 16 px square.
pub struct NoTextures;
impl json_ui::TextureSource for NoTextures {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        Some(json_ui::TextureMeta {
            base_size: [16.0, 16.0],
            pixels: [16.0, 16.0],
            nineslice: None,
        })
    }
}

/// A chat history label drawing `text` `width` px wide inside an 800x500 clip.
pub fn history_node(text: &str, width: f64) -> json_ui::DrawNode {
    use json_ui::{Draw, DrawNode};

    DrawNode {
        name: "text".into(),
        key: "chat/messages_panel/messages_text/text".into(),
        dest: json_ui::RectOut {
            x: 10.0,
            y: 20.0,
            w: width,
            h: 400.0,
        },
        clip: json_ui::RectOut {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 500.0,
        },
        layer: 1,
        alpha: 1.0,
        anim: None,
        gates: Vec::new(),
        draw: Draw::Text {
            text: text.into(),
            color: [255; 4],
            shadow: true,
            align: json_ui::TextAlign::Left,
            scale: 1.0,
            localize: false,
            options: Default::default(),
        },
    }
}
