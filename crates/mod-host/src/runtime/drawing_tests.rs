use super::*;
use cinnabar::world_draw::drawing::{self as wit, Host as _};

/// Creates a guest whose callbacks let the test exercise the host transaction boundary.
fn instance() -> Instance {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    Instance::new(
        &engine,
        br#"(component
            (core module $m (func (export "init")) (func (export "frame")))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
        ModGrants {
            world_draw: true,
            ..Default::default()
        },
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap()
}

/// Creates bounded mesh input with enough vertices to test callback mutation accounting.
fn mesh(vertices: usize) -> wit::GeometrySpec {
    wit::GeometrySpec::Triangles(wit::Mesh {
        vertices: (0..vertices)
            .map(|index| wit::Vertex {
                position: ((index % 2) as f32, ((index / 2) % 2) as f32, 0.),
                color: (1., 1., 1., 1.),
                uv: (0., 0.),
                data: (0., 0., 0., 0.),
            })
            .collect(),
        indices: vec![0, 1, 2],
    })
}

#[test]
fn event_geometry_commits_and_its_handles_survive_the_next_frame() {
    let mut instance = instance();
    let mut handles = Vec::new();
    for fuel in [CALLBACK_FUEL, CALLBACK_FUEL, LOAD_FUEL] {
        let before = instance.render().1;
        instance
            .run(fuel, commit_event, |_, store| {
                handles.push(store.data_mut().create_geometry(mesh(3))?.unwrap());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            instance.render().0.retained[0].geometry.len(),
            handles.len()
        );
        assert_ne!(instance.render().1, before);
    }
    instance.begin_frame();
    instance
        .run(FRAME_FUEL, commit, |exports, store| exports.frame(store))
        .unwrap();
    assert_eq!(
        instance.render().0.retained[0].geometry.len(),
        handles.len()
    );
    instance
        .run(CALLBACK_FUEL, commit_event, |_, store| {
            for handle in handles {
                store.data_mut().remove(handle)?.unwrap();
            }
            Ok(())
        })
        .unwrap();
    assert!(instance.render().0.retained.is_empty());
}

#[test]
fn retained_mutation_and_import_budgets_reset_for_each_event() {
    let mut instance = instance();
    let vertices =
        mod_api::MAX_DRAW_MUTATION_BYTES / std::mem::size_of::<mod_render::retained::Vertex>() * 3
            / 4;
    for fuel in [CALLBACK_FUEL, LOAD_FUEL] {
        instance
            .run(fuel, commit_event, |_, store| {
                let state = store.data_mut();
                // Each mesh fits one callback's byte budget; the pair exceeds it.
                state.create_geometry(mesh(vertices))?.unwrap();
                for _ in 1..mod_api::MAX_RENDER_CALLS {
                    state.capabilities()?;
                }
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(instance.render().0.retained[0].geometry.len(), 2);
}

#[test]
fn event_geometry_preserves_the_frames_legacy_primitives() {
    use cinnabar::extension::render::{self as legacy, Host as _};
    let mut instance = instance();
    instance
        .run(FRAME_FUEL, commit, |_, store| {
            store
                .data_mut()
                .draw_lines(vec![legacy::Line {
                    points: vec![
                        legacy::Vector3 {
                            x: 0.,
                            y: 0.,
                            z: 0.,
                        },
                        legacy::Vector3 {
                            x: 1.,
                            y: 0.,
                            z: 0.,
                        },
                    ],
                    width: 1.,
                    color: legacy::Rgba {
                        r: 1.,
                        g: 1.,
                        b: 1.,
                        a: 1.,
                    },
                    pattern: legacy::LinePattern::Solid,
                }])?
                .unwrap();
            Ok(())
        })
        .unwrap();
    let primitives = Arc::clone(&instance.render().0.primitives);
    assert!(!primitives.is_empty());
    instance
        .run(CALLBACK_FUEL, commit_event, |_, store| {
            store.data_mut().create_geometry(mesh(3))?.unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(instance.render().0.retained[0].geometry.len(), 1);
    assert!(Arc::ptr_eq(&instance.render().0.primitives, &primitives));
    instance.begin_frame();
    instance
        .run(FRAME_FUEL, commit, |exports, store| exports.frame(store))
        .unwrap();
    assert_eq!(instance.render().0.retained[0].geometry.len(), 1);
    assert!(instance.render().0.primitives.is_empty());
}

#[test]
fn a_trapped_event_revokes_committed_and_staged_geometry() {
    let mut instance = instance();
    instance
        .run(CALLBACK_FUEL, commit_event, |_, store| {
            store.data_mut().create_geometry(mesh(3))?.unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(instance.render().0.retained[0].geometry.len(), 1);
    let result = instance.run(CALLBACK_FUEL, commit_event, |_, store| {
        store.data_mut().create_geometry(mesh(3))?.unwrap();
        bail!("guest trap after staging geometry")
    });
    assert!(result.is_err());
    assert!(!instance.is_active());
    assert!(instance.render().0.retained.is_empty());
}
