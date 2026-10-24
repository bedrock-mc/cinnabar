//! Compact terrain tables for devices with a small per-stage storage budget.

use crate::chunk::*;
use bevy::render::renderer::{RenderContext, RenderGraph};

/// One packed table and its original buffer, retained until graph encoding.
struct Table {
    source: wgpu::Buffer,
    offset: u64,
    dirty: bool,
}

/// Retains compact tables; immutable tables are copied only after a binding change.
#[derive(Resource, Default)]
pub(in crate::chunk) struct CompactTerrain {
    buffer: Option<Buffer>,
    tables: Vec<Table>,
}

impl CompactTerrain {
    /// Packs the storage entries absent from the compact layout, preserving their addresses.
    pub(super) fn prepare(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        entries: &mut Vec<BindGroupEntry<'_>>,
    ) -> bool {
        let layout = super::super::pipeline::layouts::compact_chunk_bind_group_layout();
        let packed: Vec<_> = entries
            .iter()
            .filter_map(|entry| {
                let BindingResource::Buffer(binding) = &entry.resource else {
                    return None;
                };
                (entry.binding == 2
                    || !layout
                        .entries
                        .iter()
                        .any(|kept| kept.binding == entry.binding))
                .then_some((entry.binding, binding.buffer.clone()))
            })
            .collect();
        let header_words = (TRANSPARENT_REFS_BINDING as usize + 1) * 2;
        let mut header = vec![0u32; header_words];
        let mut size = (header_words * size_of::<u32>()) as u64;
        self.tables.clear();
        for (binding, source) in packed {
            header[binding as usize * 2] = (size / 4) as u32;
            header[binding as usize * 2 + 1] = (source.size() / 4) as u32;
            self.tables.push(Table {
                source: source.clone(),
                offset: size,
                dirty: true,
            });
            size += source.size();
        }
        let limits = device.limits();
        if size > limits.max_storage_buffer_binding_size || size > limits.max_buffer_size {
            bevy::log::error!(
                size,
                "compact terrain tables exceed the device's storage binding size"
            );
            self.buffer = None;
            return false;
        }
        let Some(buffer) = crate::gpu_allocation::buffer(
            device,
            &BufferDescriptor {
                label: Some("compact terrain tables"),
                size,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        ) else {
            self.buffer = None;
            return false;
        };
        use crate::queue_writes::ProfiledQueue as _;
        queue.profiled_write_buffer(&buffer, 0, bytemuck::cast_slice(&header));
        self.buffer = Some(buffer);
        entries.retain(|entry| {
            layout
                .entries
                .iter()
                .any(|kept| kept.binding == entry.binding)
        });
        true
    }

    /// Binds the retained packed buffer after preparation has released the source entries.
    pub(super) fn binding(&self) -> BindingResource<'_> {
        self.buffer
            .as_ref()
            .expect("compact terrain was prepared")
            .as_entire_binding()
    }

    /// Records table copies after any uploads to their original buffers.
    fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Self { buffer, tables } = self;
        let Some(buffer) = buffer else {
            return;
        };
        for table in tables {
            if table.dirty || table.source.usage().contains(BufferUsages::COPY_DST) {
                encoder.copy_buffer_to_buffer(
                    &table.source,
                    0,
                    buffer,
                    table.offset,
                    table.source.size(),
                );
                table.dirty = false;
            }
        }
    }
}

/// Installs copies after staged terrain uploads and before any camera reads the tables.
pub(in crate::chunk) fn install(world: &mut World) {
    if world
        .resource::<RenderDevice>()
        .limits()
        .max_storage_buffers_per_shader_stage
        >= required_vertex_storage_buffers()
    {
        return;
    }
    let _ = world.try_schedule_scope(RenderGraph, |_, graph| {
        graph.add_systems(
            copy_tables
                .after(crate::upload_staging::BufferUploads)
                .before(bevy::core_pipeline::schedule::camera_driver)
                .in_set(bevy::render::renderer::RenderGraphSystems::Render),
        );
    });
}

/// Refreshes mutable tables each rendered frame and immutable tables only once.
fn copy_tables(mut compact: ResMut<CompactTerrain>, mut context: RenderContext) {
    if compact.buffer.is_some() {
        compact.encode(context.command_encoder());
    }
}

#[cfg(test)]
mod tests;
