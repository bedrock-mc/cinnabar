//! A view's actor draws and their planning: skinning jobs, kernel dispatches, texture
//! views and per-draw slots.

use std::collections::HashMap;
use std::ops::Range;

use bevy::math::Vec3;
use bevy::render::render_resource::{Texture, TextureId};
use wgpu::{Device, Queue, TextureView};

use super::classify::{
    DrawValues, ENTITY_GLINT_SCALE, ITEM_GLINT_SCALE, classify, entity_colour,
    glint_texture_matrix, vertex_colour,
};
use super::skin::{JOB_GLINT, JOB_ONE_SIDED, JOB_VIEW_SPACE, SkinJob};
use super::{ActorSources, BlockEntitySource, Hand, LayerTexture, RigSource};
use crate::shaderpack::atlas::{NEUTRAL_NORMAL, NEUTRAL_SPECULAR};
use crate::shaderpack::draws::DrawKind;

/// A texture layer's identity: the texture and the layer.
pub(super) type TextureKey = (TextureId, u32);

/// One actor draw: its vertices in the view's stream, its texture and its values.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ActorDraw {
    pub(super) kind: DrawKind,
    pub(super) vertices: Range<u64>,
    /// Camera-relative depth used by translucent draw ordering.
    pub(super) distance: f32,
    pub(super) texture: TextureKey,
    pub(super) values: DrawValues,
}

/// Measures world geometry along the view and hand geometry in its existing view space.
pub(super) fn draw_depth(position: Vec3, camera: Vec3, forward: Vec3, hand: bool) -> f32 {
    if hand {
        -position.z
    } else {
        (position - camera).dot(forward)
    }
}

/// One draw's uniform copy and the bind groups over it and its texture.
pub(super) struct Slot {
    pub(super) uniforms: wgpu::Buffer,
    pub(super) texture: Option<TextureKey>,
    /// Per [`crate::shaderpack::targets::ViewTargets::parity`].
    pub(super) groups: [crate::shaderpack::targets::Groups; 2],
}

/// A storage buffer that grows to the largest size asked of it.
#[derive(Default)]
pub(super) struct Growable {
    pub(super) buffer: Option<wgpu::Buffer>,
}

impl Growable {
    pub(super) fn ensure(
        &mut self,
        device: &Device,
        label: &str,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> &wgpu::Buffer {
        let size = size.max(16).next_multiple_of(16);
        if self.buffer.as_ref().is_none_or(|b| b.size() < size) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: super::pages::allocation_size(
                    size,
                    if usage.contains(wgpu::BufferUsages::STORAGE) {
                        u64::from(device.limits().max_storage_buffer_binding_size)
                    } else {
                        device.limits().max_buffer_size
                    },
                ),
                usage,
                mapped_at_creation: false,
            }));
        }
        self.buffer.as_ref().expect("just ensured")
    }
}

/// One kernel dispatch of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dispatch {
    /// Rig jobs `first..first + count` of the actors (`hand == false`) or the hand.
    Rig {
        hand: bool,
        first: u32,
        count: u32,
        max_vertices: u32,
    },
    /// Block-entity list `list` (0 solid, 1 overlay, 2 additive) into `first_output`.
    BlockEntities {
        list: usize,
        first_output: u64,
        count: u32,
    },
}

/// Retained allocations for one device-sized output page.
#[derive(Default)]
pub(super) struct PageBuffers {
    pub first: u64,
    pub vertices: u32,
    pub output: Growable,
    pub jobs: Growable,
    pub params: Growable,
}

/// A view's actor geometry: skinned vertices, draws and their bindings.
#[derive(Default)]
pub(crate) struct ViewActors {
    pub(super) draws: Vec<ActorDraw>,
    /// Each draw's slot within its kind, parallel to `draws`.
    pub(super) slot_of: Vec<u32>,
    pub(super) jobs: Vec<SkinJob>,
    pub(super) dispatches: Vec<Dispatch>,
    pub(super) slots: HashMap<DrawKind, Vec<Slot>>,
    pub(super) views: HashMap<TextureKey, (TextureView, u64)>,
    pub(super) pages: Vec<PageBuffers>,
    pub(super) neutral: Option<NeutralTextures>,
    pub(super) frame: u64,
    pub(super) scratch: Vec<u8>,
}

