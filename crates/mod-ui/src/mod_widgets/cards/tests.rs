//! Card previews exercise the production painter without game carriers or a window.

use super::*;
use crate::test_support::sample_panel;
use json_ui::Context;
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceValue};
use ui_engine::{
    engine::screen::ScreenRegistry,
    sources::{ImageWork, RemoteImage, RemoteSource},
    text_metrics::TextMetrics,
};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);

struct NoImages;
impl RemoteSource for NoImages {
    /// Test cards do not download artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

struct Harness {
    engine: FormEngine,
    font: Arc<assets::RuntimeFontCatalog>,
    layouts: ui::TextLayoutCache,
    nodes: Vec<UiNode>,
}

impl Harness {
    /// Builds the real engine with an empty atlas and a small redistributable font fixture.
    fn new() -> Self {
        static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
        let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
        Self {
            engine: FormEngine::new(
                Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
                json_ui::Catalog::default(),
                0,
                REGISTRY.get_or_init(|| ScreenRegistry::new(Context::default())),
                Arc::new(NoImages),
                ImageWork::inline(),
            ),
            font: ui_engine::test_support::fixture_font(),
            layouts: ui::TextLayoutCache::new(1, 1024 * 1024),
            nodes: Vec::new(),
        }
    }

    /// Paints a panel or the gameplay HUD at the same scale and clock.
    fn paint(&mut self, ui: &mut ModUi, panel: bool) {
        self.nodes.clear();
        let mut next = 1;
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics: TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area: ui::SafeArea::ZERO,
            content: [1280., 720.],
            translate: &|_| None,
            language: [0; 3],
            now: 0.,
            nodes: &mut self.nodes,
            next: &mut next,
            renderers: &mut [],
        };
        if ui.mod_hud_editor_open() {
            let icons = ui.mod_hud_editor_icons(|_, _| None).unwrap();
            ui.paint_mod_hud_editor(Some(&self.engine), &mut frame, &icons);
        } else if panel {
            ui.paint_mod_panel(Some(&self.engine), &mut frame);
        } else {
            let icons = ui.mod_widget_icons(|_, _| None).unwrap();
            ui.paint_mod_widgets(Some(&self.engine), &mut frame, &icons);
        }
    }

    /// Resolves actual painted vertices and scissors through the normal retained geometry path.
    fn drawn(&self) -> ui::UiDrawList {
        ui::RetainedDraw::build(
            &self.nodes,
            ui_engine::engine::rect(0., 0., 1280., 720.).unwrap(),
            ui::UiScale::default(),
            ui::SafeArea::ZERO,
            ui::TextEffects::default(),
        )
        .unwrap()
        .draw_list()
        .clone()
    }
}

/// Produces easily distinguishable cards with a colored cell and live text.
fn hud(count: usize) -> Hud {
    serde_json::from_value(json!({"cards":(0..count).map(|index|json!({
        "id":format!("card_{index}"), "width":40,"rows":[],"background_opacity":0,
        "cells":[{"rect":[0,0,40,20],"label":"0","background":[1,0,0,1]}]
    })).collect::<Vec<_>>()}))
    .unwrap()
}

/// Builds an authored surface with one standalone preview and optional virtualized tiles.
fn panel(collection: bool) -> ui::mod_panel::Panel {
    let mut panel = sample_panel();
    panel.controls.clear();
    panel.surface = Some(Surface {
        screen:"preview.screen".into(), document:json!({"namespace":"preview","screen":{"type":"screen","size":["100%","100%"],"controls":[
            {"stage":{"type":"custom","renderer":ui::mod_panel::hud_card::RENDERER,"size":[80,60],"offset":[20,30],"anchor_from":"top_left","anchor_to":"top_left","bindings":[{"binding_name":"#selected","binding_name_override":"#card_id"}]}}
        ]}}).to_string(),
        bindings: [("#selected".into(),SurfaceValue::Text("card_0".into()))].into(),
        previews:Default::default(),transitions:Default::default(), collections:Default::default(),
    });
    if collection {
        let surface = panel.surface.as_mut().unwrap();
        let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
        document["screen"]["controls"].as_array_mut().unwrap().push(json!({"grid":{"type":"panel","cinnabar_collection":"cards","offset":[120,20],"anchor_from":"top_left","anchor_to":"top_left"}}));
        surface.document = document.to_string();
        let template = Surface { screen:"tile.root".into(), document:json!({"namespace":"tile","root":{"type":"panel","size":[100,60],"clips_children":true,"controls":[
            {"preview":{"type":"custom","renderer":ui::mod_panel::hud_card::RENDERER,"size":[100,60],"scale":0.5,"bindings":[{"binding_name":"#item_card","binding_name_override":"#card_id"}]}}
        ]}}).to_string(), bindings:Default::default(), previews:Default::default(),transitions:Default::default(),collections:Default::default() };
        let definition = ui::mod_panel::CollectionDefinition {
            template,
            columns: 4,
            viewport: [408., 120.],
            items: (0..32)
                .map(|index| ui::mod_panel::CollectionItem {
                    values: [(
                        "#item_card".into(),
                        SurfaceValue::Text(format!("card_{index}")),
                    )]
                    .into(),
                    ..Default::default()
                })
                .collect(),
        };
        panel.surface.as_mut().unwrap().collections.insert(
            "cards".into(),
            ui::mod_panel::SurfaceCollection {
                definition: Arc::new(definition),
                order: (0..32).collect(),
            },
        );
    }
    panel.validate().unwrap();
    panel
}

