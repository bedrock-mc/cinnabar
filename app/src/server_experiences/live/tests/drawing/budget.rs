use super::*;
use server_experience::runtime::SceneObject;

/// Declares one legacy object whose asset is admitted by the fixture bundle.
fn legacy(triangles: Option<u32>) -> SceneObject {
    let transform = [0., 0., 0., 0., 0., 0., 1., 1., 1., 1.];
    match triangles {
        Some(triangles) => SceneObject::Mesh {
            asset: "legacy".into(),
            transform,
            triangles,
        },
        None => SceneObject::Quad {
            texture: "legacy".into(),
            transform,
            size: [1.; 2],
        },
    }
}

/// Queues a bounded immutable snapshot independently of later callback scene commands.
fn queue(
    live: &mut Live<FakeWorker>,
    revision: u64,
    output: &mod_host::presentation::Output,
) -> usize {
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .extend([Permission::WorldDraw, Permission::Scene]);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.capabilities.assets.insert("legacy".into());
    instance
        .dispatch(None, Event::Frame, 1, None, None)
        .unwrap();
    let bytes = serde_json::to_vec(output).unwrap();
    let chunks: Vec<_> = bytes
        .chunks(4096)
        .enumerate()
        .map(|(index, part)| mod_host::presentation::Chunk {
            revision,
            offset: (index * 4096) as u32,
            total: bytes.len() as u32,
            bytes: part.to_vec(),
        })
        .collect();
    let count = chunks.len();
    instance
        .helper
        .as_mut()
        .unwrap()
        .presentation
        .extend(chunks);
    count
}

/// Drives fake helper completions without reading a clock or skipping pending chunks.
fn finish(live: &mut Live<FakeWorker>, pieces: usize, slice: &mut u64) {
    for _ in 0..pieces {
        *slice += 1;
        live.poll(1, *slice * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
    }
}

#[test]
fn legacy_only_scene_chunks_complete_without_world_drawing_permission() {
    let mut live = initialized(1);
    let output = mod_host::presentation::Output {
        scene: BTreeMap::from([(1, legacy(None))]),
        ..Default::default()
    };
    queue(&mut live, 1, &output);
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .remove(&Permission::WorldDraw);
    let helper = instance.helper.as_mut().unwrap();
    helper.presentation = snapshot_chunks(1, &output);
    let pieces = helper.presentation.len();
    assert!(pieces > 1);
    finish(&mut live, pieces, &mut 0);
    assert!(live.instances["bundle0"].stopped.is_none());
    assert_eq!(live.instances["bundle0"].presentation.scene().len(), 1);
}

#[test]
fn legacy_and_retained_output_share_limits_in_every_completion_order() {
    for (draws, triangles) in [(MAX_DRAWS as usize, None), (1, Some(MAX_TRIANGLES))] {
        for order in 0..3 {
            let mut live = initialized(1);
            let mut slice = 0;
            let object = legacy(triangles);
            if order != 2 {
                let initial = if order == 0 {
                    mod_host::presentation::Output {
                        scene: BTreeMap::from([(1, object.clone())]),
                        ..Default::default()
                    }
                } else {
                    retained_scene(draws)
                };
                let pieces = queue(&mut live, 1, &initial);
                finish(&mut live, pieces, &mut slice);
                assert!(live.instances["bundle0"].stopped.is_none());
            }
            let mut excessive = retained_scene(draws);
            excessive.scene.insert(1, object);
            let pieces = queue(&mut live, 2, &excessive);
            finish(&mut live, pieces, &mut slice);
            assert!(
                live.instances["bundle0"].stopped.is_some(),
                "order {order}, triangles {triangles:?}"
            );
            assert!(live.drawing().0.retained.is_empty());
        }
    }
}

#[test]
fn combined_scene_accepts_the_exact_draw_and_triangle_limits() {
    for (draws, triangles) in [(MAX_DRAWS as usize - 1, None), (1, Some(MAX_TRIANGLES - 1))] {
        let mut live = initialized(1);
        let mut output = retained_scene(draws);
        output.scene.insert(1, legacy(triangles));
        let pieces = queue(&mut live, 1, &output);
        finish(&mut live, pieces, &mut 0);
        assert!(live.instances["bundle0"].stopped.is_none());
        assert_eq!(live.instances["bundle0"].presentation.scene().len(), 1);
        assert_eq!(live.drawing().0.retained[0].instances.len(), draws);
    }
}

#[test]
fn multichunk_scene_replacements_publish_matching_legacy_and_drawing_revisions() {
    let mut live = initialized(1);
    let mut slice = 0;
    let full = retained_scene(MAX_DRAWS as usize);
    let pieces = queue(&mut live, 1, &full);
    finish(&mut live, pieces, &mut slice);
    let mut replacement = retained_scene(MAX_DRAWS as usize - 1);
    replacement.scene.insert(1, legacy(None));
    let pieces = queue(&mut live, 2, &replacement);
    assert!(pieces > 1);
    finish(&mut live, 1, &mut slice);
    assert!(live.instances["bundle0"].stopped.is_none());
    assert!(live.instances["bundle0"].presentation.scene().is_empty());
    assert_eq!(
        live.drawing().0.retained[0].instances.len(),
        MAX_DRAWS as usize
    );
    // A newer callback changes the legacy scene before the older snapshot has arrived.
    let later = queue(&mut live, 3, &full);
    finish(&mut live, pieces - 1, &mut slice);
    assert!(live.instances["bundle0"].stopped.is_none());
    assert_eq!(live.instances["bundle0"].presentation.scene().len(), 1);
    assert_eq!(
        live.drawing().0.retained[0].instances.len(),
        MAX_DRAWS as usize - 1
    );
    finish(&mut live, later, &mut slice);
    assert!(live.instances["bundle0"].stopped.is_none());
    assert!(live.instances["bundle0"].presentation.scene().is_empty());
    assert_eq!(
        live.drawing().0.retained[0].instances.len(),
        MAX_DRAWS as usize
    );
}
