//! Pack template replacements discard the earlier definition's own properties.

use json_ui::{
    BindState, Catalog, Context, Draw, HudModel, LayoutEnv, ResolvedControl, Sidebar, TextMeasure,
    TextureMeta, TextureSource, hud_data_source,
};
use serde_json::json;

/// Makes two templates and a control with its own content to replace.
fn catalog() -> Catalog {
    Catalog::from_files([
        (json_ui::GLOBAL_VARIABLES_PATH, b"{}".as_slice()),
        (
            json_ui::UI_DEFS_PATH,
            br#"{"ui_defs":["ui/screen.json"]}"#.as_slice(),
        ),
        (
            "ui/screen.json",
            br#"{"namespace":"screen",
                "first":{"type":"panel","size":[10,10]},
                "second":{"type":"panel","size":[30,20],"controls":[
                    {"caption":{"type":"label","text":"Replacement"}}]},
                "root":{"type":"panel","size":[100,100],"alpha":0.5,
                    "controls":[{"old":{"type":"label","text":"Old"}}]}}
            "#
            .as_slice(),
        ),
    ])
    .unwrap()
}

#[test]
fn pack_inheritance_replaces_the_previous_definition() {
    for prior in ["root", "root@screen.first"] {
        let mut catalog = catalog();
        catalog.overlay_text(
            "ui/screen.json",
            &json!({prior: {"alpha": 0.5}}).to_string(),
        );
        catalog.overlay_text("ui/screen.json", r#"{"root@screen.second":{}}"#);
        let root = json_ui::resolve(&catalog, "screen.root", &Context::empty())
            .control
            .unwrap();
        assert_eq!(root.properties["size"], json!([30, 20]));
        assert!(!root.properties.contains_key("alpha"));
        assert_eq!(texts(root), ["Replacement"]);
        assert!(
            catalog.diagnostics().is_empty(),
            "{:?}",
            catalog.diagnostics()
        );
    }
}

#[test]
fn pack_inheritance_preserves_patches_to_the_same_template() {
    let mut catalog = catalog();
    catalog.overlay_text("ui/screen.json", r#"{"root@screen.second":{"alpha":0.5}}"#);
    catalog.overlay_text("ui/screen.json", r#"{"root@screen.second":{"layer":2}}"#);
    catalog.overlay_text("ui/screen.json", r#"{"root":{"visible":false}}"#);
    let root = json_ui::resolve(&catalog, "screen.root", &Context::empty())
        .control
        .unwrap();
    assert_eq!(root.properties["alpha"], 0.5);
    assert_eq!(root.properties["layer"], 2);
    assert_eq!(root.properties["visible"], false);
    assert_eq!(root.children[0].properties["text"], "Replacement");
}

struct FixedText;

impl TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

struct NoTextures;

impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

/// Returns the text the resolved and bound control would draw.
fn texts(root: ResolvedControl) -> Vec<String> {
    json_ui::render_bound(
        root,
        [480.0, 270.0],
        &LayoutEnv {
            text: &FixedText,
            textures: &NoTextures,
        },
        &Default::default(),
    )
    .nodes
    .iter()
    .filter_map(|node| match &node.draw {
        Draw::Text { text, .. } => Some(text.clone()),
        _ => None,
    })
    .collect()
}

/// Renders the sidebar with the same collections the gameplay HUD supplies.
fn sidebar_texts(catalog: &Catalog, model: &HudModel) -> Vec<String> {
    let context = Context::desktop();
    let root = json_ui::resolve(catalog, "scoreboard.scoreboard_sidebar", &context)
        .control
        .expect("sidebar resolves");
    texts(json_ui::bind_screen(
        &root,
        catalog,
        &context,
        &hud_data_source(model),
        &mut BindState::new(),
    ))
}

#[test]
fn local_sidebar_pack_inheritance_omits_scores() {
    let Some(directory) = std::env::var_os("CINNABAR_SCOREBOARD_PACK_DIR") else {
        eprintln!("skipping sidebar pack fixture: missing CINNABAR_SCOREBOARD_PACK_DIR");
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    if !directory.join("ui/scoreboards.json").is_file() {
        eprintln!("skipping sidebar pack fixture: missing ui/scoreboards.json");
        return;
    }
    let mut files = crate::support::java_pack::files();
    let paths: Vec<_> = files
        .iter()
        .map(|(path, _)| path)
        .filter(|path| path.as_str() != json_ui::GLOBAL_VARIABLES_PATH)
        .collect();
    let index = serde_json::to_vec(&json!({"ui_defs": paths})).unwrap();
    files.push((json_ui::UI_DEFS_PATH.into(), index));
    let mut catalog = Catalog::from_files(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    )
    .unwrap();
    let model = HudModel {
        sidebar: Some(Sidebar {
            title: "Sidebar".into(),
            rows: (0..8)
                .map(|index| (format!("Row {index}"), index.to_string()))
                .collect(),
            background_opacity: 0.3,
            title_background_opacity: 0.4,
        }),
        ..Default::default()
    };
    let original = sidebar_texts(&catalog, &model);
    for index in 0..8 {
        assert!(original.contains(&index.to_string()), "{original:?}");
    }
    let mut paths = vec![
        json_ui::GLOBAL_VARIABLES_PATH.to_owned(),
        json_ui::UI_DEFS_PATH.to_owned(),
        "ui/scoreboards.json".into(),
    ];
    let index = std::fs::read(directory.join(json_ui::UI_DEFS_PATH)).expect("pack index");
    paths.extend(Catalog::declared_paths(&index).unwrap());
    let files: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(directory.join(&path)).expect("pack UI document");
            (path, bytes)
        })
        .collect();
    catalog.apply_pack(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    let replaced = sidebar_texts(&catalog, &model);
    for index in 0..8 {
        assert!(replaced.contains(&format!("Row {index}")), "{replaced:?}");
        assert!(
            !replaced.contains(&index.to_string()),
            "score still drawn: {replaced:?}"
        );
    }
}
