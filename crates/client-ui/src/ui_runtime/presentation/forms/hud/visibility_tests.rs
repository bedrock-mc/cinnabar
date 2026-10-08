use super::{Arc, CachedScreen, Catalog, Context, DataSource};
use json_ui::{Draw, LayoutEnv, Scalar, TextMeasure, TextureMeta, TextureSource};

struct Metrics;

impl TextMeasure for Metrics {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

impl TextureSource for Metrics {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn source(text: &str) -> Arc<DataSource> {
    let mut data = DataSource::default();
    data.set_global("#source", Scalar::Text(text.into()));
    Arc::new(data)
}

#[test]
fn cached_hud_consumes_visibility_transition_on_unchanged_frame() {
    let catalog = Arc::new(Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/retained.json"]}"#.as_slice()),
        ("ui/retained.json", br##"{
            "namespace":"retained",
            "root":{"type":"panel","controls":[
                {"data_control":{"type":"panel","size":[0,0],"bindings":[
                    {"binding_name":"#source"},
                    {"binding_name":"#source","binding_name_override":"#preserved","binding_condition":"visibility_changed"},
                    {"binding_type":"view","source_property_name":"(not(#source = #preserved) and not((#source - 'fx.') = #source))","target_property_name":"#visible"}
                ]}},
                {"text":{"type":"label","text":"#text","bindings":[
                    {"binding_type":"view","source_control_name":"data_control","source_property_name":"#preserved","target_property_name":"#text"}
                ]}}
            ]}
        }"##.as_slice()),
    ]).unwrap());
    let mut screen = CachedScreen::default();
    let context = Context::desktop();
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    let render = |screen: &mut CachedScreen, data| {
        screen
            .render(
                "retained.root",
                &catalog,
                &context,
                data,
                ([480.0, 270.0], 1.0, [0; 3]),
                &env,
            )
            .unwrap()
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let empty = source("");
    render(&mut screen, Arc::clone(&empty));
    render(&mut screen, Arc::clone(&empty));
    assert_eq!(
        screen.passes, 2,
        "cache skipped a pending visibility-scheduled bind"
    );
    render(&mut screen, empty);
    assert_eq!(screen.passes, 2, "settled unchanged HUD rebuilt again");
    let first = source("fx.first");
    render(&mut screen, Arc::clone(&first));
    assert!(render(&mut screen, Arc::clone(&first)).contains(&"fx.first".into()));
    render(&mut screen, Arc::clone(&first));
    let settled = screen.passes;
    render(&mut screen, first);
    assert_eq!(screen.passes, settled);
    assert!(render(&mut screen, source("unrelated")).contains(&"fx.first".into()));
}
