use super::*;

/// Completes every startup callback across aggregate scheduling slices.
fn initialized(count: usize) -> Live<FakeWorker> {
    let mut live = fixture(count);
    for slice in 0..count {
        for instance in live.instances.values_mut() {
            if instance.busy
                && instance.callback == "init"
                && let Some(helper) = &mut instance.helper
            {
                helper.response = Some(Transaction {
                    owner: instance.owner.clone(),
                    epoch: live.epoch,
                    commands: Vec::new(),
                });
            }
        }
        live.poll(1, slice as u64 * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(live.ready);
    live
}

#[test]
fn frame_callbacks_share_the_session_budget_fairly() {
    let mut live = initialized(4);
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
    }
    for slice in 4..12 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    for instance in live.instances.values() {
        let helper = instance.helper.as_ref().unwrap();
        assert_eq!(
            helper.dispatched,
            vec![Event::Frame; 4],
            "{}",
            instance.owner.bundle
        );
    }
}

#[test]
fn every_multichunk_initial_scene_finishes_under_frame_budget_pressure() {
    let mut live = fixture(4);
    let bytes = serde_json::to_vec(&mod_host::presentation::Output {
        time: Some(18_000),
        ..Default::default()
    })
    .unwrap();
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
        instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
        instance.drawing_context.connection = live.drawing_connection;
    }
    for slice in 0..16 {
        for instance in live.instances.values_mut() {
            if instance.busy
                && instance.callback == "init"
                && let Some(helper) = &mut instance.helper
            {
                helper.presentation = bytes
                    .chunks(32)
                    .enumerate()
                    .map(|(index, part)| mod_host::presentation::Chunk {
                        revision: 1,
                        offset: (index * 32) as u32,
                        total: bytes.len() as u32,
                        bytes: part.to_vec(),
                    })
                    .collect();
                helper.response = Some(Transaction {
                    owner: instance.owner.clone(),
                    epoch: live.epoch,
                    commands: Vec::new(),
                });
            }
        }
        live.poll(1, slice * CALLBACK_INTERVAL_MS).unwrap();
    }
    assert!(live.ready);
    for instance in live.instances.values() {
        assert_eq!(
            instance.presentation.time,
            Some(18_000),
            "{}",
            instance.owner.bundle
        );
        assert!(instance.helper.as_ref().unwrap().presentation.is_empty());
    }
}

#[test]
fn drawing_releases_retired_gpu_charges_without_a_helper_reply() {
    use mod_api::drawing::{DrawingHandle, DrawingPermissions};
    use mod_render::retained::{GeometrySpec, Vertex};

    let mut live = initialized(1);
    let mut input = mod_host::presentation::Output::default();
    input.drawing.geometry.push((
        DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        GeometrySpec::Triangles {
            vertices: [[0.; 3], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        },
    ));
    let context = mod_host::presentation::Context::default();
    live.instances
        .values_mut()
        .next()
        .unwrap()
        .presentation
        .apply(input, DrawingPermissions::ALL, &context, u64::MAX)
        .unwrap();
    let gpu_mesh = Arc::clone(
        live.drawing().0.retained[0]
            .geometry
            .values()
            .next()
            .unwrap(),
    );
    live.instances
        .values_mut()
        .next()
        .unwrap()
        .presentation
        .apply(
            Default::default(),
            DrawingPermissions::ALL,
            &context,
            u64::MAX,
        )
        .unwrap();
    let (output, revision) = live.drawing();
    assert_eq!(
        output.gpu_reservations[0].geometry_bytes,
        gpu_mesh.gpu_bytes()
    );
    drop(gpu_mesh);
    let (output, released_revision) = live.drawing();
    assert_eq!(output.gpu_reservations[0].geometry_bytes, 0);
    assert_ne!(released_revision, revision);
    assert_eq!(live.drawing().1, released_revision);
}