/// Publishes identical gameplay and owner-scoped output and opens the test surface.
fn installed(hud: &Hud, panel: &ui::mod_panel::Panel) -> ModUi {
    let mut ui = ModUi::default();
    ui.set_mod_hud_sources([(OWNER, 1, hud)].into_iter())
        .unwrap();
    ui.set_mod_hud(Some(hud)).unwrap();
    ui.set_mod_panel(OWNER, Some(panel)).unwrap();
    ui.set_mod_panel_open(true);
    ui
}

#[test]
fn hud_card_preview_centers_scales_clips_and_reuses_gameplay_output() {
    let hud = hud(1);
    let mut ui = installed(&hud, &panel(false));
    let mut harness = Harness::new();
    harness.paint(&mut ui, false);
    let shared = Arc::clone(&ui.hud_sources.get_mut(OWNER).unwrap().cards[0].nodes);
    harness.paint(&mut ui, true);
    assert!(Arc::ptr_eq(
        &shared,
        &ui.hud_sources.get_mut(OWNER).unwrap().cards[0].nodes
    ));
    let bounds = harness.drawn().painted_bounds().unwrap();
    assert_eq!(
        [
            bounds.min().x(),
            bounds.min().y(),
            bounds.width(),
            bounds.height()
        ],
        [80., 100., 80., 40.]
    );
    let mut panel = panel(false);
    let surface = panel.surface.as_mut().unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    let node = &mut document["screen"]["controls"][0]["stage"];
    node["scale"] = json!(5.);
    node["alignment"] = json!([0, 0]);
    surface.document = document.to_string();
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    assert!(
        harness
            .drawn()
            .batches
            .iter()
            .all(|batch| batch.clip.max().x() <= 200. && batch.clip.max().y() <= 180.)
    );
}

#[test]
fn hud_card_preview_updates_values_without_catalog_rebuild_and_rejects_other_owners() {
    let mut hud = hud(1);
    hud.cards[0].preview_only = true;
    let mut ui = installed(&hud, &panel(false));
    let mut harness = Harness::new();
    harness.paint(&mut ui, false);
    assert!(harness.drawn().vertices.is_empty());
    harness.paint(&mut ui, true);
    let catalog = Arc::clone(&ui.hud_sources.get_mut(OWNER).unwrap().catalog);
    let before = harness.drawn().vertices;
    hud.cards[0].cells[0].background = [0., 1., 0., 1.];
    ui.set_mod_hud_sources([(OWNER, 2, &hud)].into_iter())
        .unwrap();
    harness.paint(&mut ui, true);
    assert_ne!(before, harness.drawn().vertices);
    assert!(Arc::ptr_eq(
        &catalog,
        &ui.hud_sources.get_mut(OWNER).unwrap().catalog
    ));
    ui.set_mod_panel(PanelUiOwner::Registration(9), Some(&panel(false)))
        .unwrap();
    harness.paint(&mut ui, true);
    assert!(harness.drawn().vertices.is_empty());
    for _ in 0..3 {
        harness.paint(&mut ui, true);
    }
    assert_eq!(ui.hud_preview_unknowns(), 1);
}

