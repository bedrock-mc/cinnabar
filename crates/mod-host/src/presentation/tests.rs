use super::*;
use mod_render::retained::{self, Anchor, GeometrySpec, Vertex};

/// Builds a planet-sized camera-relative triangle using the canonical drawing store.
fn planet() -> Output {
    let mut store = retained::Store::default();
    let geometry = store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    let instance = retained::Instance {
        geometry,
        material: None,
        params: [0.; mod_api::materials::PARAMS],
        anchor: Anchor::Camera,
        transform: retained::Transform {
            translation: [0., 0., -20_000.],
            scale: [6000.; 3],
            ..Default::default()
        },
        tint: [1.; 4],
        depth: retained::Depth::Test,
        blend: retained::Blend::Premultiplied,
        width: 1.,
        width_mode: retained::WidthMode::World,
        join: retained::Join::Bevel,
        cap: retained::Cap::Butt,
    };
    store.create(instance, DrawingPermissions::ALL).unwrap();
    store.commit();
    Output {
        drawing: Snapshot::from_resources(store.snapshot()),
        time: Some(18_000),
        ..Default::default()
    }
}

/// Supplies a native connection with no terrain or actors; celestial drawing needs neither.
fn context() -> Context {
    Context {
        connection: 7,
        dimension: 0,
        ..Default::default()
    }
}

#[test]
fn bundle_planet_and_environment_require_drawing_authority() {
    let mut receiver = Receiver::default();
    assert!(
        receiver
            .apply(planet(), Default::default(), &context(), u64::MAX)
            .is_err()
    );
    assert!(receiver.output().0.retained.is_empty());
    receiver
        .apply(planet(), DrawingPermissions::ALL, &context(), u64::MAX)
        .unwrap();
    assert_eq!(receiver.output().0.retained[0].instances.len(), 1);
    assert_eq!(receiver.time, Some(18_000));
}

#[test]
fn bundle_budget_rejection_preserves_the_previous_scene() {
    let mut receiver = Receiver::default();
    receiver
        .apply(planet(), DrawingPermissions::ALL, &context(), u64::MAX)
        .unwrap();
    let revision = receiver.output().1;
    assert!(
        receiver
            .apply(planet(), DrawingPermissions::ALL, &context(), 0)
            .is_err()
    );
    assert_eq!(receiver.output().1, revision);
    let mut invalid = planet();
    invalid.drawing.geometry[0].1 = GeometrySpec::Triangles {
        vertices: Vec::new(),
        indices: vec![u32::MAX],
    };
    assert!(
        receiver
            .apply(invalid, DrawingPermissions::ALL, &context(), u64::MAX)
            .is_err()
    );
    assert_eq!(receiver.output().1, revision);
}

#[test]
fn helper_tokens_cannot_collide_and_unchanged_geometry_stays_shared() {
    let input = planet();
    let mut a = Receiver::default();
    let mut b = Receiver::default();
    a.apply(input.clone(), DrawingPermissions::ALL, &context(), u64::MAX)
        .unwrap();
    b.apply(input.clone(), DrawingPermissions::ALL, &context(), u64::MAX)
        .unwrap();
    let before = a.output().0.retained[0]
        .geometry
        .values()
        .next()
        .unwrap()
        .clone();
    let owner = a.output().0.retained[0]
        .geometry
        .keys()
        .next()
        .unwrap()
        .owner;
    assert_ne!(
        owner,
        b.output().0.retained[0]
            .geometry
            .keys()
            .next()
            .unwrap()
            .owner
    );
    a.apply(input, DrawingPermissions::ALL, &context(), u64::MAX)
        .unwrap();
    assert!(Arc::ptr_eq(
        &before,
        a.output().0.retained[0].geometry.values().next().unwrap()
    ));
    let weak = Arc::downgrade(&before);
    drop(before);
    drop(a);
    assert!(weak.upgrade().is_none());
}

#[test]
fn serialized_drawing_rechecks_topology_and_elevated_grants() {
    let input = planet();
    let bytes = serde_json::to_vec(&input).unwrap();
    let mut input: Output = serde_json::from_slice(&bytes).unwrap();
    input.drawing.instances[0].1.depth = retained::Depth::Always;
    let mut receiver = Receiver::default();
    assert!(
        receiver
            .apply(
                input,
                DrawingPermissions {
                    world_draw: true,
                    ..Default::default()
                },
                &context(),
                u64::MAX
            )
            .is_err()
    );
}

#[test]
fn large_scenes_stream_within_callback_limits_and_publish_only_when_complete() {
    let mut input = planet();
    input.passes.push(Pass {
        name: "long".into(),
        order: 0,
        source: format!(
            "//{}\nfn effect(uv: vec2<f32>) -> vec3<f32> {{ return scene(uv); }}",
            "original annotation ".repeat(600)
        ),
        enabled: true,
        params: [0.; mod_api::MAX_PASS_PARAMS],
        masks: Vec::new(),
    });
    let (id, mut geometry) = input.drawing.geometry[0].clone();
    if let GeometrySpec::Triangles { vertices, indices } = &mut geometry {
        vertices.extend(vertices.clone());
        indices.extend([3, 4, 5]);
    }
    for slot in 1000..1200 {
        input
            .drawing
            .geometry
            .push((DrawingHandle { slot, ..id }, geometry.clone()));
    }
    assert!(serde_json::to_vec(&input).unwrap().len() > server_experience::policy::MAX_HOST_OUTPUT);
    let mut pending = Pending::new(&input, server_experience::policy::MAX_GUEST_MEMORY).unwrap();
    let mut receiver = Receiver::default();
    let mut pieces = 0;
    while let Some(chunk) = pending.take(512) {
        assert!(
            serde_json::to_vec(&chunk).unwrap().len() <= server_experience::policy::MAX_HOST_OUTPUT
        );
        pieces += 1;
        let complete = pending.complete();
        receiver
            .receive(
                chunk,
                DrawingPermissions::ALL,
                &context(),
                server_experience::policy::MAX_GUEST_MEMORY,
                u64::MAX,
            )
            .unwrap();
        if !complete {
            assert!(receiver.output().0.retained.is_empty());
        } else {
            break;
        }
    }
    assert!(pieces > 1);
    assert_eq!(receiver.output().0.retained[0].instances.len(), 1);
    assert_eq!(receiver.output().0.passes.len(), 1);
}

#[test]
fn partial_transfers_deny_overflow_and_release_their_memory_on_teardown() {
    let mut receiver = Receiver::default();
    let excessive = Chunk {
        revision: 1,
        offset: 0,
        total: 1000,
        bytes: vec![1; 8],
    };
    assert!(
        receiver
            .receive(excessive, DrawingPermissions::ALL, &context(), 16, u64::MAX)
            .is_err()
    );
    let mut pending = Pending::new(&planet(), server_experience::policy::MAX_GUEST_MEMORY).unwrap();
    receiver
        .receive(
            pending.take(32).unwrap(),
            DrawingPermissions::ALL,
            &context(),
            server_experience::policy::MAX_GUEST_MEMORY,
            u64::MAX,
        )
        .unwrap();
    let mut out_of_order = pending.take(32).unwrap();
    out_of_order.offset += 1;
    assert!(
        receiver
            .receive(
                out_of_order,
                DrawingPermissions::ALL,
                &context(),
                server_experience::policy::MAX_GUEST_MEMORY,
                u64::MAX
            )
            .is_err()
    );
    assert!(receiver.output().0.retained.is_empty());
    drop(receiver);
}
