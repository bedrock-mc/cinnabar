//! Device-bounded immutable neutral artwork, replaced whole when its identity changes.
use super::*;
use crate::actor::{
    ActorArtworkPageId, ActorArtworkPages, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
    gpu::ActorDrawSpan,
};

pub(super) struct GpuArtworkPage {
    _texture: Texture,
    pub view: TextureView,
    pub bind_group: Option<BindGroup>,
    pub color_mask: bool,
    pub multitexture: bool,
}

#[derive(Default)]
pub(super) struct GpuArtwork {
    identity: Option<([u8; 32], [u8; 32])>,
    pub pages: Vec<GpuArtworkPage>,
    rejected: bool,
}

impl GpuArtwork {
    pub fn prepare(
        &mut self,
        pages: &ActorArtworkPages,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        let identity = (pages.identity, pages.entity_identity);
        if self.identity == Some(identity) {
            return !self.rejected;
        }
        if self.identity.take().is_some() {
            // Session packs change the artwork. wgpu keeps dropped pages alive until
            // work already submitted with them completes, so a generation is only
            // briefly doubled.
            self.pages.clear();
            self.rejected = false;
        }
        if pages.identity == [0; 32] {
            return true;
        }
        self.identity = Some(identity);
        let limits = device.limits();
        let bytes = pages
            .pages
            .iter()
            .try_fold(crate::actor::PLAYER_SKIN_BUDGET_BYTES, |total, page| {
                total.checked_add(page.rgba8.len())
            });
        if pages.pages.len() + 1 > MAX_ACTOR_TEXTURE_PAGES
            || bytes.is_none_or(|bytes| bytes > MAX_ACTOR_GPU_PIXEL_BYTES)
            || pages
                .pages
                .iter()
                .any(|page| page.layers > limits.max_texture_array_layers)
        {
            self.rejected = true;
            bevy::log::warn!(
                "neutral actor artwork exceeds device limits; generic artwork unavailable"
            );
            return false;
        }
        for page in pages.pages.iter() {
            // UVs are normalised, so a page past the device limit draws downscaled, not blank.
            let page = &page.fit_within(limits.max_texture_dimension_2d);
            let texture = device.create_texture_with_data(
                queue,
                &TextureDescriptor {
                    label: Some("immutable neutral binary-alpha actor page"),
                    size: Extent3d {
                        width: u32::from(page.width),
                        height: u32::from(page.height),
                        depth_or_array_layers: page.layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                TextureDataOrder::LayerMajor,
                &page.rgba8,
            );
            let view = texture.create_view(&TextureViewDescriptor {
                dimension: Some(TextureViewDimension::D2Array),
                ..default()
            });
            self.pages.push(GpuArtworkPage {
                _texture: texture,
                view,
                bind_group: None,
                color_mask: page.color_mask,
                multitexture: page.multitexture,
            });
        }
        true
    }

    pub fn invalidate_bindings(&mut self) {
        for page in &mut self.pages {
            page.bind_group = None;
        }
    }
}

/// Opaque instances share runs; blended instances retain individual sort positions.
pub(super) fn draw_spans(
    pages: &[ActorArtworkPageId],
    instances: &[crate::actor::ActorGpuInstance],
    geometry: &[crate::actor::ActorRigGeometrySpan],
) -> Vec<ActorDrawSpan> {
    let mut spans: Vec<ActorDrawSpan> = Vec::new();
    let mut last_geometry = None;
    for (index, (page, instance)) in pages.iter().copied().zip(instances).enumerate() {
        if let Some(span) = spans.last_mut().filter(|span| {
            span.page == page
                && !crate::actor::material::state(instance.material)
                    .is_some_and(|state| state.blend)
                && last_geometry == Some(instance.geometry_id)
                && span.material == instance.material
        }) {
            span.count += 1;
        } else {
            spans.push(ActorDrawSpan {
                material: instance.material,
                page,
                first: index as u32,
                count: 1,
                vertex_count: geometry
                    .get(instance.geometry_id as usize)
                    .map_or(0, |span| span.vertex_count),
            });
            last_geometry = Some(instance.geometry_id);
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blended_instances_keep_separate_sortable_spans_while_opaque_instances_batch() {
        let material = |blend| {
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                blend,
                ..Default::default()
            }))
        };
        let instances = [false, false, true, true].map(|blend| crate::actor::ActorGpuInstance {
            material: material(blend),
            ..Default::default()
        });
        let spans = draw_spans(
            &[1; 4],
            &instances,
            &[crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.first, span.count))
                .collect::<Vec<_>>(),
            [(0, 2), (2, 1), (3, 1)]
        );
        assert!(spans.iter().all(|span| span.vertex_count == 36));
    }

