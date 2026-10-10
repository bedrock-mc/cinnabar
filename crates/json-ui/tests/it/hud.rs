//! The gameplay HUD against the real vanilla templates. The `.local` pack is
//! gitignored, so reference tests explain missing fixtures and skip until the pack is fetched.

use crate::support;
use crate::support::java_pack;

use std::path::PathBuf;

use json_ui::{
    BossBar, Catalog, Context, Draw, DrawNode, HUD_SCREEN, HudModel, HudSlot, HudTitle, LayoutEnv,
    Sidebar, TextMeasure, TextureMeta, TextureSource, Timed, ViewState, hud_context,
    hud_data_source, parse_texture_meta, render_screen,
};

fn pack() -> Option<PathBuf> {
    let dir = support::vanilla_pack();
    dir.join("ui").is_dir().then_some(dir)
}

/// Six virtual px per character, nine per line.
struct FixedText;
impl TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        if text.is_empty() {
            return [0.0, 0.0];
        }
        let lines = text.split('\n');
        let width = lines
            .clone()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        [width as f64 * 6.0, lines.count() as f64 * 9.0]
    }
}

/// Texture sizes read from the pack's png headers and json sidecars, cached.
struct PackTextures(
    PathBuf,
    std::cell::RefCell<std::collections::HashMap<String, Option<TextureMeta>>>,
);
impl TextureSource for PackTextures {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        if let Some(meta) = self.1.borrow().get(path) {
            return *meta;
        }
        let meta = self.read(path);
        self.1.borrow_mut().insert(path.to_owned(), meta);
        meta
    }
}

impl PackTextures {
    fn new(dir: PathBuf) -> Self {
        Self(dir, Default::default())
    }

    fn read(&self, path: &str) -> Option<TextureMeta> {
        let stem = path.trim_end_matches(".png");
        if let Ok(text) = std::fs::read_to_string(self.0.join(format!("{stem}.json")))
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(meta) = parse_texture_meta(&value)
        {
            return Some(meta);
        }
        let bytes = std::fs::read(self.0.join(format!("{stem}.png"))).ok()?;
        let dimension = |at: usize| -> Option<f64> {
            Some(f64::from(u32::from_be_bytes(
                bytes.get(at..at + 4)?.try_into().ok()?,
            )))
        };
        Some(TextureMeta {
            base_size: [dimension(16)?, dimension(20)?],
            pixels: [dimension(16)?, dimension(20)?],
            nineslice: None,
        })
    }
}

fn model() -> HudModel {
    HudModel {
        survival_ui: true,
        armor_visible: true,
        hotbar_visible: true,
        xp_bar: true,
        exp_progress: 0.5,
        level: 7,
        hotbar: (0..9)
            .map(|index| HudSlot {
                icon: (index == 0).then_some(0),
                count: if index == 0 { 12 } else { 0 },
                selected: index == 2,
                durability: None,
            })
            .collect(),
        chat_visible: true,
        chat_lifetime: 10.0,
        chat_background_opacity: 0.5,
        chat: vec![Timed {
            text: "hello".into(),
            born: 0.0,
        }],
        title: Some(HudTitle {
            creation_id: 1,
            title: "Title".into(),
            subtitle: "Sub".into(),
            fade_in: 0.5,
            stay: 3.5,
            fade_out: 1.0,
            background_alpha: 0.0,
            born: 0.0,
        }),
        actionbar: Some(Timed {
            text: "bar".into(),
            born: 0.0,
        }),
        tip: Some(Timed {
            text: "tip".into(),
            born: 0.0,
        }),
        sidebar: Some(Sidebar {
            title: "Kills".into(),
            rows: vec![("Steve".into(), "3".into()), ("Alex".into(), "1".into())],
            background_opacity: 0.3,
            title_background_opacity: 0.4,
        }),
        boss_bars: vec![BossBar {
            name: "Wither".into(),
            progress: 0.75,
            color: "#aa00aa".into(),
            notches: 0,
        }],
        ..HudModel::default()
    }
}

