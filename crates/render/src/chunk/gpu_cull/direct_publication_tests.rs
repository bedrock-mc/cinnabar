use super::super::model::CullRecord;
use super::*;

impl DirectOcclusion {
    /// Reads the GPU records used to compute this publication's direct-draw occlusion.
    pub(in crate::chunk::gpu_cull) fn publication_snapshot(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Vec<Vec<u8>> {
        self.storage.as_ref().map_or_else(Vec::new, |storage| {
            vec![crate::render_test_support::read_buffer(
                device,
                queue,
                &storage.records,
                0,
                u64::from(self.table.slot_count()) * std::mem::size_of::<CullRecord>() as u64,
            )]
        })
    }
}

#[test]
fn cancelled_direct_plans_return_their_readback_slots() {
    use super::super::app_tests::{camera_transform, chunk_app, noop_render_plugin};

    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Off,
        camera_transform(),
    );
    let world = app.sub_app_mut(RenderApp).world_mut();
    let solid_draw = world
        .resource::<DrawFunctions<Opaque3d>>()
        .read()
        .id::<pipeline::units::commands::DrawTerrainOccluders>();
    let mut occlusion = world.resource_mut::<DirectOcclusion>();
    let tag = VerdictTag {
        frame: 1,
        basis: OcclusionBasis {
            eye: [0.0; 3],
            view_rotation: Mat3::IDENTITY.to_cols_array(),
            clip_from_view: Mat4::IDENTITY.to_cols_array(),
            viewport: [0, 0, 64, 64],
            depth_size: [64, 64],
            depth_samples: 1,
            world: 0,
        },
        slots: 1,
    };
    let in_flight = occlusion.verdicts.acquire(tag).unwrap();
    drop(occlusion.verdicts.submit(in_flight));
    for invalidate in [false, true] {
        for _ in 0..SLOTS * 2 {
            let verdict = occlusion
                .verdicts
                .acquire(tag)
                .expect("cancelled slot is free");
            occlusion.plan = Some(TerrainPassPlan {
                view: Entity::PLACEHOLDER,
                solid_pipeline: CachedRenderPipelineId::INVALID,
                solid_draw,
                solid: Vec::new(),
                verdict: Some(verdict),
                slots: 1,
                encoded: AtomicBool::new(false),
            });
            if invalidate {
                occlusion.invalidate_publication();
            } else {
                occlusion.suppress_publication();
            }
            assert!(occlusion.plan.is_none());
            assert!(occlusion.verdicts.tags[verdict].is_none());
        }
    }
    assert_eq!(occlusion.verdicts.ring.oldest_in_flight(), Some(in_flight));
    assert!((0..SLOTS - 1).all(|_| occlusion.verdicts.acquire(tag).is_some()));
    assert!(occlusion.verdicts.acquire(tag).is_none());
}
