//! Local-only harness: renders server forms through the real UI carrier and, when
//! `CINNABAR_FORM_PACK_DIR` names an unpacked server resource pack, its ui overlay.
//! Skips when the gitignored carrier is absent; the pack is never committed.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeUiAssets};
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent};
use ui::{DpiScale, UiNode, UiVisual};

use super::super::{TextMetrics, UiPresentationRuntime, tests::fixture_font};
use super::ServerUiPack;
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};

const PACK_ENV: &str = "CINNABAR_FORM_PACK_DIR";

/// Resolves the installed placeholder text used by the input harness.
pub(crate) fn menu_translation(runtime: &UiRuntime, key: &str) -> Option<Arc<str>> {
    runtime.translation(key)
}

/// Loads the installed language catalog for real launcher input and GPU frames.
pub(crate) fn menu_runtime() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    if let Some(lang) = std::fs::read(local("assets/compiled/vanilla-v1.mcbelang"))
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(Arc::new(lang));
    }
    runtime
}

fn local(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(path)
}

pub(crate) fn carrier() -> Option<Arc<RuntimeUiAssets>> {
    let bytes = std::fs::read(local("assets/compiled/vanilla-v1.mcbeui")).ok()?;
    RuntimeUiAssets::decode(&bytes).ok().map(Arc::new)
}

pub(crate) fn font() -> Arc<RuntimeFontCatalog> {
    let manifest = crate::asset_startup::canonical_source_manifest_sha256(include_str!(
        "../../../../../assets/ui-font-source.json"
    ));
    std::fs::read(local("assets/compiled/ui-monocraft-v1.mcbefont"))
        .ok()
        .and_then(|bytes| RuntimeFontCatalog::decode(&bytes, manifest).ok())
        .map_or_else(fixture_font, Arc::new)
}

/// Every file of an unpacked pack directory as `(pack-relative path, bytes)`.
pub(crate) fn pack_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let (Ok(relative), Ok(bytes)) =
                (path.strip_prefix(root), std::fs::read(&path))
            {
                files.push((relative.to_string_lossy().replace('\\', "/"), bytes));
            }
        }
    }
    files.sort();
    files
}

/// The packs `CINNABAR_FORM_PACK_DIR` lists (`:`-separated, lowest first).
pub(crate) fn env_pack() -> Option<ServerUiPack> {
    Some(dir_pack(&std::env::var(PACK_ENV).ok()?))
}

/// Installs the real pack's Unicode cells alongside its JSON-UI textures.
pub(crate) fn env_glyphs() -> Option<Arc<super::super::SessionGlyphSheets>> {
    let roots = std::env::var(PACK_ENV).ok()?;
    let roots: Vec<_> = roots.split(':').map(Path::new).collect();
    let mut cells = Vec::new();
    for high_byte in 0..=u8::MAX {
        let source = roots.iter().rev().find_map(|root| {
            [
                format!("font/glyph_{high_byte:02X}.png"),
                format!("font/glyph_{high_byte:02x}.png"),
            ]
            .into_iter()
            .find_map(|path| image::open(root.join(path)).ok())
        });
        if let Some(source) = source {
            let source = source.into_rgba8();
            cells.extend(assets::extract_cells(&assets::GlyphSheet {
                high_byte,
                width: source.width(),
                height: source.height(),
                rgba8: source.into_raw().into_boxed_slice(),
            }));
        }
    }
    Some(Arc::new(super::super::SessionGlyphSheets::with_named(
        cells,
        Default::default(),
    )))
}

/// The unpacked packs `dirs` lists (`:`-separated, lowest first) as a session pack.
pub(crate) fn dir_pack(dirs: &str) -> ServerUiPack {
    let mut pack = ServerUiPack::default();
    let mut all = Vec::new();
    for dir in dirs.split(':').filter(|dir| !dir.is_empty()) {
        let files = pack_files(Path::new(dir));
        pack.ui_layers.push(
            files
                .iter()
                .filter(|(path, _)| path.starts_with("ui/") && path.ends_with(".json"))
                .cloned()
                .collect(),
        );
        all.extend(files);
    }
    let mut textures = BTreeMap::new();
    for (path, bytes) in all {
        if path.starts_with("textures/") {
            textures.insert(path, bytes);
        }
    }
    pack.textures = textures.into_iter().collect();
    pack
}

pub(crate) fn action_form(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    title: &str,
    buttons: &[&str],
) -> UiRuntime {
    image_form(player_runtime, title, buttons, Vec::new())
}

