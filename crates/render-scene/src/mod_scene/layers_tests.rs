use super::*;
use mod_render::retained::{GeometrySpec, Store, Vertex};

#[test]
fn bundle_primitives_render_only_the_admitted_shared_budget() {
    let outputs: Vec<_> = (0..2)
        .map(|index| {
            let primitives = Arc::new(mod_render::Primitives {
                beams: vec![
                    mod_render::Beam {
                        start: [index as f32, 0., 0.],
                        end: [index as f32, 1., 0.],
                        width: 1.,
                        color: [1.; 4],
                        intensity: 1.,
                    };
                    mod_api::MAX_RENDER_BEAMS
                ],
                ..Default::default()
            });
            RenderOutput {
                gpu_reservations: vec![mod_render::GpuReservation {
                    owners: Vec::new(),
                    ledger: mod_render::GpuLedger::new(u64::MAX),
                    primitives: mod_render::PrimitiveRanges::all(&primitives),
                    passes: Vec::new(),
                }],
                primitives,
                ..Default::default()
            }
        })
        .collect();
    let merged = mod_render::RenderMerge::default().merge(&outputs);
    let mut scene = ModRenderScene::default();
    scene.apply_server(&merged, 1);
    assert_eq!(scene.vertex_count(), mod_api::MAX_RENDER_BEAMS * 6);
    assert_eq!(
        scene.bundle_vertices[0].0,
        outputs[0].gpu_reservations[0].ledger
    );
    assert_eq!(
        scene.bundle_vertices[0].1.len(),
        mod_api::MAX_RENDER_BEAMS * 6
    );
    assert!(scene.bundle_vertices[1].1.is_empty());
    let vertices = Arc::clone(&scene.bundle_vertices[0].1);
    let mut metadata = merged.clone();
    metadata.gpu_reservations[0].passes.push(1);
    scene.apply_server(&metadata, 2);
    assert!(Arc::ptr_eq(&vertices, &scene.bundle_vertices[0].1));
    let personal = RenderOutput {
        primitives: Arc::new(mod_render::Primitives {
            beams: vec![outputs[0].primitives.beams[0]; 1],
            ..Default::default()
        }),
        ..Default::default()
    };
    scene.apply(&personal, 1);
    assert_eq!(scene.vertices.len(), 6);
    assert_eq!(scene.vertex_count(), mod_api::MAX_RENDER_BEAMS * 6);
    assert_eq!(
        scene.bundle_vertices[0].1.len(),
        (mod_api::MAX_RENDER_BEAMS - 1) * 6
    );
    assert!(scene.bundle_vertices[1].1.is_empty());
}

#[test]
fn bundle_gpu_reservations_survive_merging_extraction_and_revocation() {
    let outputs: Vec<_> = (1..=2)
        .map(|owner| RenderOutput {
            gpu_reservations: vec![mod_render::GpuReservation {
                owners: vec![owner],
                ledger: mod_render::GpuLedger::new(owner * 64),
                primitives: Default::default(),
                passes: Vec::new(),
            }],
            ..Default::default()
        })
        .collect();
    let merged = mod_render::RenderMerge::default().merge(&outputs);
    let mut scene = ModRenderScene::default();
    scene.apply_server(&merged, 1);
    scene.apply(&output(), 1);
    let extracted = ModRenderScene::extract_resource(&scene);
    assert_eq!(extracted.gpu_reservations, merged.gpu_reservations);
    assert_eq!(extracted.gpu_reservations.len(), 2);
    scene.apply_server(&RenderOutput::default(), 0);
    assert!(scene.gpu_reservations.is_empty());
}

/// Supplies an admitted resource snapshot from one independent drawing owner.
fn output() -> RenderOutput {
    let mut store = Store::default();
    store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
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
    store.commit();
    RenderOutput {
        retained: vec![Arc::clone(store.snapshot())],
        ..Default::default()
    }
}

#[test]
fn revoking_client_parts_preserves_personal_drawing() {
    let mut scene = ModRenderScene::default();
    let personal = output();
    scene.apply(&personal, 1);
    scene.apply_server(&output(), 2);
    assert_eq!(scene.retained.len(), 2);
    scene.apply_server(&RenderOutput::default(), 0);
    assert_eq!(scene.retained.len(), 1);
    assert!(Arc::ptr_eq(&scene.retained[0], &personal.retained[0]));
    let generation = scene.generation();
    scene.apply_server(&RenderOutput::default(), 0);
    assert_eq!(scene.generation(), generation);
}
