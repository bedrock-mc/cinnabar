use super::*;

#[test]
fn staggered_drawing_initializers_send_ready_before_overlapping_frames() {
    let mut live = fixture(2);
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
    }
    complete(&mut live, "bundle0");
    assert!(live.poll(1, CALLBACK_INTERVAL_MS).unwrap().is_empty());
    // Let an early frame stay pending while the second initializer finishes.
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response = None;
    complete(&mut live, "bundle1");
    let packets = live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    assert!(live.ready);
    for instance in live.instances.values() {
        assert_eq!(instance.helper.as_ref().unwrap().dispatched, [Event::Frame]);
    }
}

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
    let ledger = live.drawing().0.gpu_reservations[0].ledger.clone();
    let gpu_mesh = ledger.allocate(256, || Some(())).unwrap();
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
    assert_eq!(output.gpu_reservations[0].ledger.used(), 256);
    drop(gpu_mesh);
    let (output, released_revision) = live.drawing();
    assert_eq!(output.gpu_reservations[0].ledger.used(), 0);
    assert_eq!(released_revision, revision);
    assert_eq!(live.drawing().1, released_revision);
}

#[test]
fn idle_dimension_change_binds_current_entities_before_epoch_callback() {
    let mut live = initialized(1);
    live.grant.wire = Wire {
        version: MAX_WIRE_VERSION,
        limits: Limits::host(),
    };
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.capabilities.scope.permissions.extend([
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::RenderThroughWalls,
    ]);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.helper.as_mut().unwrap().observe_epoch = true;
    let old = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let mut initial = context(1);
    initial.candidates = vec![old];
    initial.live = vec![old];
    initial.targets = vec![(
        mod_api::drawing::MaskEntity {
            connection: old.session,
            dimension: old.dimension,
            runtime_id: old.runtime_id,
            spawn_revision: old.spawn_revision,
        },
        3,
    )];
    live.bind_drawing(&initial, |_| true).unwrap();
    let current = mod_render::retained::EntityKey {
        dimension: 1,
        runtime_id: 43,
        ..old
    };
    let current_context = mod_host::presentation::Context {
        dimension: 1,
        candidates: vec![current],
        live: vec![current],
        targets: vec![(
            mod_api::drawing::MaskEntity {
                connection: current.session,
                dimension: current.dimension,
                runtime_id: current.runtime_id,
                spawn_revision: current.spawn_revision,
            },
            3,
        )],
        ..context(2)
    };
    live.bind_drawing(&current_context, |key| key == current)
        .unwrap();
    live.poll(2, CALLBACK_INTERVAL_MS).unwrap();
    live.poll(2, 2 * CALLBACK_INTERVAL_MS).unwrap();
    let instance = &live.instances["bundle0"];
    assert!(instance.stopped.is_none());
    let context = instance.helper.as_ref().unwrap().contexts.last().unwrap();
    assert_eq!(context, &current_context);
    assert_eq!(
        instance.presentation.output().0.retained[0]
            .entities
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [current]
    );
}

#[test]
fn pending_initialization_uses_its_original_drawing_fence() {
    let mut live = fixture(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::EntityHighlight);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    let helper = instance.helper.as_mut().unwrap();
    assert_eq!(helper.contexts, [context(1)]);
    let bytes = serde_json::to_vec(&mod_host::presentation::Output {
        time: Some(18_000),
        ..Default::default()
    })
    .unwrap();
    helper
        .presentation
        .push_back(mod_host::presentation::Chunk {
            revision: 1,
            offset: 0,
            total: bytes.len() as u32,
            bytes,
        });
    complete(&mut live, "bundle0");
    let changed = mod_host::presentation::Context {
        dimension: 1,
        ..context(1)
    };
    live.bind_drawing(&changed, |_| false).unwrap();
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.ready);
    assert_eq!(live.instances["bundle0"].presentation.time, None);
}

#[test]
fn dispatch_requires_a_context_for_the_callback_epoch() {
    let mut live = initialized(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert!(instance.dispatch(None, Event::Epoch, 2, None).is_err());
    instance.drawing_context = Default::default();
    assert!(instance.dispatch(None, Event::Frame, 1, None).is_err());
    assert!(instance.helper.as_ref().unwrap().dispatched.is_empty());
}