fn render(model: &HudModel) -> Option<Vec<DrawNode>> {
    render_with(model, false)
}

fn render_with(model: &HudModel, java: bool) -> Option<Vec<DrawNode>> {
    render_full(model, java).map(|render| render.nodes)
}

/// The vanilla UI catalog for the pack's directory, before any overlay.
fn catalog_for(dir: &PathBuf) -> Catalog {
    Catalog::load_dir(&dir.join("ui")).expect("vanilla ui loads")
}

fn render_full(model: &HudModel, java: bool) -> Option<json_ui::ScreenRender> {
    let dir = pack()?;
    let mut catalog = catalog_for(&dir);
    if java {
        let files = java_pack::files();
        let before = catalog.diagnostics().len();
        catalog.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
        let notes = &catalog.diagnostics()[before..];
        assert!(notes.is_empty(), "java pack diagnostics: {notes:?}");
    }
    let textures = PackTextures::new(dir);
    let env = LayoutEnv {
        text: &FixedText,
        textures: &textures,
    };
    let render = render_screen(
        HUD_SCREEN,
        &catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(model),
        [480.0, 270.0],
        &env,
        &ViewState::default(),
    )
    .expect("hud renders");
    Some(render)
}

// The HUD never takes a gameplay click: a press at the crosshair reaches no control.
#[test]
fn hud_leaves_gameplay_clicks_alone() {
    for java in [false, true] {
        let Some(render) = render_full(&model(), java) else {
            return;
        };
        let mut view = ViewState::default();
        let mut dispatcher = json_ui::Dispatcher::default();
        let center = [240.0, 135.0];
        let hover = dispatcher.pointer(
            &render.hits,
            &mut view,
            json_ui::PointerInput {
                point: Some(center),
                held: false,
                mode: json_ui::InputMode::Mouse,
                now: 0.0,
            },
        );
        assert!(
            !hover.consumed,
            "java {java}: hover taken by {:?}",
            view.hovered
        );
        for down in [true, false] {
            let press = dispatcher.button(
                &render.hits,
                &mut view,
                json_ui::ButtonInput {
                    id: "button.menu_select",
                    down,
                    point: Some(center),
                    mode: json_ui::InputMode::Mouse,
                    now: 0.0,
                },
            );
            assert!(
                !press.consumed,
                "java {java}: press consumed: {:?}",
                press.events
            );
        }
    }
}

fn named<'a>(nodes: &'a [DrawNode], name: &str) -> Vec<&'a DrawNode> {
    nodes.iter().filter(|node| node.name == name).collect()
}

fn dump(nodes: &[DrawNode]) {
    if std::env::var_os("HUD_DUMP").is_some() {
        for node in nodes {
            eprintln!(
                "{:40} {:7.1} {:7.1} {:6.1} {:6.1} a={:.2} f={} {:?}",
                node.name,
                node.dest.x,
                node.dest.y,
                node.dest.w,
                node.dest.h,
                node.alpha,
                node.anim.is_some(),
                match &node.draw {
                    Draw::Text { text, .. } => format!("text {text:?}"),
                    Draw::Sprite { texture, .. } => texture.clone(),
                    Draw::Custom { renderer, .. } => format!("custom {renderer}"),
                    Draw::Solid { .. } => "solid".into(),
                }
            );
        }
    }
}

