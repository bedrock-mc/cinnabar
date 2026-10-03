//! Local-only: renders server forms through the vanilla catalog overlaid with the
//! unpacked resource packs `CINNABAR_FORM_PACK_DIR` lists (`:`-separated, lowest
//! first). Run explicitly after provisioning both; packs are never committed.

mod support;

use std::path::Path;

use json_ui::{
    ActionElement, ActionForm, Catalog, Context, Draw, FormButton, FormModel, LayoutEnv,
    TextMeasure, TextureMeta, TextureSource, render_form,
};

/// Six virtual pixels per character and nine per line, near the vanilla font.
struct ApproxText;
impl TextMeasure for ApproxText {
    fn extent(&self, text: &str) -> [f64; 2] {
        let lines = text.split('\n').filter(|line| !line.is_empty());
        let (count, widest) = lines.fold((0, 0), |(count, widest), line| {
            (count + 1, widest.max(line.chars().count()))
        });
        [widest as f64 * 6.0, f64::from(count.max(1)) * 9.0]
    }
}

struct AnyTexture;
impl TextureSource for AnyTexture {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [16.0, 16.0],
            pixels: [16.0, 16.0],
            nineslice: None,
        })
    }
}

fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let (Ok(relative), Ok(bytes)) =
                (path.strip_prefix(root), std::fs::read(&path))
            {
                let relative = relative.to_string_lossy().replace('\\', "/");
                if relative.starts_with("ui/") && relative.ends_with(".json") {
                    out.push((relative, bytes));
                }
            }
        }
    }
    out
}

fn overlaid() -> Option<Catalog> {
    let vanilla = support::vanilla_pack().join("ui");
    let packs = std::env::var("CINNABAR_FORM_PACK_DIR").ok()?;
    let mut catalog = Catalog::load_dir(&vanilla).ok()?;
    let base = catalog.diagnostics().len();
    for dir in packs.split(':').filter(|dir| !dir.is_empty()) {
        let files = files(Path::new(dir));
        catalog.apply_pack(files.iter().map(|(p, b)| (p.as_str(), b.as_slice())));
    }
    for note in catalog.diagnostics()[base..].iter().take(40) {
        eprintln!("pack: {note}");
    }
    Some(catalog)
}

/// Prints the laid-out chain from the root to each control named `target`.
fn trace<'a>(
    node: &'a json_ui::LaidOut<'a>,
    target: &str,
    chain: &mut Vec<&'a json_ui::LaidOut<'a>>,
) {
    chain.push(node);
    if node.control.name == target {
        for step in chain.iter() {
            eprintln!(
                "  {} {:?} size={:?} offset={:?} anchor={:?}/{:?} rect={:?}",
                step.control.name,
                step.control.control_type,
                step.control.properties.get("size"),
                step.control.properties.get("offset"),
                step.control.properties.get("anchor_from"),
                step.control.properties.get("anchor_to"),
                step.rect
            );
        }
        eprintln!("--");
    }
    for child in &node.children {
        trace(child, target, chain);
    }
    chain.pop();
}

#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first and CINNABAR_FORM_PACK_DIR"]
fn pack_overlay_routes_a_marked_title_to_the_pack_layout() {
    let Some(catalog) = overlaid() else {
        panic!(
            "requires the pinned local vanilla UI pack; fetch vanilla-assets first and CINNABAR_FORM_PACK_DIR"
        );
    };
    let title = std::env::var("CINNABAR_FORM_TITLE")
        .unwrap_or_else(|_| "@mineville/boxes:Spirit Bundle".to_owned());
    let count: usize = std::env::var("CINNABAR_FORM_BUTTONS")
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(90);
    let elements = (0..count)
        .map(|index| {
            ActionElement::Button(FormButton {
                text: format!("Item {index}\n§7Rare"),
                image: None,
            })
        })
        .collect();
    let model = FormModel::Action(ActionForm {
        title,
        body: String::new(),
        elements,
    });
    let env = LayoutEnv {
        text: &ApproxText,
        textures: &AnyTexture,
    };
    let started = std::time::Instant::now();
    let root = json_ui::resolve(
        &catalog,
        "server_form.zeqa_main_content",
        &Context::desktop(),
    );
    eprintln!(
        "resolve content took {:?}, {} diagnostics",
        started.elapsed(),
        root.diagnostics.len()
    );
    let started = std::time::Instant::now();
    let bound = json_ui::bind_form(&model, &catalog, &Context::desktop()).expect("binds");
    eprintln!("bind_form took {:?}", started.elapsed());
    if let Ok(target) = std::env::var("CINNABAR_FORM_TRACE") {
        let (laid, _) = json_ui::layout_with(&bound, [427.0, 240.0], &env, &Default::default());
        trace(&laid, &target, &mut Vec::new());
    }
    let started = std::time::Instant::now();
    let render = json_ui::render_bound(bound, [427.0, 240.0], &env, &Default::default());
    eprintln!("layout+emit took {:?}", started.elapsed());
    let _ = render_form;
    let mut texts = 0;
    for node in &render.nodes {
        match &node.draw {
            Draw::Text {
                text, color, scale, ..
            } => {
                texts += 1;
                eprintln!(
                    "text {:?} {text:?} at {:?} clip {:?} alpha {} color {color:?} scale {scale}",
                    node.name, node.dest, node.clip, node.alpha
                );
            }
            Draw::Sprite { texture, .. } => eprintln!("sprite {} {texture}", node.name),
            _ => {}
        }
    }
    eprintln!(
        "{} nodes, {texts} texts, {} hits",
        render.nodes.len(),
        render.hits.len()
    );
    assert!(texts > 0);
}
