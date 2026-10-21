//! Cross-crate observations of the production actor preparation consumer.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::{ActorGpuInstance, ActorRenderIdentity};

/// One instance covered by an actual prepared actor draw span.
#[derive(Debug)]
pub struct PreparedEquipmentDraw {
    pub identity: ActorRenderIdentity,
    pub rig: render_model::EntityRigId,
    pub instance: ActorGpuInstance,
    pub page: render_model::ActorArtworkPageId,
    pub vertex_count: u32,
}

/// Retains the real renderer's upload and residency state across fixture frames.
pub struct EquipmentPreparation {
    world: World,
}

impl EquipmentPreparation {
    /// Creates the mandatory device-independent preparation consumer.
    pub fn noop() -> Self {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        Self::new(device.into(), RenderQueue::new(queue))
    }

    /// Installs the resources read by production actor preparation on the given device.
    fn new(device: RenderDevice, queue: RenderQueue) -> Self {
        let gpu = create_actor_gpu(&device, &queue).expect("fixture actor allocation");
        let mut world = World::new();
        world.insert_resource(device);
        world.insert_resource(queue);
        world.insert_resource(gpu);
        world.init_resource::<ActorRuntimeWitness>();
        world.init_resource::<ActorPresentationGate>();
        world.init_resource::<ActorDrawTracker>();
        Self { world }
    }

    /// Uploads a published frame and reports only instances covered by prepared draw spans.
    pub fn prepare(&mut self, frame: &ActorRenderFrame) -> Vec<PreparedEquipmentDraw> {
        self.world.insert_resource(frame.clone());
        self.world.run_system_once(prepare_actor_resources).unwrap();
        let gpu = self.world.resource::<ActorGpu>();
        assert_eq!(gpu.instance_count as usize, gpu.instances.len());
        gpu.spans
            .iter()
            .flat_map(|span| {
                (span.first..span.first + span.count).map(|index| PreparedEquipmentDraw {
                    identity: gpu.manifest[index as usize].identity,
                    rig: gpu.manifest[index as usize].rig,
                    instance: gpu.instances[index as usize],
                    page: span.page,
                    vertex_count: span.vertex_count,
                })
            })
            .collect()
    }
}

/// Samples the native prepared equipment pages, using their prepared layer selections.
/// Only a missing native adapter skips this supplemental pixel check.
pub fn equipment_pixels(frame: &ActorRenderFrame) -> Option<Vec<(ActorRenderIdentity, [u8; 4])>> {
    let gpu = crate::gpu_snapshot::Gpu::for_fixture("remote equipment artwork pixels")?;
    let mut preparation = EquipmentPreparation::new(
        gpu.device.clone().into(),
        RenderQueue::new(gpu.queue.clone()),
    );
    let draws = preparation.prepare(frame);
    assert!(!draws.is_empty(), "native preparation admits the fixture");
    let prepared = preparation.world.resource::<ActorGpu>();
    Some(
        draws
            .iter()
            .filter(|draw| draw.page != 0)
            .map(|draw| {
                let page = prepared.artwork.page(usize::from(draw.page - 1)).unwrap();
                let layer = gpu.words(
                    &[draw.instance.texture_layer, 0, 0, 0],
                    wgpu::BufferUsages::UNIFORM,
                );
                let pixels = gpu.render(
                    r#"
@group(0) @binding(0) var artwork: texture_2d_array<f32>;
@group(0) @binding(1) var<uniform> layer: vec4<u32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - 1.0, 0.5, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> {
    return textureLoad(artwork, vec2<i32>(0), i32(layer.x), 0);
}
"#,
                    "vertex",
                    &[crate::gpu_snapshot::Draw {
                        fragment: "fragment",
                        vertices: 0..3,
                        bindings: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&page.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: layer.as_entire_binding(),
                            },
                        ],
                        blend: None,
                        write_depth: false,
                    }],
                );
                (draw.identity, pixels[..4].try_into().unwrap())
            })
            .collect(),
    )
}
