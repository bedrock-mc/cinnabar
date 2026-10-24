//! The world's actor rig as a shader-pack [`RigSource`]: the prepared instance, geometry and
//! bone buffers the actor pass draws from, with each instance's artwork layer and actor.

use super::*;
use crate::shaderpack::actors::{LayerCompose, LayerTexture, RigBuffers, RigInstance, RigSource};

/// The prepared actor rig, borrowed for one frame.
pub(crate) struct ShaderpackActors<'a> {
    gpu: &'a ActorGpu,
    buffers: RigBuffers<'a>,
}

/// The actor rig when it has instances and uploaded geometry.
pub(crate) fn shaderpack_actors(world: &World) -> Option<ShaderpackActors<'_>> {
    let gpu = world.get_resource::<ActorGpu>()?;
    if gpu.instance_count == 0 || !gpu.geometry_ready {
        return None;
    }
    let buffers = RigBuffers {
        instances: &gpu.instance_buffer,
        vertices: gpu.geometry_vertices.buffer()?,
        spans: gpu.geometry_span_buffer.as_ref()?,
        previous_bones: &gpu.previous_bone_buffer,
        current_bones: &gpu.current_bone_buffer,
    };
    Some(ShaderpackActors { gpu, buffers })
}

/// Light levels of a packed actor light word; unlit materials read full light.
fn light_levels(light: u32) -> [u8; 2] {
    if light & render_api::ACTOR_LIGHT_WORLD == 0 {
        return [15, 15];
    }
    [(light & 15) as u8, ((light >> 4) & 15) as u8]
}

impl RigSource for ShaderpackActors<'_> {
    fn buffers(&self) -> RigBuffers<'_> {
        self.buffers
    }

    fn for_each_instance(&self, visit: &mut dyn FnMut(RigInstance<'_>)) {
        let gpu = self.gpu;
        // Mask-only instances follow the camera-drawn prefix and still cast pack shadows.
        let camera_count = gpu.main_manifest.len() as u32;
        let shadow_only: Vec<_> = gpu
            .mask_spans
            .iter()
            .filter_map(|span| {
                let first = span.first.max(camera_count);
                let end = span.first + span.count;
                (first < end).then(|| crate::actor::gpu::ActorDrawSpan {
                    first,
                    count: end - first,
                    ..*span
                })
            })
            .collect();
        for span in gpu.spans.iter().chain(&shadow_only) {
            if !gpu.pack_supports_span(span) {
                continue;
            }
            if span.page != 0 && !gpu.artwork_current {
                continue;
            }
            let page = (span.page != 0)
                .then(|| gpu.artwork.page(usize::from(span.page) - 1))
                .flatten();
            for index in span.first..span.first + span.count {
                let Some(data) = gpu.instances.get(index as usize) else {
                    continue;
                };
                let layer = data.texture_layer & 0x00ff_ffff;
                let texture = match page {
                    Some(page) => page.raw_texture(),
                    None if span.page == 0 => {
                        gpu.skins.raw_texture((data.texture_layer >> 24) as usize)
                    }
                    None => None,
                };
                let Some(texture) = texture else {
                    continue;
                };
                visit(RigInstance {
                    index,
                    data,
                    vertex_count: span.vertex_count,
                    texture: LayerTexture {
                        texture,
                        layer,
                        compose: page.map_or(LayerCompose::Plain, |page| {
                            LayerCompose::for_page(page.color_mask, page.multitexture, data)
                        }),
                    },
                    runtime_id: gpu
                        .manifest
                        .get(index as usize)
                        .map(|entry| entry.identity.runtime_id),
                    camera: index < camera_count,
                    light: light_levels(data.light),
                    blend: None,
                    held: None,
                });
            }
        }
    }

    fn glint(&self, item: bool) -> Option<&Texture> {
        let glint = if item {
            &self.gpu.artwork.item_glint
        } else {
            &self.gpu.artwork.glint
        };
        glint.raw_texture()
    }
}

#[cfg(test)]
mod tests {
    use super::light_levels;

