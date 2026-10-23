//! Withdraws derived shader-pack terrain while retaining its GPU storage for retry.

use super::*;

impl ShaderpackGeometry {
    /// Cancels expansion and draws, and requires every live section to be expanded again.
    pub(crate) fn invalidate_publication(&mut self) {
        self.prepared = PreparedExpansion::default();
        self.views.clear();
        self.shadows.clear();
        self.residency.invalidate_contents(std::iter::empty());
        self.inputs = None;
        self.complete_radius = None;
        self.budget_horizon = None;
        self.retry_eye = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retries reuse allocated storage and hide stale faces until their new expansion commits.
    #[test]
    fn invalidation_retains_storage_and_requeues_stale_faces() {
        let (device, queue) = wgpu::Device::noop(&default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let mut geometry = ShaderpackGeometry::pending_fixture(&device);
        let page = device.create_buffer(&BufferDescriptor {
            label: Some("retained expansion recovery fixture"),
            size: 256,
            usage: BufferUsages::VERTEX | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        geometry.pages.push(page.clone());
        geometry
            .expansion
            .write(&device, &queue, &[0, 0], [0; PARAM_WORDS], &[&[[0; 4]]]);
        let jobs = geometry.expansion.jobs().unwrap().id();
        let inputs = geometry.inputs.clone().unwrap();
        geometry.invalidate_publication();
        assert_eq!(geometry.pages.len(), 1);
        assert_eq!(geometry.pages[0].id(), page.id());
        assert_eq!(geometry.expansion.jobs().unwrap().id(), jobs);
        let draws = ViewDraws::build(&DrawInputs::new(
            geometry.residency.sections.values(),
            None,
            geometry.residency.epoch,
            geometry.residency.generation,
            None,
        ));
        assert!(
            draws.commands.is_empty(),
            "stale expansion must not become drawable again"
        );
        inputs.refresh_residency(
            geometry.inputs.as_ref(),
            false,
            &mut geometry.residency,
            [Entity::PLACEHOLDER],
        );
        assert_eq!(geometry.residency.take_queue(), [Entity::PLACEHOLDER]);
    }
}
