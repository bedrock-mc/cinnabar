use super::*;
use crate::ModGrants;
use wit::Host as _;

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

#[test]
fn retained_publication_allocates_nothing_for_empty_callbacks() {
    use mod_render::retained::{GeometrySpec, Vertex};

    let mut render = RenderState::new();
    let geometry = render
        .retained
        .create_geometry(GeometrySpec::Triangles {
            vertices: vec![
                Vertex {
                    position: [0.; 3],
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                };
                mod_api::MAX_DRAW_VERTICES / 4
            ],
            indices: vec![0, 1, 2],
        })
        .unwrap();
    render.commit_retained();
    let generation = render.generation();
    let before = Arc::clone(&render.output().retained[0]);
    let (_, allocations) = test_alloc::count(|| {
        for _ in 0..8 {
            render.begin_callback();
            render.commit_retained();
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(render.generation(), generation);
    assert!(Arc::ptr_eq(&before, &render.output().retained[0]));

    render.begin_callback();
    render.retained.remove(geometry).unwrap();
    render.commit_retained();
    assert!(render.output().retained.is_empty());
    assert_ne!(render.generation(), generation);
    let generation = render.generation();
    let (_, allocations) = test_alloc::count(|| {
        render.begin_callback();
        render.commit_retained();
    });
    assert_eq!(allocations, 0);
    assert_eq!(render.generation(), generation);
}

#[test]
fn material_only_output_survives_entity_reconciliation() {
    let mut render = RenderState::new();
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    };
    render.retained.bind_session(1, 0);
    render.update_entities(vec![actor], &[]).unwrap();
    let entity = render.retained.observe_entities().unwrap()[0].0;
    let material = render
        .retained
        .create_material(
            mod_render::material::BUILTIN,
            mod_api::drawing::DrawingPermissions::ALL,
        )
        .unwrap();
    render.commit_retained();
    assert_eq!(render.output().retained.len(), 1);
    let before = Arc::clone(&render.output().retained[0]);
    let generation = render.generation();

    render.update_entities(Vec::new(), &[]).unwrap();
    assert!(Arc::ptr_eq(&before, &render.output().retained[0]));
    assert_eq!(render.generation(), generation);

    render.update_entities(Vec::new(), &[actor]).unwrap();
    let after = &render.output().retained[0];
    assert!(!after.entities.contains_key(&entity));
    assert!(after.materials.contains_key(&material));
    assert!(Arc::ptr_eq(
        &before.materials[&material],
        &after.materials[&material]
    ));
    assert_ne!(render.generation(), generation);
}

#[test]
fn material_only_publication_preserves_allocation_free_empty_callbacks() {
    let mut render = RenderState::new();
    let material = render
        .retained
        .create_material(
            mod_render::material::BUILTIN,
            mod_api::drawing::DrawingPermissions::ALL,
        )
        .unwrap();
    render.commit_retained();
    assert!(
        render.output().retained[0]
            .materials
            .contains_key(&material)
    );
    let before = render.output().retained[0].clone();
    let generation = render.generation();
    let (_, allocations) = test_alloc::count(|| {
        render.begin_callback();
        render.commit_retained();
    });
    assert_eq!(allocations, 0);
    assert_eq!(render.generation(), generation);
    assert!(Arc::ptr_eq(&before, &render.output().retained[0]));

    render.begin_callback();
    render.retained.remove(material).unwrap();
    render.commit_retained();
    assert!(render.output().retained.is_empty());
    assert_ne!(render.generation(), generation);
}

fn state() -> State {
    let grants = ModGrants {
        render: true,
        ..Default::default()
    };
    State::new(grants, String::new(), Default::default())
}

#[test]
fn unchanged_pass_updates_stage_nothing() {
    let mut state = state();
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }";
    let spec = wit::PassSpec {
        name: "grade".into(),
        order: 0,
        source: source.into(),
        depth: false,
    };
    state.register_pass(spec).unwrap().unwrap();
    state
        .update_pass("grade".into(), true, vec![0.5])
        .unwrap()
        .unwrap();
    state.render.commit();
    state.render.begin_frame();
    state
        .update_pass("grade".into(), true, vec![0.5])
        .unwrap()
        .unwrap();
    assert!(
        state.render.staged_passes.is_none(),
        "a no-op update copies no passes"
    );
    state
        .update_pass("grade".into(), true, vec![0.75])
        .unwrap()
        .unwrap();
    assert!(state.render.staged_passes.is_some());
    assert!(
        state
            .update_pass("missing".into(), true, vec![])
            .unwrap()
            .is_err()
    );
}

/// Additive line drawing preserves legacy ribbons and rejects overflow atomically.
#[test]
fn solid_lines_share_the_legacy_ribbon_budget() {
    let mut state = state();
    let ribbon = wit::Ribbon {
        points: vec![
            wit::Vector3 {
                x: 0.,
                y: 0.,
                z: 0.,
            },
            wit::Vector3 {
                x: 1.,
                y: 1.,
                z: 1.,
            },
        ],
        width: 0.01,
        color: wit::Rgba {
            r: 1.,
            g: 1.,
            b: 1.,
            a: 1.,
        },
    };
    state
        .draw(wit::Primitives {
            decals: vec![],
            ribbons: vec![ribbon.clone(); mod_api::MAX_RENDER_RIBBONS - 1],
            beams: vec![],
            billboards: vec![],
        })
        .unwrap()
        .unwrap();
    let line = wit::Line {
        points: ribbon.points.clone(),
        width: 1.,
        color: ribbon.color,
        pattern: wit::LinePattern::Solid,
    };
    state.draw_lines(vec![line.clone()]).unwrap().unwrap();
    assert!(state.render.staged_primitives.ribbons[0].taper);
    assert!(!state.render.staged_primitives.ribbons.last().unwrap().taper);
    assert!(state.draw_lines(vec![line]).unwrap().is_err());
    assert_eq!(
        state.render.staged_primitives.ribbons.len(),
        mod_api::MAX_RENDER_RIBBONS
    );
}

#[test]
fn legacy_post_imports_obey_the_world_fence() {
    let mut state = state();
    state.drawing_policy = Some(mod_api::drawing::DrawingSession {
        connection: 1,
        permissions: mod_api::drawing::DrawingPermissions {
            world_draw: true,
            ..Default::default()
        },
        ..Default::default()
    });
    assert!(
        state
            .register_pass(wit::PassSpec {
                name: "grade".into(),
                order: 0,
                source: "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }".into(),
                depth: false,
            })
            .unwrap()
            .is_err()
    );
    assert!(
        state
            .update_pass("grade".into(), true, vec![])
            .unwrap()
            .is_err()
    );
    assert!(state.remove_pass("grade".into()).unwrap().is_err());
    state.render.commit();
    assert!(state.render.output().passes.is_empty());
}

#[test]
fn legacy_post_effects_use_the_same_user_consent_in_local_and_remote_worlds() {
    for local in [false, true] {
        for granted in [false, true] {
            let mut state = state();
            state.grants.render = granted;
            state.drawing_policy = Some(mod_api::drawing::DrawingSession {
                connection: 1,
                local,
                permissions: mod_api::drawing::DrawingPermissions::ALL,
                ..Default::default()
            });
            assert_eq!(
                state
                    .register_pass(wit::PassSpec {
                        name: "grade".into(),
                        order: 0,
                        source: "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }"
                            .into(),
                        depth: false,
                    })
                    .unwrap()
                    .is_ok(),
                granted
            );
        }
    }
}

/// Native overlays share the ribbon budget and revocation invalidates even native-only output.
#[test]
fn native_lines_respect_existing_budget_and_revoke_their_generation() {
    let mut state = RenderState::new();
    let ribbon = mod_render::Ribbon {
        pattern: mod_render::LinePattern::Solid,
        taper: false,
        points: vec![[0.; 3], [1., 0., 0.]],
        width: 1.,
        color: [1.; 4],
    };
    state.staged_primitives.ribbons = vec![ribbon.clone(); mod_api::MAX_RENDER_RIBBONS];
    state.commit();
    state.native_lines(vec![ribbon.clone()]);
    assert_eq!(
        state.output().primitives.ribbons.len(),
        mod_api::MAX_RENDER_RIBBONS
    );
    state.begin_frame();
    state.commit();
    state.native_lines(vec![ribbon]);
    assert_eq!(state.output().primitives.ribbons.len(), 1);
    let generation = state.generation();
    state.revoke();
    assert!(state.output().primitives.is_empty());
    assert_ne!(state.generation(), generation);
}

/// Native outlines preserve retained commits and entity removals, and revoke with them.
#[test]
fn native_lines_reconcile_retained_output_and_revoke_both() {
    let mut render = RenderState::new();
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    };
    render.retained.bind_session(1, 0);
    render.update_entities(vec![actor], &[]).unwrap();
    let entity = render.retained.observe_entities().unwrap()[0].0;
    let material = render
        .retained
        .create_material(
            mod_render::material::BUILTIN,
            mod_api::drawing::DrawingPermissions::ALL,
        )
        .unwrap();
    render.commit_retained();
    render.native_lines(vec![mod_render::Ribbon {
        pattern: mod_render::LinePattern::Solid,
        taper: false,
        points: vec![[0.; 3], [1., 0., 0.]],
        width: 1.,
        color: [1.; 4],
    }]);
    render.begin_callback();
    render.retained.remove(material).unwrap();
    render.commit_retained();
    assert!(render.output().retained.is_empty());
    let material = render
        .retained
        .create_material(
            mod_render::material::BUILTIN,
            mod_api::drawing::DrawingPermissions::ALL,
        )
        .unwrap();
    render.commit_retained();
    assert!(
        render.output().retained[0]
            .materials
            .contains_key(&material)
    );
    assert!(render.output().retained[0].entities.contains_key(&entity));
    render.update_entities(Vec::new(), &[actor]).unwrap();
    assert!(render.output().retained[0].entities.is_empty());
    assert_eq!(render.output().primitives.ribbons.len(), 1);
    render.revoke();
    assert_eq!(render.output(), &RenderOutput::default());
}
