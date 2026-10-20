//! Keeps each animated display sprite on the frame the world animation clock selects.
//!
//! Frame pixels live in a separate frame store, because a texture subresource cannot be
//! sampled and written in one pass. Non-blending frames are copied mip by mip; blending
//! animations draw one instanced quad per display sprite and mip that mixes the current and
//! next frame in the sampled colour space, as the chunk shaders do. One pass per mip writes
//! every animated atlas (colour and, with LabPBR frames, normal and specular) as attachments.

use super::pack::SpriteRect;
use super::plan::AnimationLayout;
use super::upload::{COLOR_STORAGE_FORMAT, LayerTextures, PBR_FORMAT, UploadedAtlas};
use crate::queue_writes::ProfiledQueue as _;
use assets::{Animation, Material, TextureRef};
use bevy::prelude::Resource;
use render_scene::{ChunkAnimationClock, select_animation_frames};

/// Vertex-buffer layout of [`BlendInstance`].
pub(crate) const BLEND_INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Uint32x4, 1 => Uint32x2, 2 => Uint32x4, 3 => Float32];
/// The colour, normal and specular frame stores the blend pass reads with `textureLoad`.
pub(crate) const BLEND_SOURCE_ENTRIES: [wgpu::BindGroupLayoutEntry; 3] =
    [blend_source(0), blend_source(1), blend_source(2)];

const fn blend_source(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
/// Two triangles per display sprite.
const QUAD_VERTICES: u32 = 6;

/// One display sprite at one mip, as `blend.wgsl` reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BlendInstance {
    target: [u32; 4],
    extent: [u32; 2],
    sources: [u32; 4],
    blend: f32,
    _padding: u32,
}

/// The frames one display sprite shows for a clock value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SlotUpdate {
    pub display: SpriteRect,
    pub current: SpriteRect,
    pub next: SpriteRect,
    /// Weight of `next`; zero copies `current`.
    pub blend: f32,
}

/// Display refreshes for `clock`, skipping animations whose shown frames and blend are unchanged.
/// Frame choice and weight are exactly `select_animation_frames`.
pub(crate) fn select_updates(
    animations: &[Animation],
    frames: &[TextureRef],
    layouts: &[Option<AnimationLayout>],
    shown: &[Option<SlotUpdate>],
    clock: ChunkAnimationClock,
) -> Vec<(usize, SlotUpdate)> {
    layouts
        .iter()
        .enumerate()
        .filter_map(|(index, layout)| {
            let layout = layout.as_ref()?;
            let material = Material {
                animation: u32::try_from(index).ok()?,
                ..Material::unvaried()
            };
            let sample = select_animation_frames(material, animations, frames, clock);
            let current = layout.frame(sample.current)?;
            let next = layout.frame(sample.next)?;
            let update = if sample.blend > 0.0 && current != next {
                SlotUpdate {
                    display: layout.display,
                    current,
                    next,
                    blend: sample.blend,
                }
            } else {
                SlotUpdate {
                    display: layout.display,
                    current,
                    next: current,
                    blend: 0.0,
                }
            };
            (shown.get(index).copied().flatten() != Some(update)).then_some((index, update))
        })
        .collect()
}

/// One atlas texture and the frame store holding its animation frames.
struct AnimatedLayer {
    atlas: wgpu::Texture,
    frames: wgpu::Texture,
    frames_view: wgpu::TextureView,
    /// Render target of each atlas mip, in the storage format.
    targets: Vec<wgpu::TextureView>,
}

/// Blend pipelines writing the encoded colour atlas alone, or it and both linear LabPBR atlases.
#[derive(Clone, Copy)]
pub(crate) struct BlendPipelines<'a> {
    pub colour: &'a wgpu::RenderPipeline,
    pub layers: &'a wgpu::RenderPipeline,
}

/// Animated display state of the published atlas generation.
#[derive(Resource)]
pub(crate) struct AtlasAnimations {
    /// Colour first, then normal and specular when the frame store carries LabPBR frames.
    layers: Vec<AnimatedLayer>,
    /// Every layer's frame store, bound for the blend pass.
    blend_sources: Option<wgpu::BindGroup>,
    animations: Box<[Animation]>,
    frames: Box<[TextureRef]>,
    layouts: Vec<Option<AnimationLayout>>,
    shown: Vec<Option<SlotUpdate>>,
    mip_levels: u32,
    extent: [u32; 2],
    instances: Option<wgpu::Buffer>,
    /// Retains the published textures for as long as their views are bound.
    _textures: (LayerTextures, Option<LayerTextures>),
}