#[test]
fn vanilla_hud_draws_its_bound_surfaces() {
    let Some(nodes) = render(&model()) else {
        return;
    };
    dump(&nodes);
    // Item-lock overlays bind flags the controller never raises for the hotbar.
    assert!(named(&nodes, "container_item_lock_yellow").is_empty());
    assert!(named(&nodes, "container_item_lock_red").is_empty());
    let customs: Vec<&str> = nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, .. } => Some(renderer.as_str()),
            _ => None,
        })
        .collect();
    for renderer in [
        "heart_renderer",
        "hunger_renderer",
        "armor_renderer",
        "hotbar_renderer",
    ] {
        assert!(
            customs.contains(&renderer),
            "{renderer} missing: {customs:?}"
        );
    }
    assert_eq!(
        customs
            .iter()
            .filter(|name| **name == "hotbar_renderer")
            .count(),
        9
    );
    let texts: Vec<&str> = nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for text in [
        "Title", "Sub", "bar", "hello", "Kills", "Steve", "3", "Wither", "7", "12",
    ] {
        assert!(texts.contains(&text), "{text} missing: {texts:?}");
    }
    // The selected slot frame sits over the third cell.
    let selected = named(&nodes, "hotbar_slot_selected_image");
    assert_eq!(selected.len(), 1);
    // Title and chat carry their fades.
    assert!(named(&nodes, "title").iter().all(|node| {
        node.anim
            .as_ref()
            .is_some_and(|anim| !anim.alpha.is_empty())
    }));
}

fn text_node<'a>(nodes: &'a [DrawNode], text: &str) -> &'a DrawNode {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text))
        .unwrap_or_else(|| panic!("no text {text:?}"))
}

fn custom<'a>(nodes: &'a [DrawNode], renderer: &str) -> Vec<&'a DrawNode> {
    nodes
        .iter()
        .filter(
            |node| matches!(&node.draw, Draw::Custom { renderer: drawn, .. } if drawn == renderer),
        )
        .collect()
}

fn at(node: &DrawNode) -> [f64; 2] {
    [node.dest.x, node.dest.y]
}

fn descendant<'a>(
    control: &'a json_ui::ResolvedControl,
    name: &str,
) -> Option<&'a json_ui::ResolvedControl> {
    if control.name == name {
        return Some(control);
    }
    control
        .children
        .iter()
        .find_map(|child| descendant(child, name))
}

#[test]
fn java_selected_item_name_keeps_spawned_root_offset_above_the_hotbar() {
    let mut item = model();
    item.item_name = Some(Timed {
        text: "Dirt".into(),
        born: 0.0,
    });
    let mut bottoms = Vec::new();
    for survival in [false, true] {
        item.survival_ui = survival;
        let Some(render) = render_full(&item, true) else {
            return;
        };
        let name = text_node(&render.nodes, "Dirt");
        let slots = custom(&render.nodes, "hotbar_renderer");
        let first = slots.first().expect("hotbar start");
        let last = slots.last().expect("hotbar end");
        let hotbar_bottom = first.dest.y + first.dest.h;
        let root = descendant(&render.bound, "item_name_text").expect("spawned item-name root");
        let label = descendant(root, "item_text_label").expect("native item-name label");
        assert_eq!(label.properties["localize"], serde_json::Value::Bool(false));
        let dy = root.properties["offset"][1]
            .as_f64()
            .expect("authored root offset");
        assert!(dy < 0.0, "a zero-height factory cannot carry this offset");
        let buffer = descendant(root, "survival_buffer").expect("native survival spacer");
        let padding = if survival {
            buffer.properties["size"][1].as_f64().unwrap()
        } else {
            0.0
        };
        let bottom = name.dest.y + name.dest.h;
        // Read the authored offset and padding; do not duplicate HUD constants.
        assert_eq!(bottom, hotbar_bottom + dy - padding);
        assert_eq!(
            name.dest.x + name.dest.w / 2.0,
            (first.dest.x + last.dest.x + last.dest.w) / 2.0
        );
        assert!(
            bottom < first.dest.y,
            "item name overlaps hotbar: {:?}",
            name.dest
        );
        if survival {
            assert!(bottom < custom(&render.nodes, "heart_renderer")[0].dest.y);
        }
        assert!(
            name.anim
                .as_ref()
                .is_some_and(|anim| !anim.alpha.is_empty())
        );
        assert!(!render.nodes.iter().any(|node| matches!(
            &node.draw,
            Draw::Sprite { texture, .. } if texture == "textures/ui/hud_tip_text_background"
        )));
        bottoms.push(bottom);
    }
    assert!(
        bottoms[1] < bottoms[0],
        "survival padding must raise the item name"
    );
}

