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