impl AtlasAnimations {
    /// Tracks `uploaded`'s display sprites; every slot refreshes on the first update.
    pub fn new(
        uploaded: &UploadedAtlas,
        animations: &[Animation],
        frames: &[TextureRef],
        layouts: Vec<Option<AnimationLayout>>,
        mip_levels: u32,
        extent: [u32; 2],
    ) -> Self {
        let mut layers = Vec::new();
        if let Some(store) = &uploaded.frames {
            let mut pair = |atlas: &wgpu::Texture, frames: &wgpu::Texture, srgb: bool| {
                let format = if srgb {
                    COLOR_STORAGE_FORMAT
                } else {
                    PBR_FORMAT
                };
                layers.push(AnimatedLayer {
                    atlas: atlas.clone(),
                    frames: frames.clone(),
                    frames_view: frames.create_view(&wgpu::TextureViewDescriptor {
                        label: Some("shader-pack frame store"),
                        format: Some(format),
                        ..Default::default()
                    }),
                    targets: (0..mip_levels)
                        .map(|level| {
                            atlas.create_view(&wgpu::TextureViewDescriptor {
                                label: Some("shader-pack animated atlas mip"),
                                format: Some(format),
                                base_mip_level: level,
                                mip_level_count: Some(1),
                                ..Default::default()
                            })
                        })
                        .collect(),
                });
            };
            pair(&uploaded.atlas.color, &store.color, true);
            if let (Some(atlas), Some(store)) = (&uploaded.atlas.pbr, &store.pbr) {
                pair(&atlas[0], &store[0], false);
                pair(&atlas[1], &store[1], false);
            }
        }
        Self {
            layers,
            blend_sources: None,
            animations: animations.into(),
            frames: frames.into(),
            shown: vec![None; layouts.len()],
            layouts,
            mip_levels,
            extent,
            instances: None,
            _textures: (uploaded.atlas.clone(), uploaded.frames.clone()),
        }
    }

    /// Whether blend sources still need a bind group.
    pub fn needs_blend_sources(&self) -> bool {
        !self.layers.is_empty() && self.blend_sources.is_none()
    }

    /// Binds the frame stores for the blend pass with the pipelines' group-zero layout. Without
    /// LabPBR frames the colour store fills the unread normal and specular bindings.
    pub fn bind_blend_sources(&mut self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout) {
        let entries = BLEND_SOURCE_ENTRIES.map(|entry| wgpu::BindGroupEntry {
            binding: entry.binding,
            resource: wgpu::BindingResource::TextureView(
                &self
                    .layers
                    .get(entry.binding as usize)
                    .unwrap_or(&self.layers[0])
                    .frames_view,
            ),
        });
        self.blend_sources = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader-pack frame blend sources"),
            layout,
            entries: &entries,
        }));
    }

    /// Records the refreshes `clock` requires and returns how many display sprites changed.
    /// Without blend pipelines, blending sprites show their current frame and retry later.
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        clock: ChunkAnimationClock,
        pipelines: Option<BlendPipelines<'_>>,
    ) -> usize {
        if self.layers.is_empty() {
            return 0;
        }
        let updates = select_updates(
            &self.animations,
            &self.frames,
            &self.layouts,
            &self.shown,
            clock,
        );
        let pipelines = pipelines.filter(|_| !self.needs_blend_sources());
        let mut blended = Vec::new();
        for &(index, update) in &updates {
            if update.blend > 0.0 && pipelines.is_some() {
                blended.push(update);
                self.shown[index] = Some(update);
                continue;
            }
            for layer in &self.layers {
                for level in 0..self.mip_levels {
                    copy_square(encoder, layer, level, update.current, update.display);
                }
            }
            self.shown[index] = (update.blend == 0.0).then_some(update);
        }
        if let (Some(pipelines), false) = (pipelines, blended.is_empty()) {
            self.encode_blends(device, queue, encoder, pipelines, &blended);
        }
        updates.len()
    }

    fn encode_blends(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: BlendPipelines<'_>,
        blended: &[SlotUpdate],
    ) {
        let instances = (0..self.mip_levels)
            .flat_map(|level| {
                let extent = self.extent.map(|axis| axis >> level);
                blended.iter().map(move |update| {
                    let display = update.display.at_mip(level);
                    let current = update.current.at_mip(level);
                    let next = update.next.at_mip(level);
                    BlendInstance {
                        target: [display.x, display.y, display.side, level],
                        extent,
                        sources: [current.x, current.y, next.x, next.y],
                        blend: update.blend,
                        _padding: 0,
                    }
                })
            })
            .collect::<Vec<_>>();
        let bytes: &[u8] = bytemuck::cast_slice(&instances);
        if self
            .instances
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
        {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader-pack frame blend instances"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buffer = self.instances.as_ref().expect("instances were allocated");
        queue.profiled_write_buffer(buffer, 0, bytes);
        let per_level = (blended.len() * size_of::<BlendInstance>()) as u64;
        let bind_group = self
            .blend_sources
            .as_ref()
            .expect("blend sources are bound");
        let pipeline = if self.layers.len() == 1 {
            pipelines.colour
        } else {
            pipelines.layers
        };
        for level in 0..self.mip_levels {
            let attachments: [_; 3] = std::array::from_fn(|index| {
                self.layers
                    .get(index)
                    .map(|layer| wgpu::RenderPassColorAttachment {
                        view: &layer.targets[level as usize],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shader-pack animated sprite blend"),
                color_attachments: &attachments[..self.layers.len()],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            let start = u64::from(level) * per_level;
            pass.set_vertex_buffer(0, buffer.slice(start..start + per_level));
            pass.draw(0..QUAD_VERTICES, 0..blended.len() as u32);
        }
    }
}

/// Copies one frame-store square into a display square at `level`.
fn copy_square(
    encoder: &mut wgpu::CommandEncoder,
    layer: &AnimatedLayer,
    level: u32,
    source: SpriteRect,
    target: SpriteRect,
) {
    let (source, target) = (source.at_mip(level), target.at_mip(level));
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &layer.frames,
            mip_level: level,
            origin: wgpu::Origin3d {
                x: source.x,
                y: source.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: &layer.atlas,
            mip_level: level,
            origin: wgpu::Origin3d {
                x: target.x,
                y: target.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: target.side,
            height: target.side,
            depth_or_array_layers: 1,
        },
    );
}
