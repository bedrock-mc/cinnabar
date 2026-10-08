//! Cached screens follow their text environment and resolution context.

use super::*;
use std::borrow::Cow;

struct Metrics(&'static str);

impl json_ui::TextMeasure for Metrics {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 6.0, 9.0]
    }

    fn localize<'a>(&self, text: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(if text == "label.key" { self.0 } else { text })
    }
}

impl json_ui::TextureSource for Metrics {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

#[test]
fn cached_hud_remeasures_language_and_context_changes() {
    let catalog = Arc::new(
        Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            ("ui/_ui_defs.json", br#"{"ui_defs":["ui/cache.json"]}"#.as_slice()),
            (
            "ui/cache.json",
            br#"{"namespace":"cache","root":{"type":"label","text":"$label","size":["default","default"]}}"#.as_slice(),
        )])
        .unwrap(),
    );
    let mut context = Context::desktop().with_var("label", serde_json::json!("label.key"));
    let mut cache = CachedScreen::default();
    let mut data = DataSource::new();
    let state = ViewState::default();
    let metrics = Metrics("unused");
    assert!(
        cache
            .render_with(
                "cache.missing",
                &catalog,
                &context,
                data.clone(),
                ([480.0, 270.0], 1.0, [0; 3]),
                &json_ui::LayoutEnv {
                    text: &metrics,
                    textures: &metrics
                },
                &state,
            )
            .is_none()
    );
    for (revision, translated, data_change, context_change) in [
        (0, "one", false, false),
        (1, "longer translation", false, false),
        (2, "another longer translation", true, false),
        (2, "another longer translation", false, true),
    ] {
        if data_change {
            data.set_global("#unrelated", json_ui::Scalar::Bool(true));
        }
        if context_change {
            context = context.with_var("label", serde_json::json!("context changed"));
        }
        let metrics = Metrics(translated);
        let env = json_ui::LayoutEnv {
            text: &metrics,
            textures: &metrics,
        };
        let render = cache
            .render_with(
                "cache.root",
                &catalog,
                &context,
                data.clone(),
                ([480.0, 270.0], 1.0, [revision, 0, 0]),
                &env,
                &state,
            )
            .unwrap();
        let expected = if context_change {
            "context changed"
        } else {
            translated
        };
        assert_eq!(render.nodes[0].dest.w, expected.len() as f64 * 6.0);
    }
    assert_eq!(cache.passes, 4);
}