/// The built-in level label shares the survival status row and clears the XP bar.
#[test]
fn built_in_level_aligns_with_health_and_hunger() {
    for level in [2, 27, 123] {
        let mut hud = model();
        hud.level = level;
        let Some(nodes) = render_with(&hud, true) else {
            return;
        };
        let text = level.to_string();
        let label = nodes
            .iter()
            .find(|node| {
                matches!(&node.draw, Draw::Text { text: value, color, .. }
                if value == &text && color[..3] != [0, 0, 0])
            })
            .expect("visible level label");
        let hearts = custom(&nodes, "heart_renderer")[0];
        let hunger = custom(&nodes, "hunger_renderer")[0];
        assert_eq!(label.dest.y, hearts.dest.y, "level is below the health row");
        assert_eq!(label.dest.y, hunger.dest.y, "level is below the hunger row");
        assert_eq!(label.dest.x + label.dest.w / 2.0, 240.0);
        let bar = named(&nodes, "empty_progress_bar")
            .into_iter()
            .find(|node| node.dest.y > 200.0)
            .expect("xp bar");
        assert!(label.dest.y + label.dest.h < bar.dest.y);
    }
}

// Built-in HUD geometry on a 480x270 GUI-px screen (centre 240, bottom 270).
#[test]
fn java_pack_places_the_hud() {
    let Some(nodes) = render_with(&model(), true) else {
        return;
    };
    dump(&nodes);
    // Status rows: hearts from (c-91, H-39); hunger's right end at c+90.
    assert_eq!(at(custom(&nodes, "heart_renderer")[0]), [149.0, 231.0]);
    assert_eq!(at(custom(&nodes, "armor_renderer")[0]), [149.0, 231.0]);
    assert_eq!(at(custom(&nodes, "hunger_renderer")[0]), [330.0, 231.0]);
    // Hotbar cells from c-90 at H-22, the selection frame 1 px out.
    let slots = custom(&nodes, "hotbar_renderer");
    assert_eq!(slots.len(), 9);
    assert_eq!(at(slots[0]), [150.0, 248.0]);
    assert_eq!(at(slots[8]), [310.0, 248.0]);
    let selected = named(&nodes, "hotbar_slot_selected_image");
    assert_eq!(selected.len(), 1);
    assert_eq!(at(selected[0]), [188.0, 247.0]);
    let icons = custom(&nodes, "inventory_item_renderer");
    assert_eq!(at(icons[0]), [152.0, 251.0]);
    // XP bar 182x5 at H-29; level text shares the status row, outlined four ways.
    let bar = named(&nodes, "empty_progress_bar")
        .into_iter()
        .find(|node| node.dest.y > 200.0)
        .expect("xp bar");
    assert_eq!([bar.dest.x, bar.dest.y], [149.0, 241.0]);
    let level: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(&node.draw, Draw::Text { text, .. } if text == "7"))
        .collect();
    assert_eq!(level.len(), 5);
    assert_eq!(level[4].dest.x + level[4].dest.w / 2.0, 240.0);
    assert_eq!(level[4].dest.y, custom(&nodes, "heart_renderer")[0].dest.y);
    // Count text right-aligned in the cell: right edge at cell + 19, top at +12.
    let count = text_node(&nodes, "12");
    assert_eq!(count.dest.x + count.dest.w, 169.0);
    assert_eq!(count.dest.y, 260.0);
    // Titles: 4x from H/2-40, subtitle 2x from H/2+10, action bar top at H-72.
    let title = text_node(&nodes, "Title");
    assert_eq!([title.dest.y, title.dest.h], [95.0, 36.0]);
    assert_eq!(title.dest.x + title.dest.w / 2.0, 240.0);
    assert_eq!(text_node(&nodes, "Sub").dest.y, 145.0);
    assert_eq!(text_node(&nodes, "bar").dest.y, 198.0);
    // Chat: one line whose bottom sits 40 px above the screen bottom, text x 4.
    let chat = text_node(&nodes, "hello");
    assert_eq!([chat.dest.x, chat.dest.y + 9.0], [4.0, 230.0]);
    // Boss bar: name from y 3, bar at y 12, both centred.
    assert_eq!(text_node(&nodes, "Wither").dest.y, 3.0);
    let boss = named(&nodes, "empty_progress_bar")
        .into_iter()
        .find(|node| node.dest.y < 50.0)
        .expect("boss bar");
    assert_eq!(boss.dest.y, 12.0);
    // Sidebar: two rows, box bottom at H/2 + 18/3, right edge at W-1.
    let kills = text_node(&nodes, "Kills");
    let steve = text_node(&nodes, "Steve");
    let score = text_node(&nodes, "3");
    assert!(
        (steve.dest.y + 18.0 - 141.0).abs() < 1e-3,
        "{}",
        steve.dest.y
    );
    assert!((kills.dest.y - (steve.dest.y - 9.0)).abs() < 1e-6);
    assert_eq!(score.dest.x + score.dest.w, 477.0);
    // Titles and the action bar draw without Bedrock's text background.
    assert!(!nodes.iter().any(|node| matches!(
        &node.draw,
        Draw::Sprite { texture, .. } if texture == "textures/ui/hud_tip_text_background"
    )));
    assert_eq!(
        text_node(&nodes, "Alex").dest.x,
        text_node(&nodes, "Steve").dest.x
    );
}