pub(crate) fn image_form(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    title: &str,
    buttons: &[&str],
    images: Vec<Option<protocol::FormButtonImage>>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 3,
                    kind: FormKind::Menu,
                    title: Some(Arc::from(title)),
                    json: Arc::from("{}"),
                    model: ServerFormModel::TextMenu(TextMenuForm {
                        title: Arc::from(title),
                        content: Arc::from(""),
                        buttons: buttons.iter().map(|text| Arc::from(*text)).collect(),
                        button_images: images.into(),
                        omitted_images: 0,
                    }),
                }),
            },
        )
        .unwrap();
    runtime
}

/// Text drawn by `nodes`, rebuilt from each layout's glyph codepoints.
pub(crate) fn drawn_texts(nodes: &[UiNode]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

pub(crate) fn render(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    size: [u32; 2],
    dpi: f32,
) -> Vec<UiNode> {
    let dpi = DpiScale::new(dpi).unwrap();
    let metrics = TextMetrics::for_viewport(size, dpi, None);
    let (width, height) = (size[0] as f32 / dpi.get(), size[1] as f32 / dpi.get());
    let mut nodes = Vec::new();
    let mut next = 1;
    presentation
        .append_server_form(runtime, &mut nodes, &mut next, metrics, width, height)
        .unwrap();
    presentation.sync_server_ui_pages();
    nodes
}

pub(crate) fn engine_presentation() -> Option<UiPresentationRuntime> {
    let carrier = carrier()?;
    let mut presentation = UiPresentationRuntime::new(font()).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let vanilla = local(&crate::install_layout::vanilla_pack_relative());
    let engine = presentation.form_presentation.engine.as_mut().unwrap();
    engine.textures.set_fallbacks(Default::default(), vanilla);
    Some(presentation)
}

/// Loads the startup texture layout, including HUD, icons, models and optional OreUI art.
pub(crate) fn startup_presentation() -> Option<UiPresentationRuntime> {
    let world = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate::asset_startup::DEFAULT_ASSET_PATH);
    let hud = crate::asset_startup::require_hud_assets(&world)
        .ok()?
        .into_runtime();
    let icons = crate::asset_startup::require_icon_assets(
        &world,
        include_str!("../../../../../assets/vanilla-source.json"),
    )
    .ok()?
    .into_runtime();
    let entities = assets::RuntimeEntityAssets::decode(
        &std::fs::read(crate::asset_startup::entity_asset_path(&world)).ok()?,
    )
    .ok()?;
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(font(), hud, icons).unwrap();
    presentation.enable_json_ui(carrier()?).unwrap();
    presentation.set_form_texture_fallbacks(
        &entities,
        local(&crate::install_layout::vanilla_pack_relative()),
    );
    if let Some(images) = crate::ui_runtime::oreui_assets::load_optional_oreui_images() {
        presentation.enable_oreui_originals(images).unwrap();
    }
    presentation
        .set_gui_models(&assets::RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    Some(presentation)
}

/// Retained nodes from the last published menu frame, including clipping and text.
pub(crate) fn menu_nodes(presentation: &UiPresentationRuntime) -> &[UiNode] {
    &presentation.last_menu.as_ref().expect("menu frame").nodes
}

/// Every text node with its bounds and its clip parent's bounds, for diagnosis.
pub(crate) fn dump(nodes: &[UiNode]) {
    for node in nodes {
        if let UiVisual::Text { layout, color, .. } = node.visual() {
            let text: String = layout
                .glyphs()
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect();
            let clip = node
                .parent()
                .and_then(|parent| nodes.iter().find(|other| other.id() == parent))
                .map(UiNode::bounds);
            eprintln!(
                "{text:?} lines={} size={:?} color={color:?} at {:?} clip {clip:?}",
                layout.line_count(),
                layout.size_64().map(|v| v as f32 / 64.0),
                node.bounds()
            );
        }
    }
}

#[test]
fn server_pack_form_renders_its_text_through_the_engine() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let pack = env_pack();
    if let Some(pack) = &pack {
        presentation.set_server_ui_pack(pack);
    }
    let buttons = [
        "Common Box\n§73 owned",
        "Rare Box",
        "Epic Box",
        "Legendary",
        "Back",
    ];
    let runtime = action_form(
        &mut player_runtime,
        "@mineville/boxes:Spirit Bundle",
        &buttons,
    );
    let nodes = render(&mut presentation, &runtime, [2560, 1600], 2.0);
    dump(&nodes);
    let identity = runtime.server_forms().active().unwrap().identity;
    let texts = drawn_texts(&nodes);
    let sprites = nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
        .count();
    eprintln!(
        "engine frame: {}, pack: {}, {} nodes, {sprites} sprites, texts: {texts:?}",
        presentation.form_engine_frame(identity).is_some(),
        pack.is_some(),
        nodes.len(),
    );
    assert!(presentation.form_engine_frame(identity).is_some());
    let (drawn, missing) = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .drawn_sprites();
    eprintln!(
        "server pages: {}, sprite textures drawn: {}, outside textures/ui: {:?}, unresolved: {missing:?}",
        presentation.server_ui_pages().len(),
        drawn.len(),
        drawn
            .iter()
            .filter(|key| !key.starts_with("textures/ui/"))
            .collect::<Vec<_>>()
    );
    assert!(
        missing.is_empty(),
        "every drawn image resolves: {missing:?}"
    );
    // Without a pack the vanilla template shows the labels verbatim.
    if pack.is_none() {
        for label in ["Rare Box", "Epic Box", "Legendary"] {
            assert!(texts.iter().any(|text| text.contains(label)), "{label}");
        }
    }
    assert!(!texts.is_empty());
    assert!(
        texts.iter().all(|text| !text.contains('§')),
        "format codes never draw"
    );
}

