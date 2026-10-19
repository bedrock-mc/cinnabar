//! Untrusted packs cannot supply the engine's internal animation indices.

use std::sync::Arc;

use json_ui::{AnimGraph, AnimNode, Animator, Catalog, Context, ControlAnims, Written, resolve};
use serde_json::json;

#[test]
fn authored_animation_graph_is_not_an_engine_program() {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/test.json"]}"#.as_slice()),
        ("ui/test.json", br#"{"namespace":"test","image":{"type":"image","anim_graph":{"nodes":[],"heads":[0]}}}"#.as_slice()),
    ]).unwrap();
    let control = resolve(&catalog, "test.image", &Context::desktop())
        .control
        .unwrap();
    assert!(!control.properties.contains_key("anim_graph"));
}

/// Build a public animation program with deliberately unchecked indices.
fn program(graph: AnimGraph) -> Arc<ControlAnims> {
    Arc::new(ControlAnims {
        key: "/image".into(),
        graph,
        rest_alpha: 1.0,
        rest_offset: [0.0; 2],
        rect: [0.0, 0.0, 10.0, 10.0],
        anchor: [0.0; 2],
        born: None,
        clock: None,
        disable_fast_forward: false,
        reset_name: Some("reset".into()),
        has_sprite: true,
        layout_authoritative: true,
    })
}

#[test]
fn invalid_heads_and_links_are_ignored_before_sampling() {
    let mut node = AnimNode::parse(
        json!({"anim_type":"alpha","duration":0})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    node.next = Some(1);
    for graph in [
        AnimGraph {
            nodes: vec![],
            heads: vec![0],
        },
        AnimGraph {
            nodes: vec![node],
            heads: vec![0],
        },
    ] {
        let mut animator = Animator::starting_at(0.0);
        let anims = program(graph);
        assert_eq!(animator.sample(&anims, 1.0, None), Written::default());
        animator.fire("reset");
        assert!(animator.take_events().is_empty());
    }
}

#[test]
fn authored_geometry_sampling_marker_cannot_disable_motion() {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/fixture.json",
        r#"{
        "namespace":"fixture",
        "move":{"anim_type":"offset","from":[0,0],"to":[20,10],"duration":1},
        "image":{"type":"image","texture":"textures/fixture","size":[10,10],
            "anchor_from":"top_left","anchor_to":"top_left","offset":"@fixture.move",
            "anim_geometry_sampled":true}
    }"#,
    );
    let control = resolve(&catalog, "fixture.image", &Context::desktop())
        .control
        .unwrap();
    let env = json_ui::LayoutEnv {
        text: &NoText,
        textures: &NoTextures,
    };
    let frame = json_ui::render_bound(
        control,
        [200.0, 100.0],
        &env,
        &json_ui::ViewState::default(),
    );
    let node = frame.nodes.first().unwrap();
    let mut animator = Animator::new();
    node.animate(&mut animator, 0.0, None, None);
    let drawn = node.animate(&mut animator, 1.0, None, None);
    assert_eq!([drawn.dest.x, drawn.dest.y], [20.0, 10.0]);
}

struct NoText;
impl json_ui::TextMeasure for NoText {
    /// This original motion fixture contains no labels.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
struct NoTextures;
impl json_ui::TextureSource for NoTextures {
    /// Motion does not require installed image metadata.
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}
