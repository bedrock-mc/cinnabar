//! The Java sidebar must admit native server glyphs wider than vanilla's row cap.

use json_ui::{
    Catalog, Context, Draw, HUD_SCREEN, HudModel, LayoutEnv, Sidebar, TextMeasure, TextureMeta,
    TextureSource, ViewState, hud_context, hud_data_source, render_screen,
};

const SEPARATOR: &str = "\u{e000}";

/// The wide separator advances 125 GUI pixels; a single glyph cannot be wrapped.
struct ServerGlyphText;
impl TextMeasure for ServerGlyphText {
    fn extent(&self, text: &str) -> [f64; 2] {
        if text.is_empty() {
            return [0.0; 2];
        }
        let width = text
            .chars()
            .map(|character| if character == '\u{e000}' { 125.0 } else { 6.0 })
            .sum();
        [width, 9.0]
    }

    fn wrapped(&self, text: &str, width: f64) -> [f64; 2] {
        let natural = self.extent(text);
        if text.contains('\u{e000}') && width < natural[0] {
            // The native shaper rejects a glyph larger than the line box.
            [0.0; 2]
        } else {
            natural
        }
    }
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

#[test]
fn java_sidebar_overrides_vanilla_row_cap_for_wide_server_glyphs() {
    // A small synthetic base retains the real vanilla row's maximum, without
    // requiring the private vanilla pack in an upstream checkout.
    let files: [(&str, &[u8]); 4] = [
        (
            "ui/_global_variables.json",
            br#"{"$player_name_color":[1,1,1],"$objective_title_color":[1,1,1]}"#,
        ),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/scoreboards.json","ui/hud_screen.json"]}"#,
        ),
        (
            "ui/scoreboards.json",
            br#"{"namespace":"scoreboard","scoreboard_sidebar_player":{
                "type":"label","max_size":[100,10]}}"#,
        ),
        (
            "ui/hud_screen.json",
            br#"{"namespace":"hud","hud_screen":{"type":"panel",
                "size":["100%","100%"],"controls":[
                    {"sidebar@scoreboard.scoreboard_sidebar":{}}]}}"#,
        ),
    ];
    let mut catalog = Catalog::from_files(files).expect("synthetic vanilla HUD");
    catalog.apply_pack([(
        "ui/scoreboards.json",
        include_bytes!("../../../assets/java-hud/ui/scoreboards.json").as_slice(),
    )]);
    assert!(catalog.diagnostics().is_empty());
    let model = HudModel {
        sidebar: Some(Sidebar {
            title: "Duel".into(),
            rows: vec![
                (SEPARATOR.into(), String::new()),
                ("Opponent: zyn".into(), String::new()),
            ],
            background_opacity: 0.3,
            title_background_opacity: 0.4,
        }),
        ..HudModel::default()
    };
    let env = LayoutEnv {
        text: &ServerGlyphText,
        textures: &NoTextures,
    };
    let rendered = render_screen(
        HUD_SCREEN,
        &catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(&model),
        [599.0, 329.5],
        &env,
        &ViewState::default(),
    )
    .expect("sidebar renders");
    for text in [SEPARATOR, "Opponent: zyn"] {
        let node = rendered
            .nodes
            .iter()
            .find(|node| matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text))
            .expect("sidebar row remains visible");
        assert!(node.dest.w >= ServerGlyphText.extent(text)[0]);
        assert!(node.dest.x >= node.clip.x);
        assert!(node.dest.x + node.dest.w <= node.clip.x + node.clip.w + 1e-6);
    }
}