    #[test]
    fn world_lit_actors_carry_their_levels_and_unlit_ones_read_full_light() {
        let lit = render_model::pack_actor_light(7, 12);
        assert_eq!(light_levels(lit), [7, 12]);
        assert_eq!(
            light_levels(render_model::pack_actor_light_without_lightmap()),
            [15, 15]
        );
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn unsupported_actor_material_leaves_only_its_span_native() {
        use crate::actor_render::tests::allocation_fault_tests::{
            draw_frame, fixture, publication,
        };
        let mut app = fixture();
        // A drawing pack lets artwork take the raw views the pack reads.
        app.sub_app_mut(RenderApp)
            .insert_resource(crate::actor::gpu::ArtworkRawViews(true));
        let mut frame = publication(1, &[55, 77], 1);
        Arc::make_mut(&mut frame.rig.instances)[1].material =
            render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG;
        draw_frame(&mut app, &frame);
        let world = app.sub_app(RenderApp).world();
        let gpu = world.resource::<ActorGpu>();
        assert_eq!(gpu.spans.len(), 2);
        assert!(gpu.pack_supports_span(&gpu.spans[0]));
        assert!(!gpu.pack_supports_span(&gpu.spans[1]));
        let mut replaced = Vec::new();
        shaderpack_actors(world)
            .unwrap()
            .for_each_instance(&mut |instance| replaced.push(instance.index));
        assert_eq!(replaced, [0]);
    }

    /// A body hidden from the camera, such as the local player in first person, still reaches
    /// the pack's shadow pass while staying out of its camera passes.
    #[test]
    fn mask_only_bodies_are_exported_as_shadow_only_instances() {
        use crate::actor_render::tests::allocation_fault_tests::{
            draw_frame, fixture, publication,
        };
        let mut app = fixture();
        app.sub_app_mut(RenderApp)
            .insert_resource(crate::actor::gpu::ArtworkRawViews(true));
        let mut frame = publication(1, &[55, 77], 1);
        let hidden = frame.rig.manifest[1].identity.runtime_id;
        frame.mask_only_runtime_ids = Arc::from([hidden]);
        frame.main_pass_instance_count = Some(1);
        draw_frame(&mut app, &frame);
        let world = app.sub_app(RenderApp).world();
        let mut exported = Vec::new();
        shaderpack_actors(world)
            .unwrap()
            .for_each_instance(&mut |instance| exported.push((instance.index, instance.camera)));
        assert_eq!(exported, [(0, true), (1, false)]);
    }

    #[test]
    fn equal_depth_overlays_keep_their_depth_producers_on_the_native_path() {
        use crate::actor_render::tests::allocation_fault_tests::{
            draw_frame, fixture, publication,
        };
        let mut app = fixture();
        app.sub_app_mut(RenderApp)
            .insert_resource(crate::actor::gpu::ArtworkRawViews(true));
        let mut frame = publication(1, &[55, 55, 77], 1);
        let manifest = Arc::make_mut(&mut frame.rig.manifest);
        manifest[1].identity.runtime_id = manifest[0].identity.runtime_id;
        manifest[1].identity.layer = render_model::ACTOR_LAYER_BODY + 1;
        let eye_state = assets::EntityRenderMaterialState {
            depth_equal: true,
            ..Default::default()
        };
        Arc::make_mut(&mut frame.rig.instances)[1].material =
            assets::EntityRenderMaterial::Default.word(Some(eye_state));
        draw_frame(&mut app, &frame);
        let world = app.sub_app(RenderApp).world();
        let gpu = world.resource::<ActorGpu>();
        assert!(gpu.artwork.page(0).unwrap().raw_texture().is_some());
        assert!(gpu.spans.len() >= 2);
        assert!(gpu.spans.iter().all(|span| !gpu.pack_supports_span(span)));
        let mut exported = Vec::new();
        shaderpack_actors(world)
            .unwrap()
            .for_each_instance(&mut |instance| exported.push(instance.index));
        assert!(
            exported.is_empty(),
            "a pack vertex transform must not split a native depth pair"
        );

        frame.rig.frame_generation += 1;
        Arc::make_mut(&mut frame.rig.instances)[1].material =
            assets::EntityRenderMaterial::Default as u32;
        draw_frame(&mut app, &frame);
        let world = app.sub_app(RenderApp).world();
        let gpu = world.resource::<ActorGpu>();
        assert!(gpu.spans.iter().all(|span| gpu.pack_supports_span(span)));
        let mut exported = Vec::new();
        shaderpack_actors(world)
            .unwrap()
            .for_each_instance(&mut |instance| exported.push(instance.index));
        assert_eq!(exported.len(), 3);
    }
}