/// LabPBR's neutral `normals` and `specular`, bound for every actor texture.
pub(super) struct NeutralTextures {
    pub(super) normals: TextureView,
    pub(super) specular: TextureView,
}

impl NeutralTextures {
    pub(super) fn new(device: &Device, queue: &Queue) -> Self {
        let texel = |label: &str, rgba: [u8; 4]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                texture.size(),
            );
            texture.create_view(&Default::default())
        };
        Self {
            normals: texel("shader-pack actor normals", NEUTRAL_NORMAL),
            specular: texel("shader-pack actor specular", NEUTRAL_SPECULAR),
        }
    }
}

/// A single-layer 2D view of `texture`, in its non-sRGB format.
fn layer_view(texture: &Texture, layer: u32) -> TextureView {
    wgpu::Texture::create_view(
        texture,
        &wgpu::TextureViewDescriptor {
            label: Some("shader-pack actor texture layer"),
            format: Some(texture.format().remove_srgb_suffix()),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        },
    )
}

impl ViewActors {
    /// Drops this frame's work, keeping allocations.
    pub(super) fn begin(&mut self) {
        self.draws.clear();
        self.slot_of.clear();
        self.jobs.clear();
        self.dispatches.clear();
        self.frame += 1;
    }

    /// The key of `texture`'s view, creating the view on first use.
    fn view(&mut self, texture: LayerTexture<'_>) -> TextureKey {
        let key = (texture.texture.id(), texture.layer);
        let frame = self.frame;
        self.views
            .entry(key)
            .and_modify(|(_, used)| *used = frame)
            .or_insert_with(|| (layer_view(texture.texture, texture.layer), frame));
        key
    }

    fn push(
        &mut self,
        kind: DrawKind,
        vertices: Range<u64>,
        texture: TextureKey,
        values: DrawValues,
        distance: f32,
    ) {
        self.draws.push(ActorDraw {
            kind,
            vertices,
            texture,
            values,
            distance,
        });
    }

    /// Returns draw indices in camera order, preserving each draw's retained slot association.
    pub(super) fn draw_order(&self, kind: DrawKind) -> Vec<usize> {
        let mut order: Vec<_> = self
            .draws
            .iter()
            .enumerate()
            .filter_map(|(index, draw)| (draw.kind == kind).then_some(index))
            .collect();
        if matches!(
            kind,
            DrawKind::EntitiesTranslucent | DrawKind::BlockTranslucent | DrawKind::HandWater
        ) {
            order.sort_by(|a, b| self.draws[*b].distance.total_cmp(&self.draws[*a].distance));
        }
        order
    }

