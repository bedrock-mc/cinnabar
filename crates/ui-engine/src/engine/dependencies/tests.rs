use std::{cell::Cell, collections::BTreeSet};

use json_ui::{RectOut, StateGate, ViewState};
use serde_json::json;

use super::*;
use crate::engine::{
    Canvas,
    screen::{CustomDraw, PaintError},
};

/// A positioned primitive with no implicit dependency or interaction state.
fn node(key: &str, draw: Draw) -> DrawNode {
    let rect = RectOut {
        x: 0.0,
        y: 0.0,
        w: 16.0,
        h: 16.0,
    };
    DrawNode {
        name: key.into(),
        key: key.into(),
        dest: rect,
        clip: rect,
        layer: 0,
        alpha: 1.0,
        anim: None,
        draw,
        gates: Vec::new(),
    }
}

/// A sprite whose logical path can differ from its control key.
fn sprite(key: &str, path: &str) -> DrawNode {
    node(
        key,
        Draw::Sprite {
            texture: path.into(),
            uv: json_ui::UvRect::full(),
            color: [255; 4],
            filter: Default::default(),
            repeat: None,
        },
    )
}

/// A custom control carrying its actual bound property bag.
fn custom(key: &str, renderer: &str, data: serde_json::Value) -> DrawNode {
    node(
        key,
        Draw::Custom {
            renderer: renderer.into(),
            data: serde_json::from_value(data).unwrap(),
        },
    )
}

/// Copies a request's paths only for stable, order-independent fixture comparisons.
fn requested(base: &Dependencies<'_>, art: ScreenArt<'_>) -> BTreeSet<String> {
    paths(base, art, &[], &mut |_| {})
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn retained_discovery_keeps_gated_tooltip_and_banner_paths_without_revisiting_nodes() {
    let mut hover = sprite("/button/hover", "textures/hover");
    hover.gates.push(StateGate {
        key: "/button".into(),
        mask: 2,
    });
    assert!(!hover.shown(&ViewState::default()));
    let nodes: Arc<[DrawNode]> = vec![
        sprite("/base", "textures/base"),
        sprite("/duplicate", "textures/base"),
        hover,
        custom("/tooltip", tooltip::RENDERER, json!({})),
        custom(
            "/banner",
            banner_renderer::RENDERER,
            json!({"property_bag": {
                "#banner_patterns": "0_1", "#banner_colors": "0_15"
            }}),
        ),
    ]
    .into();
    let expected = BTreeSet::from([
        "textures/base".to_owned(),
        "textures/hover".to_owned(),
        tooltip::BACKGROUND_TEXTURE.to_owned(),
        "textures/entity/banner/banner_base".to_owned(),
        "textures/entity/banner/banner_border".to_owned(),
    ]);
    let mut cache = DependencyCache::default();
    for hovered in [None, Some("/button".into()), None] {
        let view = ViewState {
            hovered,
            ..Default::default()
        };
        let art = ScreenArt {
            view: Some(&view),
            ..Default::default()
        };
        let base = cache.get(&nodes, art);
        assert_eq!(requested(&base, art), expected);
        assert!(base.has_renderer(tooltip::RENDERER));
        assert!(base.has_renderer(banner_renderer::RENDERER));
        assert!(!base.has_renderer("absent"));
    }
    assert_eq!(cache.discoveries, 1);
    assert_eq!(cache.node_visits, nodes.len());
    assert_eq!(cache.banner_reads, 1);
}

#[test]
fn omission_values_and_new_or_detached_snapshots_select_their_own_dependencies() {
    let mut nodes: Arc<[DrawNode]> = vec![
        sprite("/root/first[2]~1/image", "textures/first"),
        sprite("/root/second/image", "textures/second"),
    ]
    .into();
    let mut cache = DependencyCache::default();
    for omitted in [
        String::from("first"),
        String::from("first"),
        String::from("second"),
    ] {
        let omissions = [omitted.as_str()];
        let art = ScreenArt {
            omit_controls: &omissions,
            ..Default::default()
        };
        let base = cache.get(&nodes, art);
        let wanted = if omitted == "first" {
            "textures/second"
        } else {
            "textures/first"
        };
        assert_eq!(requested(&base, art), BTreeSet::from([wanted.to_owned()]));
    }
    assert_eq!(
        cache.discoveries, 2,
        "equal strings at new addresses reuse the projection"
    );
    let Storage::Cached(old) = cache.get(&nodes, ScreenArt::default()).0 else {
        panic!("small fixture is cached");
    };
    assert!(
        Arc::get_mut(&mut nodes).is_none(),
        "the cache owns a Weak identity"
    );
    Arc::make_mut(&mut nodes)[0] = sprite("/root/first/image", "textures/rebound");
    let changed = cache.get(&nodes, ScreenArt::default());
    assert!(
        old.paths
            .iter()
            .any(|path| path.as_ref() == "textures/first")
    );
    assert!(requested(&changed, ScreenArt::default()).contains("textures/rebound"));
    let replacement: Arc<[DrawNode]> = vec![
        sprite("/root/first/image", "textures/new-pack"),
        nodes[1].clone(),
    ]
    .into();
    let fresh = cache.get(&replacement, ScreenArt::default());
    assert!(requested(&fresh, ScreenArt::default()).contains("textures/new-pack"));
    assert_eq!(cache.discoveries, 5);
}

struct LiveRenderer {
    path: String,
    calls: Cell<usize>,
    drawn: Cell<bool>,
}

impl CustomRenderer for LiveRenderer {
    /// This fixture only declares textures; painting is outside these dependency tests.
    fn paint(&mut self, _: &CustomDraw<'_>, _: &mut Canvas<'_>) -> Result<bool, PaintError> {
        Ok(false)
    }

    /// Reports changing state even when no custom node names this renderer.
    fn textures<'s>(&'s self, drawn: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        self.calls.set(self.calls.get() + 1);
        self.drawn.set(drawn("live"));
        texture(&self.path);
    }
}