#[test]
fn hud_card_preview_virtualized_tiles_retain_steady_state_card_output() {
    let hud = hud(32);
    let mut ui = installed(&hud, &panel(true));
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    let paints: Vec<_> = source.cards.iter().map(|card| card.paints).collect();
    assert!(paints.iter().sum::<usize>() <= ui::mod_panel::SurfaceCollection::INITIAL_WINDOW + 1);
    assert!(paints.iter().any(|paint| *paint == 0));
    assert!(paints.iter().sum::<usize>() > 1);
    for _ in 0..8 {
        harness.paint(&mut ui, true);
    }
    assert_eq!(
        paints,
        ui.hud_sources
            .get_mut(OWNER)
            .unwrap()
            .cards
            .iter()
            .map(|card| card.paints)
            .collect::<Vec<_>>()
    );
}

#[test]
fn hud_card_preview_twenty_four_visible_instances_do_not_rebuild_each_frame() {
    let hud = hud(32);
    let mut panel = panel(false);
    let surface = panel.surface.as_mut().unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    document["screen"]["controls"] = json!((0..24).map(|index|json!({format!("preview_{index}"):{
        "type":"custom","renderer":ui::mod_panel::hud_card::RENDERER,
        "card_id":format!("card_{index}"),"size":[80,40],"offset":[(index%6)*90,(index/6)*60],
        "anchor_from":"top_left","anchor_to":"top_left"
    }})).collect::<Vec<_>>());
    surface.document = document.to_string();
    let mut ui = installed(&hud, &panel);
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    let catalog = Arc::clone(&source.catalog);
    let snapshots: Vec<_> = source
        .cards
        .iter()
        .map(|card| Arc::clone(&card.nodes))
        .collect();
    assert_eq!(
        source.cards.iter().map(|card| card.paints).sum::<usize>(),
        24
    );
    for _ in 0..12 {
        harness.paint(&mut ui, true);
    }
    let (_, allocation) = test_alloc::measure(|| harness.paint(&mut ui, true));
    assert_eq!(
        allocation.calls, 0,
        "steady-state preview paint allocated {allocation:?}"
    );
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    assert!(Arc::ptr_eq(&catalog, &source.catalog));
    for (before, card) in snapshots.iter().zip(&source.cards) {
        assert!(Arc::ptr_eq(before, &card.nodes));
    }
    assert_eq!(
        source.cards.iter().map(|card| card.paints).sum::<usize>(),
        24
    );
}

#[test]
fn hud_card_preview_same_id_stays_with_its_panel_owner_and_revokes_immediately() {
    let first = hud(1);
    let mut second = first.clone();
    second.cards[0].cells[0].background = [0., 0., 1., 1.];
    let other = PanelUiOwner::Registration(12);
    let mut ui = installed(&first, &panel(false));
    ui.set_mod_hud_sources([(OWNER, 1, &first), (other, 2, &second)].into_iter())
        .unwrap();
    ui.set_mod_panel(other, Some(&panel(false))).unwrap();
    ui.set_mod_panel_open(true);
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    assert!(
        harness
            .drawn()
            .vertices
            .iter()
            .any(|vertex| vertex.color == [0, 0, 255, 255])
    );
    assert!(
        !harness
            .drawn()
            .vertices
            .iter()
            .any(|vertex| vertex.color == [255, 0, 0, 255])
    );
    ui.set_mod_hud_sources([(OWNER, 1, &first)].into_iter())
        .unwrap();
    harness.paint(&mut ui, true);
    assert!(harness.drawn().vertices.is_empty());
}