// The tip text rides its own `hud_tip_text` slot: it draws beside, not over, the
// action bar's text, above the hotbar and inside its own factory instance.
#[test]
fn vanilla_hud_draws_the_tip_text_in_its_own_slot() {
    let Some(render) = render_full(&model(), false) else {
        return;
    };
    let nodes = &render.nodes;
    dump(nodes);
    let tip = text_node(nodes, "tip");
    let bar = text_node(nodes, "bar");
    assert_ne!(
        (tip.dest.x, tip.dest.y),
        (bar.dest.x, bar.dest.y),
        "the tip text must not take the action bar's position"
    );
    // The action bar's label sits above the hotbar's top edge, from its own slot.
    assert!(bar.dest.y < 248.0, "{:?}", bar.dest);
    // The tip text draws in the same region, from `hud_tip_text`'s own template.
    assert!(
        named(nodes, "hud_tip_text").len() + named(nodes, "popup_tip_text").len() > 0,
        "the tip text drew outside hud_tip_text: {:?}",
        named(nodes, "hud_tip_text")
    );
    // Its label keeps the tip text, and its fade is animated as one instance.
    assert!(
        tip.anim
            .as_ref()
            .is_some_and(|anim| !anim.alpha.is_empty()),
        "the tip text has no fade: {:?}",
        tip.anim
    );
}