/// Each text node's visible rect: its bounds offset by, and cut to, its clip group.
fn visible_text_rects(nodes: &[UiNode]) -> Vec<[f32; 4]> {
    nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Text { .. }))
        .filter_map(|node| {
            let clip = nodes
                .iter()
                .find(|other| Some(other.id()) == node.parent())?
                .bounds();
            let (min, max) = (clip.min(), clip.max());
            let bounds = node.bounds();
            let rect = [
                (min.x() + bounds.min().x()).max(min.x()),
                (min.y() + bounds.min().y()).max(min.y()),
                (min.x() + bounds.max().x()).min(max.x()),
                (min.y() + bounds.max().y()).min(max.y()),
            ];
            (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
        })
        .collect()
}

// Multi-line labels stack one line apart and end in `...` at the label's height
// instead of spilling over the next button.
#[test]
fn multi_line_button_labels_never_overlap() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let buttons = [
        "Free For All\n§7Playing - 12",
        "Updates In - 2m 24s\nKills - 7\nKillstreak - 1",
        "Duels",
    ];
    let runtime = action_form(&mut player_runtime, "Free For All§zfp0;", &buttons);
    let nodes = render(&mut presentation, &runtime, [2560, 1600], 2.0);
    dump(&nodes);
    let rects = visible_text_rects(&nodes);
    for (index, a) in rects.iter().enumerate() {
        for b in &rects[index + 1..] {
            let overlap = a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
            assert!(!overlap, "{a:?} overlaps {b:?}");
        }
    }
    let texts = drawn_texts(&nodes);
    assert!(texts.iter().any(|text| text == "Free For AllPlaying - 12"));
    assert!(
        texts
            .iter()
            .any(|text| text == "Updates In - 2m 24sKills - 7...")
    );
}

// Per-frame form cost with the render cache versus re-resolving every frame.
#[test]
fn form_frame_cost_with_and_without_the_render_cache() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    if let Some(pack) = env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    let buttons: Vec<String> = (0..20)
        .map(|index| format!("Button {index}\n§7Line two"))
        .collect();
    let labels: Vec<&str> = buttons.iter().map(String::as_str).collect();
    let runtime = action_form(
        &mut player_runtime,
        "@mineville/boxes:Spirit Bundle",
        &labels,
    );
    let frame = |presentation: &mut UiPresentationRuntime, cold: bool| {
        if cold {
            presentation
                .form_presentation
                .engine
                .as_mut()
                .unwrap()
                .cache = None;
        }
        let started = std::time::Instant::now();
        render(presentation, &runtime, [2560, 1600], 2.0);
        started.elapsed()
    };
    frame(&mut presentation, true);
    let average = |presentation: &mut UiPresentationRuntime, cold: bool| {
        let total: std::time::Duration = (0..20).map(|_| frame(presentation, cold)).sum();
        total / 20
    };
    let uncached = average(&mut presentation, true);
    let cached = average(&mut presentation, false);
    eprintln!("form frame: re-resolving {uncached:?}, cached {cached:?}");
    assert!(cached < uncached);
}

