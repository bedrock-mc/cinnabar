//! Retains scene-copy bindings for Bevy's two alternating post-process source views.

use super::host_pipelines::HostPipelines;
use wgpu::{BindGroup, Device, TextureFormat, TextureView};

/// Bindings for one source colour/depth pair and presentation format.
pub(super) struct SceneBindings {
    colour_source: TextureView,
    depth_source: TextureView,
    format: TextureFormat,
    pub colour: BindGroup,
    pub depth: Option<BindGroup>,
    pub comparison_depth: Option<BindGroup>,
    pub preserve: Option<BindGroup>,
}

impl SceneBindings {
    /// Builds the host bindings that sample this view's native scene.
    fn new(
        device: &Device,
        host: &HostPipelines,
        parameters: &wgpu::Buffer,
        colour: &TextureView,
        depth: &TextureView,
        format: TextureFormat,
    ) -> Self {
        let binding = |label, pipeline: &wgpu::RenderPipeline, entries: &[_]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &pipeline.get_bind_group_layout(0),
                entries,
            })
        };
        let depth_binding = |pipeline: &wgpu::RenderPipeline| {
            binding(
                "shader pack scene depth",
                pipeline,
                &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: parameters.as_entire_binding(),
                    },
                ],
            )
        };
        Self {
            colour_source: colour.clone(),
            depth_source: depth.clone(),
            format,
            colour: binding(
                "shader pack scene colour",
                &host.scene_colour,
                &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(colour),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: parameters.as_entire_binding(),
                    },
                ],
            ),
            depth: host.scene_depth.as_ref().map(depth_binding),
            comparison_depth: host.scene_depth_attachment.as_ref().map(depth_binding),
            preserve: host
                .preserve
                .iter()
                .find(|(f, _)| *f == format)
                .map(|(_, pipeline)| {
                    binding(
                        "shader pack preserve viewport exterior",
                        pipeline,
                        &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(colour),
                        }],
                    )
                }),
        }
    }
}

/// A bounded cache covering both sides of the native post-process target.
#[derive(Default)]
pub(super) struct SceneBindingCache {
    entries: [Option<SceneBindings>; 2],
    current: usize,
}

impl SceneBindingCache {
    /// Selects cached bindings or replaces the next slot when scene resources change.
    pub fn prepare(
        &mut self,
        device: &Device,
        host: &HostPipelines,
        parameters: &wgpu::Buffer,
        colour: &TextureView,
        depth: &TextureView,
        format: TextureFormat,
    ) {
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.as_ref().is_some_and(|entry| {
                entry.colour_source == *colour
                    && entry.depth_source == *depth
                    && entry.format == format
            })
        }) {
            self.current = index;
            return;
        }
        self.current = (self.current + 1) % self.entries.len();
        self.entries[self.current] = Some(SceneBindings::new(
            device, host, parameters, colour, depth, format,
        ));
    }

    /// Returns the bindings selected for the current frame.
    pub fn current(&self) -> &SceneBindings {
        self.entries[self.current]
            .as_ref()
            .expect("scene bindings were prepared")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternating_scene_sources_reuse_bindings_and_refresh_changed_depth() {
        let (device, _) = wgpu::Device::noop(&Default::default());
        let host = super::super::host_pipelines::host_pipelines(
            &device,
            &[None; super::super::plan::MAX_COLOR_BUFFERS as usize],
            TextureFormat::R32Float,
            true,
            [true; 3],
            &[],
            &[],
            None,
        );
        let texture = |format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 4,
                        height: 4,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let colours = std::array::from_fn::<_, 2, _>(|_| texture(render_scene::SCENE_COLOR_FORMAT));
        let depth = texture(TextureFormat::Depth32Float);
        let parameters = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<super::super::passes::SceneDepthParameters>() as u64,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let mut cache = SceneBindingCache::default();
        let initial = colours.each_ref().map(|colour| {
            cache.prepare(
                &device,
                &host,
                &parameters,
                colour,
                &depth,
                render_scene::SCENE_COLOR_FORMAT,
            );
            let groups = cache.current();
            (
                groups.colour.clone(),
                groups.depth.clone(),
                groups.comparison_depth.clone(),
                groups.preserve.clone(),
            )
        });
        for _ in 0..3 {
            for (colour, expected) in colours.iter().zip(&initial) {
                cache.prepare(
                    &device,
                    &host,
                    &parameters,
                    colour,
                    &depth,
                    render_scene::SCENE_COLOR_FORMAT,
                );
                let groups = cache.current();
                assert_eq!(
                    (
                        &groups.colour,
                        &groups.depth,
                        &groups.comparison_depth,
                        &groups.preserve
                    ),
                    (&expected.0, &expected.1, &expected.2, &expected.3)
                );
            }
        }
        let replacement = texture(TextureFormat::Depth32Float);
        cache.prepare(
            &device,
            &host,
            &parameters,
            &colours[0],
            &replacement,
            render_scene::SCENE_COLOR_FORMAT,
        );
        assert_ne!(cache.current().depth, initial[0].1);
        assert_ne!(cache.current().comparison_depth, initial[0].2);
    }
}