/// A vanilla tip text of several lines keeps its last line on the tip position,
/// with the earlier lines stacked above it.
#[test]
fn a_multiline_tip_text_keeps_its_last_line_on_the_tip_position() {
    for java in [false, true] {
        let mut multiline = model();
        multiline.tip = Some(Timed {
            text: "LUMINE PROXY\nJava".into(),
            born: 0.0,
        });
        let Some(render) = render_full(&multiline, java) else {
            return;
        };
        let nodes = &render.nodes;
        dump(nodes);
        let expected = format!("LUMINE PROXY\nJava");
        let block = nodes
            .iter()
            .find(|node| matches!(&node.draw, Draw::Text { text, .. } if *text == expected))
            .expect("the tip text drew both lines");
        // Two lines of nine GUI px, laid out one above the other.
        assert_eq!((block.dest.w, block.dest.h), (72.0, 18.0));
        let one = model();
        let Some(single) = render_full(&one, java) else {
            return;
        };
        let bare = text_node(&single.nodes, "tip");
        // The block's bottom is what the anchoring holds, so the earlier line
        // is pushed up and the last line stays on the tip position.
        let block_bottom = block.dest.y + block.dest.h;
        let bare_bottom = bare.dest.y + bare.dest.h;
        assert!(
            (block_bottom - bare_bottom).abs() < 1e-6,
            "the last tip line moved off the tip position: {block:?} vs {bare:?}"
        );
        // The two-line block is one line taller, anchored on the same bottom
        // edge: its extra height grows upward, above the tip position.
        assert!(
            (block.dest.h - bare.dest.h - 9.0).abs() < 1e-6,
            "the extra tip line did not grow the block: {block:?} vs {bare:?}"
        );
        // Both stay centred on the tip text's column.
        let center = |node: &DrawNode| node.dest.x + node.dest.w / 2.0;
        assert!(
            (center(&block) - center(&bare)).abs() < 1e-6,
            "tip lines are not centred alike: {block:?} vs {bare:?}"
        );
        // It stays clear of the hotbar row below it.
        let hotbar = nodes
            .iter()
            .filter(|node| node.dest.y > 240.0 && node.dest.y < 250.0)
            .map(|node| at(node))
            .min_by(|[_, a], [_, b]| a.total_cmp(b))
            .expect("a hotbar element");
        assert!(
            block.dest.y + block.dest.h <= hotbar[1],
            "the tip text overlaps the hotbar: {block:?} vs {hotbar:?}"
        );
    }
}

// A resource pack that retargets `hud_tip_text` moves the tip text with it; the
// Lumine UI pack anchors it to the top right, so it must leave the hotbar area.
#[test]
fn a_pack_retargeting_hud_tip_text_moves_the_tip_text() {
    let Some(dir) = pack() else {
        return;
    };
    // The Lumine UI pack's `hud_tip_text`, with its template flattened onto the
    // vanilla one by whole-property selection.
    let pack = r##"{
        "namespace": "hud",
        "hud_tip_text": {
            "type": "image",
            "texture": "",
            "alpha": 0,
            "size": [ "100%c + 12", "100%c + 4px" ],
            "offset": [ "0px", "24px" ],
            "anchor_from": "top_right",
            "anchor_to": "top_right",
            "$wait_duration|default": 1,
            "$destroy_id|default": "popup_tip_text",
            "controls": [
              {
                "item_text_label": {
                  "type": "label",
                  "layer": 1,
                  "color": "$tool_tip_text",
                  "text": "#text",
                  "shadow": true,
                  "alpha": "@hud.hud_tip_text_alpha_out",
                  "text_alignment": "right",
                  "bindings": [
                    { "binding_name": "#tip_text", "binding_name_override": "#text" }
                  ]
                }
              }
            ]
        }
    }"##;
    let mut catalog = catalog_for(&dir);
    catalog.apply_pack([("ui/hud_screen.json", pack.as_bytes())]);
    let model = self::model();
    let textures = PackTextures::new(dir);
    let env = LayoutEnv {
        text: &FixedText,
        textures: &textures,
    };
    let render = render_screen(
        HUD_SCREEN,
        &catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(&model),
        [480.0, 270.0],
        &env,
        &ViewState::default(),
    )
    .expect("hud renders with the pack overlay");
    let nodes = &render.nodes;
    dump(nodes);
    let tip = text_node(nodes, "tip");
    let bar = text_node(nodes, "bar");
    // The pack anchors it to the top right: the text sits in that corner and
    // nowhere near the action bar's row above the hotbar.
    assert!(
        tip.dest.y < 60.0,
        "tip text left the top-right anchor: {:?}",
        tip.dest
    );
    assert!(
        tip.dest.x + tip.dest.w > 480.0 * 0.6,
        "tip text is not right-aligned: {:?}",
        tip.dest
    );
    assert!(
        (tip.dest.y - bar.dest.y).abs() > 20.0,
        "tip text ignored the pack: {:?} vs {:?}",
        tip.dest,
        bar.dest
    );
}