// The vanilla template draws path and URL button images once they resolve.
#[test]
fn vanilla_form_button_images_resolve() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use protocol::FormButtonImage::{Path as ImagePath, Url};
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let url = format!("{}/icon.png", super::remote_images::tests::serve(png));
    let runtime = image_form(
        &mut player_runtime,
        "Shop",
        &["Diamond", "Stone", "Remote"],
        vec![
            Some(ImagePath("textures/items/diamond".into())),
            Some(ImagePath("textures/blocks/stone".into())),
            Some(Url(url.as_str().into())),
        ],
    );
    let engine = presentation.form_presentation.engine.as_ref().unwrap();
    let remote = engine.textures.remote.clone();
    render(&mut presentation, &runtime, [1280, 720], 1.0);
    super::remote_images::tests::settle(&remote, &url);
    render(&mut presentation, &runtime, [1280, 720], 1.0);
    let (drawn, missing) = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .drawn_sprites();
    eprintln!("drawn {drawn:?}, unresolved {missing:?}");
    for image in [
        "textures/items/diamond",
        "textures/blocks/stone",
        url.as_str(),
    ] {
        assert!(drawn.iter().any(|key| key == image), "{image} drawn");
    }
    assert!(missing.is_empty());
}

// A server-pack image bigger than a server page (Zeqa's 1992x669 title) draws
// from its full-resolution art copy, point-sampled, not the 256px downscale.
#[test]
fn large_server_pack_images_draw_at_full_resolution() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(1992, 669, image::Rgba([200, 30, 40, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: Vec::new(),
        textures: vec![("textures/ui/big_logo.png".to_owned(), png)],
        catalog: None,
        view: None,
    });
    let runtime = image_form(
        &mut player_runtime,
        "Logo",
        &["Logo"],
        vec![Some(protocol::FormButtonImage::Path(
            "textures/ui/big_logo".into(),
        ))],
    );
    render(&mut presentation, &runtime, [1280, 720], 1.0);
    presentation.finish_menu_artwork();
    let nodes = render(&mut presentation, &runtime, [1280, 720], 1.0);
    let widths: Vec<u16> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Sprite { uv, .. } => Some(uv[2] - uv[0]),
            _ => None,
        })
        .collect();
    assert!(widths.contains(&1022), "{widths:?}");
    assert!(!widths.contains(&256), "{widths:?}");
}

/// Button text shaped like a server's two-line entries plus a description.
fn entry(name: &str) -> String {
    format!("§e{name}\n§7PRACTICE\n§eDESCRIPTION\n§7Practice {name} here")
}

/// The snapshot's Zeqa training fixture keeps the full second-line word before dots.
#[test]
fn training_labels_keep_practice_before_the_ellipsis() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let Some(pack) = env_pack() else {
        eprintln!("skipping: server pack absent");
        return;
    };
    presentation.set_server_ui_pack(&pack);
    let button = entry("BRIDGING");
    let runtime = action_form(&mut player_runtime, "Training", &[&button]);
    let nodes = render(&mut presentation, &runtime, [1280, 720], 1.0);
    let texts = drawn_texts(&nodes);
    assert!(
        texts.iter().any(|text| text.contains("PRACTICE...")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("PRACTIC...")),
        "{texts:?}"
    );
}

// Writes PNG snapshots of pack forms for visual inspection (local only).
#[test]
fn snapshot_pack_forms() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    if let Some(pack) = env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    let training: Vec<String> = ["BRIDGING", "CLUTCH", "WALL RUN", "AIMING", "BOT DUEL"]
        .iter()
        .map(|name| entry(name))
        .collect();
    let ffa: Vec<String> = ["SUMO", "MACE", "BUILD", "SKYWARS"]
        .iter()
        .map(|name| entry(name))
        .collect();
    let boxes: Vec<String> = (0..90).map(|index| format!("Item {index}")).collect();
    let forms: [(&str, &str, &[String]); 3] = [
        ("training", "Training", &training),
        ("ffa", "Free For All§zfp0;", &ffa),
        ("boxes", "@mineville/boxes:Spirit Bundle", &boxes),
    ];
    let navigator: Vec<String> = [
        "Lobby", "Factions", "Skyblock", "KitPvP", "Practice", "Events",
    ]
    .iter()
    .map(|name| format!("§l{name}\n§r§7Click to join"))
    .collect();
    let navigator_title =
        std::env::var("CINNABAR_NAV_TITLE").unwrap_or_else(|_| "Navigator".to_owned());
    let icons = [
        "compass",
        "diamond",
        "apple_golden",
        "blaze_powder",
        "paper",
        "book_normal",
    ];
    for (name, title, buttons) in
        forms
            .into_iter()
            .chain([("navigator", navigator_title.as_str(), navigator.as_slice())])
    {
        let labels: Vec<&str> = buttons.iter().map(String::as_str).collect();
        let runtime = if name == "navigator" {
            let images = icons
                .iter()
                .map(|icon| {
                    Some(protocol::FormButtonImage::Path(
                        format!("textures/items/{icon}").into(),
                    ))
                })
                .collect();
            image_form(&mut player_runtime, title, &labels, images)
        } else {
            action_form(&mut player_runtime, title, &labels)
        };
        let dpi = DpiScale::new(1.0).unwrap();
        for _ in 0..2 {
            presentation
                .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
                .unwrap();
        }
        let input = presentation
            .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
            .unwrap();
        super::snapshot::write(&input, name);
        // The same form with its second button hovered.
        let identity = runtime.server_forms().active().unwrap().identity;
        let hovered = presentation.form_engine_frame(identity).and_then(|frame| {
            let mut buttons = frame
                .hits
                .iter()
                .filter(|hit| hit.kind == json_ui::HitKind::Button);
            buttons
                .clone()
                .nth(1)
                .or(buttons.next_back())
                .map(|hit| hit.key.clone())
        });
        let mut runtime = runtime;
        runtime.server_forms_mut().engine_mut().view.hovered = hovered;
        let input = presentation
            .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
            .unwrap();
        super::snapshot::write(&input, &format!("{name}-hover"));
    }
}

