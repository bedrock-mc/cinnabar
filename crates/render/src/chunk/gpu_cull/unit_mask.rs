//! Copies conservative current-frame section verdicts into ordinary unit descriptors.

use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 64;

/// A visibility write never changes addresses, references, counts or primitive order.
pub struct UnitMasks {
    pipeline: wgpu::ComputePipeline,
    clear: wgpu::Buffer,
}

/// Bind groups for one unit page and its current occlusion storage.
pub struct UnitMaskBindings {
    page: wgpu::Buffer,
    clear: wgpu::BindGroup,
    visibility: Option<(wgpu::Buffer, wgpu::BindGroup)>,
}

impl UnitMasks {
    /// Creates the kernel and an all-visible fallback for frames without usable depth.
    pub fn new(device: &wgpu::Device) -> Self {
        let source =
            include_str!("unit_mask.wgsl").replace("MASK_WORKGROUP", &WORKGROUP.to_string());
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ordinary terrain visibility mask"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ordinary terrain visibility mask"),
            layout: None,
            module: &module,
            entry_point: Some("mask_units"),
            compilation_options: Default::default(),
            cache: None,
        });
        let clear = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("all terrain visible"),
            contents: &[0; 4],
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self { pipeline, clear }
    }

    /// Retains both page bindings, rebuilding only when a bound buffer changes.
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        page: &wgpu::Buffer,
        bits: Option<&wgpu::Buffer>,
        bindings: &mut Option<UnitMaskBindings>,
    ) {
        if bindings
            .as_ref()
            .is_none_or(|bindings| bindings.page != *page)
        {
            *bindings = Some(UnitMaskBindings {
                page: page.clone(),
                clear: self.bind_group(device, page, &self.clear),
                visibility: None,
            });
        }
        let bindings = bindings.as_mut().expect("page bindings were just prepared");
        if let Some(bits) = bits
            && bindings
                .visibility
                .as_ref()
                .is_none_or(|(bound, _)| bound != bits)
        {
            bindings.visibility = Some((bits.clone(), self.bind_group(device, page, bits)));
        }
    }

    /// Binds the unit page and one visibility bitset to the mask kernel.
    fn bind_group(
        &self,
        device: &wgpu::Device,
        page: &wgpu::Buffer,
        bits: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ordinary terrain visibility"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: page.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bits.as_entire_binding(),
                },
            ],
        })
    }

    /// Sets referenced flags from retained bindings, clearing them when depth is unavailable.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        bindings: &UnitMaskBindings,
        visibility: bool,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        let group = if visibility {
            &bindings
                .visibility
                .as_ref()
                .expect("prepared occlusion bindings")
                .1
        } else {
            &bindings.clear
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ordinary terrain visibility"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        let groups = count.div_ceil(WORKGROUP);
        let width = groups.min(device.limits().max_compute_workgroups_per_dimension);
        pass.dispatch_workgroups(width, groups.div_ceil(width), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_visibility_mask_buffers_reuse_both_bind_groups() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let masks = UnitMasks::new(&device);
        let buffer = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let page = buffer();
        let bits = buffer();
        let mut bindings = None;
        masks.prepare(&device, &page, Some(&bits), &mut bindings);
        let first = bindings.as_ref().unwrap();
        let clear = first.clear.clone();
        let visibility = first.visibility.as_ref().unwrap().1.clone();
        masks.prepare(&device, &page, Some(&bits), &mut bindings);
        let retained = bindings.as_ref().unwrap();
        assert_eq!(retained.clear, clear);
        assert_eq!(retained.visibility.as_ref().unwrap().1, visibility);

        let replacement_bits = buffer();
        masks.prepare(&device, &page, Some(&replacement_bits), &mut bindings);
        let changed = bindings.as_ref().unwrap();
        assert_eq!(changed.clear, clear);
        assert_ne!(changed.visibility.as_ref().unwrap().1, visibility);
        let replacement_page = buffer();
        masks.prepare(
            &device,
            &replacement_page,
            Some(&replacement_bits),
            &mut bindings,
        );
        let replaced = bindings.as_ref().unwrap();
        assert_eq!(replaced.page, replacement_page);
        assert_ne!(replaced.clear, clear);
    }
}
