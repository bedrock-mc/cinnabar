//! Copies conservative current-frame section verdicts into ordinary unit descriptors.

use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 64;

/// A visibility write never changes addresses, references, counts or primitive order.
pub struct UnitMasks {
    pipeline: wgpu::ComputePipeline,
    clear: wgpu::Buffer,
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

    /// Sets every referenced unit's flag, including clearing stale flags after disocclusion.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        page: &wgpu::Buffer,
        bits: Option<&wgpu::Buffer>,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ordinary terrain visibility"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: page.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bits.unwrap_or(&self.clear).as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ordinary terrain visibility"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        let groups = count.div_ceil(WORKGROUP);
        let width = groups.min(device.limits().max_compute_workgroups_per_dimension);
        pass.dispatch_workgroups(width, groups.div_ceil(width), 1);
    }
}
