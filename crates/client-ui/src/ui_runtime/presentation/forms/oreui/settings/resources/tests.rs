use super::*;
use crate::ui_runtime::presentation::{IconRef, forms::oreui::review_tests::paint};
use std::{collections::HashMap, sync::Arc};

#[test]
fn resource_sections_and_pack_details_reveal_height_instead_of_jumping() {
    use super::super::super::{motion::Surface, transitions::Transitions};
    let pack = pack();
    let mut snapshot = Snapshot {
        active: vec![pack.clone()],
        selection: vec![selection(&pack)],
        ..Snapshot::default()
    };
    let mut transitions = Transitions::default();
    let mut frame = |snapshot: &Snapshot, seconds: f64, animated: bool| {
        transitions.configure_motion(animated);
        transitions.begin_frame(None, false, seconds);
        let view = view(snapshot.clone());
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
        let font = crate::ui_runtime::presentation::tests::fixture_font();
        let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.surface = Surface::Settings(super::super::section_index(
            "global_texture_pack_forced_index",
        ));
        canvas.seconds = seconds;
        canvas.transitions = Some(&mut transitions);
        let mut content = Content {
            canvas: &mut canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
        let hits = std::mem::take(&mut canvas.hits);
        drop(canvas);
        transitions.end_frame();
        hits.into_iter()
            .find(|(action, _)| *action == command(Action::ToggleAvailable))
            .unwrap()
            .1
            .min()
            .y()
    };
    let closed_details = frame(&snapshot, 0.0, true);
    snapshot.details_expanded = Some((true, 0));
    assert_eq!(frame(&snapshot, 1.0, true), closed_details);
    let middle = frame(&snapshot, 1.06, true);
    let expanded_details = frame(&snapshot, 1.3, true);
    assert!(closed_details < middle && middle < expanded_details);
    snapshot.active_expanded = false;
    assert_eq!(frame(&snapshot, 2.0, true), expanded_details);
    let collapsing = frame(&snapshot, 2.06, true);
    let collapsed = frame(&snapshot, 2.3, true);
    assert!(collapsed < collapsing && collapsing < expanded_details);
    snapshot.active_expanded = true;
    assert_eq!(frame(&snapshot, 3.0, true), collapsed);
    assert!(frame(&snapshot, 3.06, true) > collapsed);
    assert_eq!(frame(&snapshot, 4.0, false), expanded_details);
}

fn pack() -> resource_pack::InstalledPack {
    resource_pack::InstalledPack {
        id: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
        name: "Copper Test Pack".into(),
        description: "Copper-colored blocks and tools.".into(),
        version: [1, 0, 0],
        min_engine_version: None,
        revision: 0,
        subpacks: vec![
            resource_pack::Subpack {
                folder: "low".into(),
                name: "Low resolution".into(),
                memory_tier: 0,
            },
            resource_pack::Subpack {
                folder: "high".into(),
                name: "High resolution".into(),
                memory_tier: 2,
            },
        ],
    }
}

fn selection(pack: &resource_pack::InstalledPack) -> resource_pack::ActivePack {
    resource_pack::ActivePack {
        id: pack.id,
        revision: pack.revision,
        subpack: "low".into(),
    }
}

fn view(snapshot: Snapshot) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.global_resources = Arc::new(snapshot);
    view
}

fn render(view: &MenuView, width: f32) -> (Vec<(MenuAction, ui::UiRect)>, Vec<ui::UiNode>) {
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [100.0, 100.0 + width],
            column_width: width,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
    });
    (hits, nodes)
}