#[test]
fn ignored_instances_and_unanswered_collection_flags_draw_nothing() {
    let globals = br#"{}"#;
    let defs = br#"{ "ui_defs": ["ui/s.json"] }"#;
    let screen = br##"{
        "namespace": "s",
        "overlay": { "type": "image", "texture": "textures/ui/Black", "ignored": true },
        "kept": { "type": "image", "texture": "textures/ui/White", "size": [4, 4] },
        "root": {
            "type": "panel",
            "controls": [
                { "titles": { "type": "panel", "factory": { "name": "title_factory",
                    "control_ids": { "overlay": "overlay@s.overlay", "kept": "kept@s.kept" } } } },
                { "cells": { "type": "stack_panel", "collection_name": "slots",
                    "factory": { "name": "cells", "control_name": "s.cell" } } }
            ]
        },
        "cell": { "type": "image", "texture": "textures/ui/lock", "size": [4, 4],
            "bindings": [ { "binding_name": "#locked", "binding_name_override": "#visible",
                "binding_type": "collection", "binding_collection_name": "slots" } ] }
    }"##;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", globals.as_slice()),
        ("ui/_ui_defs.json", defs.as_slice()),
        ("ui/s.json", screen.as_slice()),
    ])
    .unwrap();
    let mut data = json_ui::DataSource::new();
    data.set_strict(true);
    data.set_factory(
        "title_factory",
        vec![
            json_ui::FactoryItem::new("overlay", 0.0),
            json_ui::FactoryItem::new("kept", 0.0),
        ],
    );
    data.set_collection("slots", vec![json_ui::CollectionItem::default(); 3]);
    let context = Context::desktop();
    let root = json_ui::resolve(&catalog, "s.root", &context)
        .control
        .unwrap();
    let library = json_ui::CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let bound = json_ui::bind(&root, &data, &library);
    let textures = PackTextures::new(PathBuf::new());
    let env = LayoutEnv {
        text: &FixedText,
        textures: &textures,
    };
    let render = json_ui::render_bound(bound, [100.0, 100.0], &env, &ViewState::default());
    let drawn: Vec<&str> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } => Some(texture.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(drawn, ["textures/ui/White"]);
}

#[test]
fn java_pack_keeps_the_position_and_days_lines_top_left() {
    let mut model = model();
    model.player_position = Some("Position: 1, 64, -3".into());
    model.days_played = Some("Days played: 2".into());
    model.text_background_alpha = 0.5;
    let Some(nodes) = render_with(&model, true) else {
        return;
    };
    dump(&nodes);
    let position = named(&nodes, "player_position_text");
    let days = named(&nodes, "number_of_days_played_text");
    assert_eq!(position.len(), 1);
    assert_eq!(days.len(), 1);
    assert!(matches!(&position[0].draw, Draw::Text { text, .. } if text == "Position: 1, 64, -3"));
    // Centred in vanilla's 40%-wide top-left column.
    assert!(position[0].dest.x < 480.0 * 0.4 && position[0].dest.y < 12.0);
    assert!(days[0].dest.y > position[0].dest.y);
    let backing = named(&nodes, "player_position");
    assert!(backing.iter().all(|node| (node.alpha - 0.5).abs() < 1e-6));
    // Off by default: neither line draws.
    let Some(nodes) = render_with(&self::model(), true) else {
        return;
    };
    assert!(named(&nodes, "player_position_text").is_empty());
    assert!(named(&nodes, "number_of_days_played_text").is_empty());
}