// Writes a snapshot of a pack's 2x2 image grid with a featured card (local only).
#[test]
fn snapshot_pack_image_grid() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let Some(pack) = env_pack() else {
        return;
    };
    presentation.set_server_ui_pack(&pack);
    let modes = ["mace", "skywars", "crystalpvp", "sumo", "build", "mace"];
    let labels: Vec<String> = modes
        .iter()
        .enumerate()
        .map(|(index, mode)| {
            let flags = if index == 0 {
                "§f§e§a§x§p§i§r§s§h"
            } else {
                ""
            };
            format!(
                "{flags}§e{}\n§a 19\n§7- - - - - - -",
                mode.to_ascii_uppercase()
            )
        })
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let images = modes
        .iter()
        .map(|mode| {
            Some(protocol::FormButtonImage::Path(
                format!("textures/ui/zeqa/icons/gm/{mode}").into(),
            ))
        })
        .collect();
    let runtime = image_form(&mut player_runtime, "Free For All§zfp0;", &labels, images);
    let dpi = DpiScale::new(2.0).unwrap();
    for _ in 0..2 {
        presentation
            .build(&player_runtime, &runtime, 0, [1280, 1440], dpi)
            .unwrap();
    }
    let input = presentation
        .build(&player_runtime, &runtime, 0, [1280, 1440], dpi)
        .unwrap();
    super::snapshot::write(&input, "image-grid");
    let identity = runtime.server_forms().active().unwrap().identity;
    let hovered = presentation.form_engine_frame(identity).and_then(|frame| {
        frame
            .hits
            .iter()
            .filter(|hit| hit.kind == json_ui::HitKind::Button)
            .nth(3)
            .map(|hit| hit.key.clone())
    });
    let mut runtime = runtime;
    runtime.server_forms_mut().engine_mut().view.hovered = hovered;
    let input = presentation
        .build(&player_runtime, &runtime, 0, [1280, 1440], dpi)
        .unwrap();
    super::snapshot::write(&input, "image-grid-hover");
    let (drawn, missing) = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .drawn_sprites();
    eprintln!("drawn {drawn:?}, unresolved {missing:?}");
}

/// The start screen shows no unresolved `$variable`, raw key or demo control.
#[test]
fn start_screen_text_resolves_through_the_language_table() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let Some(lang) = std::fs::read(local("assets/compiled/vanilla-v1.mcbelang"))
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    else {
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(lang));
    let menu = crate::menu::MenuRuntime::new(true, 2, "Player".to_owned());
    presentation.set_menu_view(Some(menu.view()));
    let dpi = DpiScale::new(1.0).unwrap();
    let metrics = TextMetrics::for_viewport([1280, 720], dpi, None);
    let mut nodes = Vec::new();
    let mut next = 1;
    presentation
        .append_menu(&runtime, &mut nodes, &mut next, metrics, 1280.0, 720.0)
        .unwrap();
    let texts = drawn_texts(&nodes);
    for label in ["Play", "Settings", "Marketplace"] {
        assert!(
            texts.iter().any(|text| text == label),
            "{label} in {texts:?}"
        );
    }
    for text in &texts {
        assert!(
            !text.starts_with('$') && !text.contains("start_screen.") && text != "Unlock Full Game",
            "{text:?} in {texts:?}"
        );
    }
}

/// An isolated scratch directory for a carrier regression's writable fixtures.
pub(crate) fn scratch_dir(label: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "cinnabar-{label}-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}
