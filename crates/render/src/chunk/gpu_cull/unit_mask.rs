//! Copies current-frame section verdicts and clears retained flags before direct drawing.

use render_model::terrain_units::IndexedUnitPage;
use wgpu::util::DeviceExt;

use super::model::MASK_WORKGROUP as WORKGROUP;

/// A visibility write never changes addresses, references, counts or primitive order.
pub struct UnitMasks {
    pipeline: wgpu::ComputePipeline,
    clear_pipeline: wgpu::ComputePipeline,
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
        if let Some(timing) = render_scene::startup_timing::current() {
            timing.count(render_scene::startup_timing::WORLD_SHADER_MODULES, 1);
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ordinary terrain visibility mask"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ordinary terrain visibility layout"),
            entries: &[storage(0, false), storage(1, true)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ordinary terrain visibility pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            if let Some(timing) = render_scene::startup_timing::current() {
                timing.count(render_scene::startup_timing::WORLD_COMPUTE_PIPELINES, 1);
            }
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("ordinary terrain visibility mask"),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let clear = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("all terrain visible"),
            contents: &[0; 4],
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self {
            pipeline: pipeline("mask_units"),
            clear_pipeline: pipeline("clear_units"),
            clear,
        }
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

    /// Sets referenced flags, or clears every retained slot even when no units are selected.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        bindings: &UnitMaskBindings,
        visibility: bool,
        count: u32,
    ) {
        let count = if visibility {
            count
        } else {
            IndexedUnitPage::capacity_for(bindings.page.size()) as u32
        };
        if count == 0 {
            return;
        }
        let (pipeline, group) = if visibility {
            (
                &self.pipeline,
                &bindings
                    .visibility
                    .as_ref()
                    .expect("prepared occlusion bindings")
                    .1,
            )
        } else {
            (&self.clear_pipeline, &bindings.clear)
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ordinary terrain visibility"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
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
    fn fallback_clears_retained_units_outside_the_current_references() {
        use crate::gpu_snapshot::Gpu;
        use render_model::terrain_units::{QUADS_PER_UNIT, VisibleTerrainUnit};

        let Some(gpu) = Gpu::for_fixture("retained terrain visibility flags") else {
            return;
        };
        let masks = UnitMasks::new(&gpu.device);
        let layout = IndexedUnitPage {
            capacity: WORKGROUP as usize + 3,
        };
        let descriptors: Vec<_> = (0..layout.capacity as u32)
            .map(|slot| VisibleTerrainUnit {
                stream_start: slot * QUADS_PER_UNIT,
                quad_count: 1,
                origin_slot: slot,
                hidden: 0,
            })
            .collect();
        let mut words = vec![0; layout.byte_size() as usize / size_of::<u32>()];
        words[..4].copy_from_slice(&layout.header(layout.capacity as u32));
        words[4..4 + descriptors.len() * 4].copy_from_slice(bytemuck::cast_slice(&descriptors));
        let reference_start = layout.reference_offset() as usize / size_of::<u32>();
        for (slot, word) in words[reference_start..][..layout.capacity]
            .iter_mut()
            .enumerate()
        {
            *word = slot as u32;
        }
        let bits = gpu.words(
            &vec![u32::MAX; layout.capacity.div_ceil(32)],
            wgpu::BufferUsages::STORAGE,
        );
        for selected in [0, 1] {
            let page = gpu.words(
                &words,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            );
            let mut bindings = None;
            masks.prepare(&gpu.device, &page, Some(&bits), &mut bindings);
            let bindings = bindings.unwrap();
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            masks.encode(
                &gpu.device,
                &mut encoder,
                &bindings,
                true,
                layout.capacity as u32,
            );
            gpu.queue.submit([encoder.finish()]);

            // The fallback selection omits units whose hidden flags are still retained.
            gpu.queue
                .write_buffer(&page, 0, bytemuck::cast_slice(&layout.header(selected)));
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            masks.encode(&gpu.device, &mut encoder, &bindings, false, selected);
            let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("retained visibility readback"),
                size: page.size(),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&page, 0, &readback, 0, page.size());
            gpu.queue.submit([encoder.finish()]);
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, Result::unwrap);
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let retained: &[VisibleTerrainUnit] = bytemuck::cast_slice(&mapped);
            assert_eq!(
                &retained[1..=layout.capacity],
                descriptors,
                "fallback must clear every retained descriptor with {selected} references"
            );
            assert!(retained[layout.capacity].quad(0).is_some());
            assert_eq!(
                &mapped[layout.reference_offset() as usize..],
                bytemuck::cast_slice::<u32, u8>(&words[reference_start..]),
                "clearing visibility must preserve the reference region"
            );
        }
    }

    #[test]
    fn unchanged_visibility_mask_buffers_reuse_both_bind_groups() {
        let (device, _queue) = wgpu::Device::noop(&Default::default());
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
        masks.prepare(&device, &page, None, &mut bindings);
        let mut encoder = device.create_command_encoder(&Default::default());
        masks.encode(&device, &mut encoder, bindings.as_ref().unwrap(), true, 0);
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