    /// Plans the jobs and draws of one rig source; `output` is the next free output vertex.
    pub(super) fn plan_rig(
        &mut self,
        source: &dyn RigSource,
        hand: bool,
        sources: &ActorSources<'_>,
        drawn: &dyn Fn(DrawKind) -> bool,
        output: &mut u64,
    ) {
        let first_job = self.jobs.len() as u32;
        let mut max_vertices = 0;
        let glint_keys = [false, true].map(|item| {
            source
                .glint(item)
                .map(|texture| self.view(LayerTexture { texture, layer: 0 }))
        });
        source.for_each_instance(&mut |instance| {
            let data = instance.data;
            let Some(class) = classify(data.material, data.glint[5], instance.blend, hand) else {
                return;
            };
            let camera = (instance.camera && drawn(class.kind)).then_some(class.kind);
            let shadow = class.shadow && drawn(DrawKind::ShadowEntities);
            if (camera.is_none() && !shadow) || instance.vertex_count < 3 {
                return;
            }
            let vertex_count = instance.vertex_count - instance.vertex_count % 3;
            let texture = self.view(instance.texture);
            let first = *output;
            let job = SkinJob {
                instance: instance.index,
                first_output: first as u32,
                first_output_high: (first >> 32) as u32,
                vertex_count,
                flags: if hand { JOB_VIEW_SPACE } else { 0 }
                    | if class.one_sided { JOB_ONE_SIDED } else { 0 },
                light: u32::from(instance.light[0].min(15))
                    | (u32::from(instance.light[1].min(15)) << 4),
                colour: vertex_colour(
                    data.tint,
                    matches!(
                        class.kind,
                        DrawKind::EntitiesTranslucent | DrawKind::HandWater
                    ),
                ),
                block_id: -1,
            };
            *output += u64::from(job.output_vertices());
            max_vertices = max_vertices.max(vertex_count);
            self.jobs.push(job);
            let entity_id = instance
                .runtime_id
                .zip(sources.entity_ids)
                .map_or(-1, |(runtime_id, ids)| ids.entity_id(runtime_id));
            let item_id = match instance.held {
                Some(Hand::Main) => sources.held_items[0],
                Some(Hand::Off) => sources.held_items[1],
                None => -1,
            };
            let values = DrawValues {
                entity_id,
                item_id,
                entity_color: entity_colour(data.overlay_rgba8, data.material),
                ..DrawValues::NONE
            };
            let distance = draw_depth(
                Vec3::new(
                    data.world_from_actor[0][3],
                    data.world_from_actor[1][3],
                    data.world_from_actor[2][3],
                ),
                sources.camera,
                sources.view_forward,
                hand,
            );
            let vertices = first..*output;
            if let Some(kind) = camera {
                self.push(kind, vertices.clone(), texture, values, distance);
                let item = data.glint[5] > 0.0 || instance.held.is_some();
                let glint_kind = if hand {
                    DrawKind::HandGlint
                } else if kind == DrawKind::EntitiesTranslucent {
                    DrawKind::ArmorGlintTranslucent
                } else {
                    DrawKind::ArmorGlint
                };
                if class.glint
                    && data.glint[2] > 0.0
                    && drawn(glint_kind)
                    && let Some(glint) = glint_keys[usize::from(item)]
                {
                    let scale = if item {
                        ITEM_GLINT_SCALE
                    } else {
                        ENTITY_GLINT_SCALE
                    };
                    let texture_matrix = glint_texture_matrix(data.glint, scale);
                    let glint_first = *output;
                    self.jobs.push(SkinJob {
                        first_output: glint_first as u32,
                        first_output_high: (glint_first >> 32) as u32,
                        flags: job.flags | JOB_GLINT,
                        colour: render_model::pack_overlay_rgba8([data.glint[2]; 4]),
                        ..job
                    });
                    *output += u64::from(job.output_vertices());
                    self.push(
                        glint_kind,
                        glint_first..*output,
                        glint,
                        DrawValues {
                            texture_matrix,
                            ..values
                        },
                        distance,
                    );
                }
            }
            if shadow {
                self.push(
                    DrawKind::ShadowEntities,
                    vertices,
                    texture,
                    values,
                    distance,
                );
            }
        });
        let count = self.jobs.len() as u32 - first_job;
        if count > 0 {
            self.dispatches.push(Dispatch::Rig {
                hand,
                first: first_job,
                count,
                max_vertices,
            });
        }
    }

    /// Plans the block-entity conversions and draws.
    pub(super) fn plan_block_entities(
        &mut self,
        source: &BlockEntitySource<'_>,
        drawn: &dyn Fn(DrawKind) -> bool,
        output: &mut u64,
    ) {
        let texture = self.view(LayerTexture {
            texture: source.atlas,
            layer: 0,
        });
        let lists = [
            (source.solid, DrawKind::Block, true),
            (source.overlay, DrawKind::BlockTranslucent, false),
        ];
        for (list_index, (list, kind, casts)) in lists.into_iter().enumerate() {
            let Some(list) = list.filter(|list| list.count >= 3) else {
                continue;
            };
            let shadow = casts && drawn(DrawKind::ShadowBlock);
            if !drawn(kind) && !shadow {
                continue;
            }
            let count = list.count - list.count % 3;
            let first = *output;
            *output += u64::from(count);
            self.dispatches.push(Dispatch::BlockEntities {
                list: list_index,
                first_output: first,
                count,
            });
            let vertices = first..*output;
            if drawn(kind) {
                self.push(kind, vertices.clone(), texture, DrawValues::NONE, 0.0);
            }
            if shadow {
                self.push(
                    DrawKind::ShadowBlock,
                    vertices,
                    texture,
                    DrawValues::NONE,
                    0.0,
                );
            }
        }
    }
}