fn labels(nodes: &[ui::UiNode]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
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

#[test]
fn active_pack_details_expand_without_giving_the_base_pack_remove_or_sort_actions() {
    let pack = pack();
    let mut snapshot = Snapshot {
        active: vec![pack.clone()],
        selection: vec![selection(&pack)],
        ..Snapshot::default()
    };
    let (closed, nodes) = render(&view(snapshot.clone()), 800.0);
    assert!(
        closed
            .iter()
            .any(|(action, _)| *action == command(Action::ReadMore(true, 0)))
    );
    assert!(
        !closed
            .iter()
            .any(|(action, _)| *action == command(Action::Deactivate(0)))
    );
    assert!(
        labels(&nodes)
            .iter()
            .any(|label| label == "Active packs (2)")
    );
    snapshot.details_expanded = Some((true, 0));
    let (opened, nodes) = render(&view(snapshot), 800.0);
    assert!(
        opened
            .iter()
            .any(|(action, _)| *action == command(Action::Deactivate(0)))
    );
    assert!(!opened.iter().any(|(action, _)| matches!(
        action,
        MenuAction::GlobalResources(Action::MoveDown(_) | Action::Deactivate(1))
    )));
    assert!(
        labels(&nodes)
            .iter()
            .any(|label| label == &pack.description)
    );
    assert!(
        labels(&nodes)
            .iter()
            .any(|label| label == "Vanilla Textures")
    );
}

#[test]
fn expanded_card_background_stays_behind_its_interactive_face_in_the_ui_tree() {
    let pack = pack();
    let view = view(Snapshot {
        active: vec![pack.clone()],
        selection: vec![selection(&pack)],
        details_expanded: Some((true, 0)),
        ..Snapshot::default()
    });
    let (hits, nodes) = render(&view, 800.0);
    let bounds = hits
        .iter()
        .find(|(action, _)| *action == command(Action::Deactivate(0)))
        .unwrap()
        .1;
    let centre = [
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    ];
    let draw = ui::UiTree::new(nodes).unwrap().build_draw_list().unwrap();
    let topmost = draw
        .vertices
        .as_chunks::<4>()
        .0
        .iter()
        .rfind(|quad| {
            let min = [
                quad.iter()
                    .map(|vertex| vertex.position[0])
                    .fold(f32::INFINITY, f32::min),
                quad.iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min),
            ];
            let max = [
                quad.iter()
                    .map(|vertex| vertex.position[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                quad.iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::NEG_INFINITY, f32::max),
            ];
            centre[0] >= min[0] && centre[0] <= max[0] && centre[1] >= min[1] && centre[1] <= max[1]
        })
        .unwrap();
    assert_ne!(
        topmost[0].color,
        theme::NEUTRAL80.fill,
        "the detail backdrop must not cover the button face"
    );
}

#[test]
fn expanded_pack_details_leave_equal_gutters_for_text_and_controls() {
    for width in [300.0, 800.0] {
        for active in [false, true] {
            let pack = pack();
            let snapshot = Snapshot {
                active: if active {
                    vec![pack.clone()]
                } else {
                    Vec::new()
                },
                available: if active {
                    Vec::new()
                } else {
                    vec![pack.clone()]
                },
                selection: if active {
                    vec![selection(&pack)]
                } else {
                    Vec::new()
                },
                details_expanded: Some((active, 0)),
                ..Snapshot::default()
            };
            let (hits, nodes) = render(&view(snapshot), width);
            let header = hits
                .iter()
                .find(|(action, _)| *action == command(Action::ReadMore(active, 0)))
                .unwrap()
                .1;
            for (action, bounds) in &hits {
                if !matches!(
                    action,
                    MenuAction::GlobalResources(
                        Action::Activate(_) | Action::Deactivate(_) | Action::Settings(_)
                    )
                ) {
                    continue;
                }
                let left = bounds.min().x() - header.min().x();
                let right = header.max().x() - bounds.max().x();
                assert!(
                    left > 0.0 && right > 0.0,
                    "detail controls need side padding"
                );
                assert!((left - right).abs() < 0.01, "detail gutters must be equal");
                assert!(bounds.min().y() > header.max().y());
            }
            let description = nodes
                .iter()
                .find(|node| {
                    matches!(node.visual(), ui::UiVisual::Text { layout, .. }
                        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>()
                            .starts_with(pack.description.split_whitespace().next().unwrap()))
                })
                .unwrap()
                .bounds();
            assert!(description.min().x() > header.min().x());
            assert!(description.max().x() < header.max().x());
            assert!(description.min().y() > header.max().y());
            if active {
                let accordion = hits
                    .iter()
                    .find(|(action, _)| *action == command(Action::ToggleAvailable))
                    .unwrap()
                    .1;
                assert_eq!(accordion.min().x(), header.min().x());
                assert_eq!(accordion.max().x(), header.max().x());
                assert!(accordion.min().y() > description.max().y());
            }
        }
    }
}

#[test]
fn apply_is_available_only_for_acknowledged_unapplied_changes() {
    let pack = pack();
    for (applied, busy, enabled) in [
        (None, false, false),
        (Some(Vec::new()), false, true),
        (Some(vec![selection(&pack)]), false, false),
        (Some(Vec::new()), true, false),
    ] {
        let view = view(Snapshot {
            active: vec![pack.clone()],
            selection: vec![selection(&pack)],
            applied_selection: applied,
            busy,
            ..Snapshot::default()
        });
        let (hits, _) = render(&view, 800.0);
        assert_eq!(
            hits.iter()
                .any(|(action, _)| *action == command(Action::Apply)),
            enabled
        );
    }
}

#[test]
fn compact_controls_wrap_and_busy_cards_keep_their_detail_toggle() {
    let pack = pack();
    let view = view(Snapshot {
        available: vec![pack],
        details_expanded: Some((false, 0)),
        busy: true,
        ..Snapshot::default()
    });
    let (hits, _) = render(&view, 300.0);
    assert!(
        hits.iter()
            .any(|(action, _)| *action == command(Action::ReadMore(false, 0)))
    );
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::GlobalResources(Action::Import | Action::Activate(_))
    )));
    let view = view.clone();
    let mut snapshot = (*view.global_resources).clone();
    snapshot.busy = false;
    let (hits, _) = render(&super::tests::view(snapshot), 300.0);
    for (_, bounds) in &hits {
        assert!(bounds.min().x() >= 100.0 && bounds.max().x() <= 400.0);
    }
    let import = hits
        .iter()
        .find(|(action, _)| *action == command(Action::Import))
        .unwrap()
        .1;
    let activate = hits
        .iter()
        .find(|(action, _)| *action == command(Action::Activate(0)))
        .unwrap()
        .1;
    assert!(activate.min().y() >= import.max().y());
}

