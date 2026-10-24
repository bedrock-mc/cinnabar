//! Separately allocated immediate geometry for signed bundle owners.
use super::*;

#[derive(Resource, Default)]
pub(crate) struct BundleGpu(pub Vec<(mod_render::GpuLedger, Option<PrimitiveGpu>)>);

impl BundleGpu {
    /// Reclaims revoked bundles and prepares each live owner's retained buffer capacity.
    pub(super) fn prepare(
        &mut self,
        scene: &ModRenderScene,
        time: [f32; 2],
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let retired: Vec<_> = self
            .0
            .extract_if(.., |(ledger, _)| {
                !scene
                    .bundle_vertices
                    .iter()
                    .any(|(live, _)| live == &*ledger)
            })
            .collect();
        for (index, (ledger, vertices)) in scene.bundle_vertices.iter().enumerate() {
            if self.0.get(index).is_none_or(|(old, _)| old != ledger) {
                if let Some(previous) = self.0.iter().position(|(old, _)| old == ledger) {
                    self.0.swap(index, previous);
                } else {
                    self.0.insert(index, (ledger.clone(), None));
                }
            }
            let gpu = &mut self.0[index].1;
            if gpu.is_none() && !vertices.is_empty() {
                *gpu = PrimitiveGpu::new(device, Some(ledger));
            }
            if let Some(gpu) = gpu {
                let empty = Arc::clone(&gpu.uploaded_marker);
                upload(
                    gpu,
                    vertices,
                    &empty,
                    &empty,
                    time,
                    device,
                    queue,
                    Some(ledger),
                );
            }
        }
        if !retired.is_empty() {
            crate::device_poll::on_frame_complete(queue, move || drop(retired));
        }
    }
}

/// Resolves the host draw at zero and signed bundle draws in scene order thereafter.
pub(super) fn select<'a>(
    host: &'a PrimitiveGpu,
    bundles: &'a BundleGpu,
    index: u32,
) -> Option<&'a PrimitiveGpu> {
    if index == 0 {
        Some(host)
    } else {
        bundles.0.get(index as usize - 1)?.1.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_growth_recovers_after_retiring_the_old_buffer() {
        let (app, _) = crate::queue_review_support::app();
        let device = app.world().resource::<RenderDevice>();
        let queue = app.world().resource::<RenderQueue>();
        let vertices: Arc<[ModVertex]> = vec![ModVertex::default(); 48].into();
        let capacity = (vertices.len() as u64).next_power_of_two();
        let ledger = mod_render::GpuLedger::new(vertex_descriptor(capacity).size + FRAME_BYTES);
        let mut gpu = PrimitiveGpu::new(device, Some(&ledger)).unwrap();
        let empty = Arc::from([]);
        for _ in 0..3 {
            upload(
                &mut gpu,
                &vertices,
                &empty,
                &empty,
                [0.; 2],
                device,
                queue,
                Some(&ledger),
            );
            assert!(ledger.used() <= ledger.limit());
            queue.submit([]);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            assert!(ledger.used() <= ledger.limit());
        }
        assert_eq!(gpu.vertex_count as usize, vertices.len());
        assert_eq!(ledger.used(), ledger.limit());
        drop(gpu);
        assert_eq!(ledger.used(), 0);
    }

    #[test]
    fn primitive_capacity_stays_charged_when_output_shrinks_and_grows_only_when_it_fits() {
        let (app, _) = crate::queue_review_support::app();
        let device = app.world().resource::<RenderDevice>();
        let queue = app.world().resource::<RenderQueue>();
        let ledger = mod_render::GpuLedger::new(16384);
        let mut gpu = PrimitiveGpu::new(device, Some(&ledger)).unwrap();
        let empty = Arc::from([]);
        let vertices = vec![ModVertex::default(); 48].into();
        upload(
            &mut gpu,
            &vertices,
            &empty,
            &empty,
            [0.; 2],
            device,
            queue,
            Some(&ledger),
        );
        queue.submit([]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let grown = ledger.used();
        assert_eq!(
            grown,
            gpu.vertices.as_ref().unwrap().size() + gpu.frame.size()
        );
        upload(
            &mut gpu,
            &empty,
            &empty,
            &empty,
            [0.; 2],
            device,
            queue,
            Some(&ledger),
        );
        assert_eq!(gpu.vertex_count, 0);
        assert_eq!(ledger.used(), grown);
        let other = ledger
            .allocate(ledger.limit() - grown, || Some(()))
            .unwrap();
        let too_large = vec![ModVertex::default(); 96].into();
        let buffer = gpu.vertices.as_ref().unwrap().id();
        upload(
            &mut gpu,
            &too_large,
            &empty,
            &empty,
            [0.; 2],
            device,
            queue,
            Some(&ledger),
        );
        assert_eq!(gpu.vertices.as_ref().unwrap().id(), buffer);
        assert_eq!(
            gpu.vertex_count, 0,
            "failed admission preserves the previous upload"
        );
        drop(other);
        drop(gpu);
        assert_eq!(ledger.used(), 0);
    }
}
