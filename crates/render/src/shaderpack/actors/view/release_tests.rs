use super::*;
use crate::block_entity::BlockEntityGpu;
use crate::shaderpack::effects::Effect;
use crate::shaderpack::effects::textures::{
    ShaderpackEffectTextures,
    release_tests::{app, publish},
};
use crate::shaderpack::plugin::ShaderPackViews;
use bevy::render::{RenderApp, renderer::RenderDevice};

#[test]
fn block_entity_release_drops_actor_views_even_without_shared_effect_textures() {
    let mut app = app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let previous;
    {
        let atlas = world.resource::<BlockEntityGpu>().atlas_texture().unwrap();
        previous = atlas.id();
        let device = world.resource::<RenderDevice>().wgpu_device();
        for state in world
            .resource::<ShaderPackViews>()
            .states
            .lock()
            .unwrap()
            .values_mut()
        {
            let key = state.actors.view(LayerTexture {
                texture: atlas,
                layer: 0,
            });
            let index = state
                .pack
                .draws
                .iter()
                .position(|draw| draw.kind == DrawKind::Effect(Effect::DamagedBlock))
                .unwrap();
            state.actors.slots.insert(
                DrawKind::Block,
                vec![Slot {
                    uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("actor atlas release fixture"),
                        size: 16,
                        usage: wgpu::BufferUsages::UNIFORM,
                        mapped_at_creation: false,
                    }),
                    texture: Some(key),
                    groups: state.bindings.draws[index].groups.clone(),
                }],
            );
        }
    }
    // Actor draws can retain the atlas even after the effect textures are absent.
    world.remove_resource::<ShaderpackEffectTextures>();
    {
        let _fault = crate::gpu_allocation::inject(|label, _| label == "block-entity atlas");
        publish(world, 2, false);
    }
    for state in world
        .resource::<ShaderPackViews>()
        .states
        .lock()
        .unwrap()
        .values()
    {
        assert!(
            state
                .actors
                .views
                .keys()
                .all(|(texture, _)| *texture != previous)
        );
        let slot = &state.actors.slots[&DrawKind::Block][0];
        assert!(slot.texture.is_none());
        assert!(slot.groups.iter().flatten().all(Option::is_none));
        assert!(
            state.bindings.draws.iter().all(|binding| binding
                .groups
                .iter()
                .flatten()
                .all(Option::is_none))
        );
    }
}