#[derive(Clone, Default)]
struct UnknownWarnings(Arc<std::sync::atomic::AtomicUsize>);
impl tracing::Subscriber for UnknownWarnings {
    /// Restricts this subscriber to the current test thread's events.
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    /// The painter does not require span storage in this log-counting fixture.
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    /// Span fields are irrelevant to warning counts.
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    /// Span links are irrelevant to warning counts.
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    /// Counts the unknown-card warning independently of other painter diagnostics.
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message<'a>(&'a std::sync::atomic::AtomicUsize);
        impl tracing::field::Visit for Message<'_> {
            /// Identifies the warning by its public message field.
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" && format!("{value:?}") == "unknown HUD preview card" {
                    self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        event.record(&mut Message(&self.0));
    }
    /// No span stack is needed for this event-only fixture.
    fn enter(&self, _: &tracing::span::Id) {}
    /// No span stack is needed for this event-only fixture.
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn hud_card_preview_unknown_id_paints_nothing_and_warns_once() {
    let mut panel = panel(false);
    panel
        .surface
        .as_mut()
        .unwrap()
        .bindings
        .insert("#selected".into(), SurfaceValue::Text("missing".into()));
    let mut ui = installed(&hud(1), &panel);
    let mut harness = Harness::new();
    let warnings = UnknownWarnings::default();
    tracing::subscriber::with_default(warnings.clone(), || {
        for _ in 0..4 {
            harness.paint(&mut ui, true);
            assert!(harness.drawn().vertices.is_empty());
        }
    });
    assert_eq!(warnings.0.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(ui.hud_preview_unknowns(), 1);
}

#[test]
fn hud_card_preview_sibling_measurement_and_updates_do_not_invalidate_shared_output() {
    let mut hud: Hud = serde_json::from_value(json!({"cards":[
        {"id":"card_0","anchor":"top_right","rows":[{"label":"","value":"0"}],"text_plate":{}},
        {"id":"card_1","rows":[{"label":"","value":"2"}],"text_plate":{}}
    ]}))
    .unwrap();
    let mut ui = installed(&hud, &panel(false));
    let mut harness = Harness::new();
    harness.paint(&mut ui, false);
    let first = Arc::clone(&ui.hud_sources.get_mut(OWNER).unwrap().cards[0].nodes);
    harness.paint(&mut ui, true);
    assert!(Arc::ptr_eq(
        &first,
        &ui.hud_sources.get_mut(OWNER).unwrap().cards[0].nodes
    ));
    let before_origin = template::origin(
        &ui.hud_sources.get_mut(OWNER).unwrap().content.cards[0],
        [640., 360.],
    );
    hud.cards[1].rows[0].value = "20".into();
    ui.set_mod_hud_sources([(OWNER, 2, &hud)].into_iter())
        .unwrap();
    assert_eq!(
        before_origin,
        template::origin(
            &ui.hud_sources.get_mut(OWNER).unwrap().content.cards[0],
            [640., 360.]
        )
    );
    harness.paint(&mut ui, true);
    assert!(Arc::ptr_eq(
        &first,
        &ui.hud_sources.get_mut(OWNER).unwrap().cards[0].nodes
    ));
}

#[test]
fn hud_card_preview_invalid_source_batch_preserves_previous_owner_output() {
    let first = hud(2);
    let mut ui = installed(&first, &panel(false));
    let mut changed = first.clone();
    changed.cards.remove(0);
    let mut invalid = first.clone();
    invalid.cards[0].id = "bad.id".into();
    assert!(
        ui.set_mod_hud_sources(
            [
                (OWNER, 2, &changed),
                (PanelUiOwner::Registration(9), 3, &invalid)
            ]
            .into_iter()
        )
        .is_err()
    );
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    assert_eq!(source.card_index("card_0"), Some(0));
    assert_eq!(source.content.cards.len(), 2);
}

#[test]
fn hud_card_preview_transformed_into_view_keeps_its_artwork() {
    let mut panel = panel(false);
    let surface = panel.surface.as_mut().unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    let node = &mut document["screen"]["controls"][0]["stage"];
    node["offset"] = json!([-100, 30]);
    node["cinnabar_transform"] = json!({"scale":4,"origin":[-1,0]});
    surface.document = document.to_string();
    let mut ui = installed(&hud(1), &panel);
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    assert!(!harness.drawn().vertices.is_empty());
    let bounds = harness.drawn().painted_bounds().unwrap();
    assert!(bounds.min().x() >= 200.);
}

#[test]
fn hud_card_preview_only_keeps_artwork_in_explicit_editor_previews() {
    let mut hud = hud(1);
    hud.cards[0].preview_only = true;
    let mut ui = installed(&hud, &panel(false));
    ui.open_mod_hud_editor(&hud).unwrap();
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    assert!(
        harness
            .drawn()
            .vertices
            .iter()
            .any(|vertex| vertex.color == [255, 0, 0, 255])
    );
}

#[test]
fn hud_card_preview_gameplay_origin_stays_on_the_physical_pixel_grid() {
    for (offset, position, expected) in [
        ([0.25, 0.75], None, [0., 1.]),
        ([0., 0.], Some([0.333, 0.177]), [399., 120.]),
    ] {
        let mut hud = hud(1);
        hud.cards[0].offset = offset;
        hud.cards[0].position = position;
        let mut ui = installed(&hud, &panel(false));
        let mut harness = Harness::new();
        harness.paint(&mut ui, false);
        let bounds = harness.drawn().painted_bounds().unwrap();
        assert_eq!([bounds.min().x(), bounds.min().y()], expected);
    }
}