    #[test]
    fn high_artwork_page_ids_survive_paged_build_and_draw_spans() {
        use crate::actor::{
            ActorRenderIdentity, ActorRigFrameBuilder, ActorRigRenderInput, ActorRigRoute,
            ActorRigSubmission,
        };
        let textures = (1..=u16::from(u8::MAX) + 2)
            .map(|height| assets::ActorTexture {
                source: u32::from(height),
                width: 1,
                height,
                pixel_sha256: [1; 32],
                rgba8: vec![255; usize::from(height) * 4].into(),
            })
            .collect::<Vec<_>>();
        let bindings = [0, textures.len() as u32 - 1].map(|texture| assets::ActorArtworkBinding {
            rig: texture,
            geometry_candidate: texture,
            entity_symbol: texture,
            geometry: 0,
            render_controller: 0,
            texture,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        });
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &bindings);
        let low = pages.route(render_model::pack_rig_id(0)).unwrap();
        let high = pages
            .route(render_model::pack_rig_id(textures.len() as u32 - 1))
            .expect("valid high page retains its route");
        assert!(usize::from(high.page()) > usize::from(u8::MAX));
        let rig = render_model::pack_rig_id(0);
        let geometry =
            render_model::ActorRigGeometry::synthetic_cuboid(rig, [0.0; 3], [1.0; 3], 1).unwrap();
        let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
        let actor = |runtime_id| ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: 1,
                    dimension: 0,
                    runtime_id,
                    spawn_revision: 1,
                    ingress_sequence: 1,
                    source_tick: Some(1),
                    movement_revision: 1,
                    pose_generation: 1,
                    layer: crate::ACTOR_LAYER_BODY,
                },
                rig,
                previous_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                current_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                completed_tick: 1,
                reset_generation: 1,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            uv_anim: crate::IDENTITY_UV_ANIM,
            light: 0,
            overlay_rgba8: 0,
        };
        let page_of = |identity: &ActorRenderIdentity| {
            if identity.runtime_id == 2 {
                low.page()
            } else {
                high.page()
            }
        };
        let frame = builder.build_paged(0.5, None, [actor(1), actor(2), actor(3)], page_of);
        assert_eq!(frame.instances.len(), 3);
        let instance_pages = frame
            .manifest
            .iter()
            .map(|entry| page_of(&entry.identity))
            .collect::<Vec<_>>();
        let spans = draw_spans(&instance_pages, &frame.instances, &frame.geometry_spans);
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].page, spans[0].count), (low.page(), 1));
        assert_eq!((spans[1].page, spans[1].count), (high.page(), 2));
    }

    #[test]
    fn coplanar_dissolve_passes_have_separate_ordered_draw_spans() {
        let instances = [
            assets::EntityRenderMaterial::DissolveDepth,
            assets::EntityRenderMaterial::DissolveColor,
        ]
        .map(|material| crate::actor::ActorGpuInstance {
            material: material as u32,
            ..Default::default()
        });
        let spans = draw_spans(
            &[1, 1],
            &instances,
            &[crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(
            (spans[0].first, spans[0].count, spans[0].material),
            (0, 1, instances[0].material)
        );
        assert_eq!(
            (spans[1].first, spans[1].count, spans[1].material),
            (1, 1, instances[1].material)
        );
    }
    #[test]
    fn spans_split_on_page_and_geometry_and_carry_exact_vertex_counts() {
        let instance = |geometry_id| crate::actor::ActorGpuInstance {
            geometry_id,
            ..Default::default()
        };
        let geometry = [
            crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            },
            crate::actor::ActorRigGeometrySpan {
                first_vertex: 36,
                vertex_count: 3024,
            },
        ];
        let spans = draw_spans(
            &[0, 1, 1, 1, 2],
            &[
                instance(0),
                instance(1),
                instance(1),
                instance(0),
                instance(0),
            ],
            &geometry,
        );
        let span = |page, first, count, vertex_count| ActorDrawSpan {
            material: 0,
            page,
            first,
            count,
            vertex_count,
        };
        assert_eq!(
            spans,
            vec![
                span(0, 0, 1, 36),
                span(1, 1, 2, 3024),
                span(1, 3, 1, 36),
                span(2, 4, 1, 36),
            ]
        );
    }

    // A tall flipbook past the device limit draws downscaled instead of blanking every page.
    #[test]
    fn a_page_past_the_device_limit_uploads_downscaled() {
        use bevy::render::renderer::WgpuWrapper;
        use std::sync::Arc;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        let side = device.limits().max_texture_dimension_2d;
        let height = u16::try_from(side * 2).unwrap();
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.pages = Arc::from([crate::actor::ActorTexturePage {
            width: 2,
            height,
            layers: 1,
            rgba8: vec![255; 2 * usize::from(height) * 4].into(),
            color_mask: true,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.pages[0].color_mask);
    }

    #[test]
    fn replacement_artwork_supersedes_the_previous_generation() {
        use bevy::render::renderer::WgpuWrapper;
        use std::sync::Arc;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.entity_identity = [2; 32];
        pages.pages = Arc::from([crate::actor::ActorTexturePage {
            width: 16,
            height: 16,
            layers: 1,
            rgba8: vec![255; 16 * 16 * 4].into(),
            color_mask: false,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.prepare(&pages, &device, &queue));
        pages.entity_identity = [3; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert_eq!(gpu.identity, Some(([1; 32], [3; 32])));
        pages.identity = [0; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert!(gpu.pages.is_empty() && gpu.identity.is_none());
    }
}
