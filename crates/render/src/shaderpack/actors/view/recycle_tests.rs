use super::*;
use crate::shaderpack::actors::{RigBuffers, RigInstance, skin};
use crate::shaderpack::effects::textures::release_tests::app;
use crate::shaderpack::plugin::ShaderPackViews;
use bevy::render::{
    RenderApp,
    render_resource::Buffer,
    renderer::{RenderDevice, RenderQueue},
};

/// One composed actor with storage buffers sufficient for the no-op device.
struct Source<'a> {
    texture: &'a Texture,
    buffer: &'a Buffer,
    data: render_model::ActorGpuInstance,
    compose: LayerCompose,
}

impl RigSource for Source<'_> {
    /// Supplies the no-op skinning dispatch with valid storage bindings.
    fn buffers(&self) -> RigBuffers<'_> {
        RigBuffers {
            instances: self.buffer,
            vertices: self.buffer,
            spans: self.buffer,
            previous_bones: self.buffer,
            current_bones: self.buffer,
        }
    }

    /// Draws one triangle using the selected composition of the shared artwork.
    fn for_each_instance(&self, visit: &mut dyn FnMut(RigInstance<'_>)) {
        visit(RigInstance {
            index: 0,
            data: &self.data,
            vertex_count: 3,
            texture: LayerTexture {
                texture: self.texture,
                layer: 0,
                compose: self.compose,
            },
            runtime_id: None,
            camera: true,
            light: [15; 2],
            blend: None,
            held: None,
        });
    }

    /// The fixture has no enchantment glint.
    fn glint(&self, _: bool) -> Option<&Texture> {
        None
    }
}

/// Retained slots must rebind when their composition returns after its texture was reused.
#[test]
fn returning_composition_rebuilds_retained_bind_groups() {
    let mut app = app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().wgpu_device();
    let queue = world.resource::<RenderQueue>();
    let layout = |descriptor: bevy::render::render_resource::BindGroupLayoutDescriptor| {
        bevy::render::render_resource::BindGroupLayout::from(device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &descriptor.entries,
            },
        ))
    };
    let rig_layout = layout(skin::rig_layout());
    let block_entity_layout = layout(skin::block_entity_layout());
    let output_layout = layout(skin::output_layout());
    let compose_layout = layout(crate::shaderpack::actors::compose::layout());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
    });
    let pipeline = bevy::render::render_resource::ComputePipeline::from(
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        }),
    );
    let kernels = skin::SkinKernels {
        rig: &pipeline,
        block_entities: &pipeline,
        compose: &pipeline,
        rig_layout: &rig_layout,
        block_entity_layout: &block_entity_layout,
        output_layout: &output_layout,
        compose_layout: &compose_layout,
    };
    let texture = Texture::from(device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    }));
    let buffer = Buffer::from(device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    }));
    let a = Source {
        texture: &texture,
        buffer: &buffer,
        data: Default::default(),
        compose: LayerCompose::ColorMask { tint: 1 },
    };
    let b = Source {
        compose: LayerCompose::ColorMask { tint: 2 },
        ..a
    };
    let key_a = (texture.id(), 0, a.compose);
    let key_b = (texture.id(), 0, b.compose);
    let mut states = world.resource::<ShaderPackViews>().states.lock().unwrap();
    let state = states.values_mut().next().unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut prepare = |state: &mut crate::shaderpack::passes::ViewState, actors, hand| {
        crate::shaderpack::actors::prepare(
            device,
            queue,
            &mut encoder,
            state,
            &ActorSources {
                actors,
                hand,
                kernels: Some(kernels),
                ..Default::default()
            },
        );
        assert!(!state.actors.allocation_failed);
    };
    prepare(state, Some(&a as &dyn RigSource), None);
    let original = state.actors.composed[&key_a].clone();
    let groups = state.actors.slots[&DrawKind::Entities][0].groups.clone();
    assert!(
        groups
            .iter()
            .all(|parity| parity.iter().any(Option::is_some))
    );
    prepare(state, Some(&a as &dyn RigSource), None);
    assert_eq!(state.actors.slots[&DrawKind::Entities][0].groups, groups);

    // B uses hand slots, leaving A's entity slots inactive while reusing A's texture.
    prepare(state, None, Some(&b as &dyn RigSource));
    assert!(!state.actors.composed.contains_key(&key_a));
    assert_eq!(state.actors.composed[&key_b], original);
    assert_eq!(state.actors.slots[&DrawKind::Entities][0].groups, groups);

    // B remains visible, so returning A needs a different composed texture.
    prepare(
        state,
        Some(&a as &dyn RigSource),
        Some(&b as &dyn RigSource),
    );
    assert_ne!(state.actors.composed[&key_a], original);
    for (previous, current) in groups
        .iter()
        .zip(&state.actors.slots[&DrawKind::Entities][0].groups)
    {
        assert_ne!(
            previous, current,
            "returning A must bind its new artwork in both parities"
        );
    }
}