#[test]
fn overlays_and_every_renderer_callback_remain_live_on_cached_and_empty_membership() {
    let nodes: Arc<[DrawNode]> = vec![sprite("/base", "textures/base")].into();
    let mut cache = DependencyCache::default();
    let base = cache.get(&nodes, ScreenArt::default());
    let overlay = [
        sprite("/held", "textures/held"),
        custom("/live", "live", json!({})),
        custom(
            "/banner",
            banner_renderer::RENDERER,
            json!({"#banner_patterns":"1"}),
        ),
    ];
    let mut renderer = LiveRenderer {
        path: "textures/state-a".into(),
        calls: Cell::new(0),
        drawn: Cell::new(false),
    };
    for (index, overlay) in [overlay.as_slice(), &[], overlay.as_slice()]
        .into_iter()
        .enumerate()
    {
        renderer.path = format!("textures/state-{index}");
        let art = ScreenArt {
            overlay,
            ..Default::default()
        };
        let paths: BTreeSet<String> = paths(&base, art, &[&mut renderer], &mut |_| {})
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(paths.contains(&renderer.path));
        assert_eq!(paths.contains("textures/held"), index != 1);
        assert_eq!(
            paths.contains("textures/entity/banner/banner_border"),
            index != 1
        );
        assert_eq!(renderer.drawn.get(), index != 1);
    }
    let art = ScreenArt {
        overlay: &overlay,
        omit_controls: &["live", "held", "banner"],
        ..Default::default()
    };
    let paths: BTreeSet<String> = paths(&base, art, &[&mut renderer], &mut |_| {})
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        paths,
        BTreeSet::from(["textures/base".into(), renderer.path.clone()])
    );
    assert!(!renderer.drawn.get());
    assert_eq!(renderer.calls.get(), 4);
    assert_eq!(cache.discoveries, 1);
}

#[test]
fn slot_and_byte_limits_bound_weak_backing_and_dead_layouts_are_retired() {
    let mut cache = DependencyCache::default();
    let layouts: Vec<Arc<[DrawNode]>> = (0..MAX_ENTRIES + 3)
        .map(|index| vec![sprite("/image", &format!("textures/{index}"))].into())
        .collect();
    for nodes in &layouts {
        let _ = cache.get(nodes, ScreenArt::default());
        assert!(cache.entries.len() <= MAX_ENTRIES);
        assert!(cache.bytes <= MAX_BYTES);
    }
    assert_eq!(cache.entries.len(), MAX_ENTRIES);
    assert_eq!(
        Arc::weak_count(&layouts[0]),
        0,
        "eviction releases the old Weak"
    );
    let retained = Arc::clone(layouts.last().unwrap());
    drop(layouts);
    let _ = cache.get(&retained, ScreenArt::default());
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.bytes, cache.entries[0].bytes);
    cache.byte_limit = cache.bytes * 2;
    let others: Vec<Arc<[DrawNode]>> = (0..3).map(|_| retained.to_vec().into()).collect();
    for nodes in &others {
        let _ = cache.get(nodes, ScreenArt::default());
        assert!(cache.bytes <= cache.byte_limit);
        assert!(cache.entries.len() <= 2);
    }
}

#[test]
fn oversized_summaries_or_node_backing_fall_back_without_retaining_weak_or_owned_paths() {
    let summaries: Arc<[DrawNode]> = vec![
        sprite("/image", &"textures/".repeat(1024)),
        custom("/tooltip", tooltip::RENDERER, json!({})),
    ]
    .into();
    let backing: Arc<[DrawNode]> = vec![node("/solid", Draw::Solid { color: [255; 4] }); 16].into();
    for (nodes, count) in [(summaries, 2), (backing, 0)] {
        let mut cache = DependencyCache {
            byte_limit: 4 * size_of::<DrawNode>(),
            ..Default::default()
        };
        let base = cache.get(&nodes, ScreenArt::default());
        assert!(matches!(&base.0, Storage::Uncached(_)));
        assert_eq!(Arc::weak_count(&nodes), 0);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
        let expected = requested(&base, ScreenArt::default());
        assert_eq!(expected.len(), count);
        cache.byte_limit = MAX_BYTES;
        let cached = cache.get(&nodes, ScreenArt::default());
        assert_eq!(requested(&cached, ScreenArt::default()), expected);
        assert!(cache.bytes >= nodes.len() * size_of::<DrawNode>());
    }
}

#[path = "preparation_tests.rs"]
mod preparation;