#[test]
fn imported_pack_thumbnail_uses_the_ready_artwork_without_stretching() {
    let pack = pack();
    let key = (pack.id.to_string(), pack.revision);
    let view = view(Snapshot {
        available: vec![pack],
        icons: [(key, "pack-art".into())].into(),
        ..Snapshot::default()
    });
    let artwork = HashMap::from([(
        "pack-art".into(),
        IconRef {
            page: 9,
            uv: [10, 20, 74, 52],
            glint: false,
        },
    )]);
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let font = crate::ui_runtime::presentation::tests::fixture_font();
    let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    {
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.artwork = Some(&artwork);
        let mut content = Content {
            canvas: &mut canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
    }
    let image = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 9,
                    ..
                }
            )
        })
        .unwrap();
    assert_eq!(image.bounds().width(), 2.0 * image.bounds().height());
}

#[test]
fn variant_picker_filters_unsupported_tiers_and_owns_all_input() {
    let pack = pack();
    let view = view(Snapshot {
        active: vec![pack.clone()],
        selection: vec![selection(&pack)],
        settings: Some(0),
        memory_tier: 0,
        ..Snapshot::default()
    });
    let (scrolls, hits, _) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
        assert!(draw_picker(canvas, &view, [1280.0, 720.0]).unwrap());
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == command(Action::Subpack(0)))
    );
    assert!(
        !hits
            .iter()
            .any(|(action, _)| *action == command(Action::Subpack(1)))
    );
    assert!(hits.iter().all(|(action, _)| matches!(
        action,
        MenuAction::GlobalResources(Action::CloseSettings | Action::Subpack(_))
    )));
    assert!(
        scrolls
            .iter()
            .all(|area| area.key.starts_with("oreui_pack_variant/"))
    );
}

#[test]
fn filtered_variants_preserve_the_declared_index_and_root_identity() {
    let mut pack = pack();
    pack.subpacks.swap(0, 1);
    let mut snapshot = Snapshot {
        active: vec![pack.clone()],
        selection: vec![selection(&pack)],
        settings: Some(0),
        memory_tier: 0,
        ..Snapshot::default()
    };
    let picker = variant_picker(&snapshot).unwrap();
    assert_eq!(picker.actions, [command(Action::Subpack(1))]);
    assert_eq!(picker.selected, 0);
    snapshot.selection[0].subpack.clear();
    let picker = variant_picker(&snapshot).unwrap();
    assert_eq!(
        picker.selected,
        usize::MAX,
        "root textures must not mark a declared variant selected"
    );
    snapshot.active[0].subpacks[1].memory_tier = 2;
    assert!(variant_picker(&snapshot).is_none());
    snapshot.details_expanded = Some((true, 0));
    let (hits, nodes) = render(&view(snapshot), 800.0);
    assert!(
        !hits
            .iter()
            .any(|(action, _)| *action == command(Action::Settings(0)))
    );
    assert!(
        labels(&nodes)
            .iter()
            .any(|label| label == "Variant: Default textures")
    );
}
