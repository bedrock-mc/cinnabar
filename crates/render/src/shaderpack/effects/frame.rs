//! One view's effect geometry per frame: what it is built from, and the vertex and index
//! buffers it is uploaded into.

use super::celestial::Celestial;
use super::clouds::{CloudCache, CloudFrame, cloud_quads};
use super::lines::{cracks, lightning, outline};
use super::particles::particle_quads;
use super::vertices::Mesh;
use super::weather::{RAIN_KIND, rain_columns, weather_quads};
use super::{CloudSetting, Effect, EffectSettings};
use crate::shaderpack::draws::DrawKind;
use crate::shaderpack::loader::PackGpu;
use assets::AtmosphereRole;
use bevy::math::{Mat4, Vec3};
use render_model::{BlockEntityFrame, BlockSelectionFrame};
use render_scene::shaderpack::{ShaderPackEffects, ShaderPackFrameInputs};
use render_scene::{
    AtmosphereFrame, AtmosphereTextureAssets, AtmosphereViewInputs, LightningScene,
    ParticleGpuFrame, PrecipitationScene,
};
use std::ops::Range;

/// The camera the effects are placed around.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectCamera {
    /// World position; effect vertices are relative to it.
    pub position: Vec3,
    /// `gbufferModelView`: the camera rotation.
    pub model_view: Mat4,
    /// The projection's x and y scales.
    pub focal: [f32; 2],
    /// Viewport in pixels.
    pub screen: [u32; 2],
}

/// The render-world data a view's effects are built from this frame; each source is absent
/// when its renderer is not installed.
pub(crate) struct EffectInputs<'a> {
    /// The replacement decision already used by this frame's native queues.
    pub coverage: ShaderPackEffects,
    pub camera: EffectCamera,
    pub frame: ShaderPackFrameInputs,
    pub atmosphere: Option<&'a AtmosphereFrame>,
    pub atmosphere_view: Option<AtmosphereViewInputs>,
    pub atmosphere_textures: Option<&'a AtmosphereTextureAssets>,
    /// The player's cloud preference, used under `clouds=default`.
    pub clouds_visible: bool,
    pub particles: Option<&'a ParticleGpuFrame>,
    pub precipitation: Option<&'a PrecipitationScene>,
    pub lightning: Option<&'a LightningScene>,
    pub selection: Option<&'a BlockSelectionFrame>,
    pub block_entities: Option<&'a BlockEntityFrame>,
    /// Seconds the renderer has run, the game clock Java's rain scrolls with unless the
    /// pack's `frameTimeCounter` is pinned.
    pub seconds: f32,
}

/// One view's effect buffers and the index range each effect draws this frame.
#[derive(Default)]
pub(crate) struct EffectBuffers {
    pub vertices: Option<wgpu::Buffer>,
    pub indices: Option<wgpu::Buffer>,
    ranges: Vec<(Effect, Range<u32>)>,
    mesh: Mesh,
    clouds: CloudCache,
}

impl EffectBuffers {
    /// The indices `effect` draws this frame, when it has any.
    pub(crate) fn range(&self, effect: Effect) -> Option<Range<u32>> {
        self.ranges
            .iter()
            .find(|(e, range)| *e == effect && !range.is_empty())
            .map(|(_, range)| range.clone())
    }
}

/// Appends `effect`'s geometry for this frame.
fn build(
    mesh: &mut Mesh,
    clouds: &mut CloudCache,
    effect: Effect,
    inputs: &EffectInputs<'_>,
    settings: &EffectSettings,
    sun_path_rotation: f32,
) {
    let camera = inputs.camera.position;
    let overworld = inputs.frame.dimension == 0;
    let celestial = Celestial {
        angle: inputs.frame.celestial_angle,
        sun_path_rotation,
        moon_phase: inputs.frame.moon_phase,
        rain: inputs.frame.rain_strength.clamp(0.0, 1.0),
    };
    match effect {
        Effect::Sun if overworld => celestial.sun(mesh),
        Effect::Moon if overworld => celestial.moon(mesh),
        Effect::Stars if overworld => celestial.stars(mesh),
        Effect::Particles => {
            if let Some(frame) = inputs.particles {
                particle_quads(mesh, &frame.opaque, camera, frame.atlas_side);
            }
        }
        Effect::ParticlesTranslucent => {
            if let Some(frame) = inputs.particles {
                particle_quads(mesh, &frame.blend, camera, frame.atlas_side);
            }
        }
        Effect::ParticlesAdditive => {
            if let Some(frame) = inputs.particles {
                particle_quads(mesh, &frame.add, camera, frame.atlas_side);
            }
        }
        Effect::Weather => {
            if let Some(scene) = inputs.precipitation {
                if let Some(rain) = scene
                    .layers
                    .iter()
                    .find(|layer| layer.kind == RAIN_KIND && layer.particle_count > 0)
                {
                    let seconds = inputs.frame.frame_time_counter.unwrap_or(inputs.seconds);
                    rain_columns(
                        mesh,
                        &scene.occlusion,
                        rain.uv_rect,
                        camera,
                        inputs.frame.rain_strength,
                        seconds * render_model::PRECIPITATION_TICKS_PER_SECOND as f32,
                    );
                }
                weather_quads(
                    mesh,
                    &scene.layers,
                    scene.forward_offset,
                    &scene.occlusion,
                    camera,
                );
            }
        }
        Effect::Lightning => {
            if let Some(scene) = inputs.lightning {
                lightning(mesh, &scene.records, camera);
            }
        }
        Effect::Outline => {
            if let Some(selection) = inputs.selection {
                outline(
                    mesh,
                    &inputs.camera,
                    &selection.outline,
                    &selection.highlight,
                );
            }
        }
        Effect::DamagedBlock => {
            if let Some(frame) = inputs.block_entities {
                cracks(mesh, &frame.crack, camera);
            }
        }
        Effect::Clouds => {
            let visible = match settings.clouds {
                CloudSetting::Off => false,
                CloudSetting::Fast | CloudSetting::Fancy => true,
                CloudSetting::Default => inputs.clouds_visible,
            };
            let (Some(atmosphere), Some(textures)) =
                (inputs.atmosphere, inputs.atmosphere_textures)
            else {
                return;
            };
            let texture = textures
                .runtime()
                .and_then(|runtime| runtime.texture(AtmosphereRole::Clouds));
            let (true, true, Some(texture)) =
                (visible, atmosphere.sky_kind().has_clouds(), texture)
            else {
                return;
            };
            let colour = inputs.atmosphere_view.map_or([1.0, 1.0, 1.0, 0.8], |view| {
                atmosphere.cloud_colour_for_view(view)
            });
            let frame = CloudFrame {
                texture,
                identity: textures.identity(),
                scroll: atmosphere.cloud_scroll_blocks(),
                colour,
                fast: settings.clouds == CloudSetting::Fast,
            };
            cloud_quads(mesh, clouds, &frame, camera.as_dvec3());
        }
        Effect::Sun | Effect::Moon | Effect::Stars => {}
    }
}

/// Builds every effect the pack draws for this frame and uploads them into `buffers`.
pub(crate) fn prepare_effects(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &PackGpu,
    buffers: &mut EffectBuffers,
    inputs: &EffectInputs<'_>,
) {
    let EffectBuffers {
        ranges,
        mesh,
        clouds,
        ..
    } = buffers;
    mesh.clear();
    ranges.clear();
    for draw in &pack.draws {
        let DrawKind::Effect(effect) = draw.kind else {
            continue;
        };
        if !effect.covered_by(inputs.coverage) {
            continue;
        }
        let start = mesh.index_count();
        build(
            mesh,
            clouds,
            effect,
            inputs,
            &pack.effects,
            pack.settings.sun_path_rotation,
        );
        ranges.push((effect, start..mesh.index_count()));
    }
    if mesh.indices.is_empty() {
        return;
    }
    let vertex_bytes: &[u8] = bytemuck::cast_slice(&mesh.words);
    let index_bytes: &[u8] = bytemuck::cast_slice(&mesh.indices);
    let fit = |buffer: &mut Option<wgpu::Buffer>, bytes: &[u8], usage, label| {
        if buffer
            .as_ref()
            .is_none_or(|b| b.size() < bytes.len() as u64)
        {
            *buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (bytes.len() as u64).next_power_of_two().max(4096),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(buffer) = buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
    };
    fit(
        &mut buffers.vertices,
        vertex_bytes,
        wgpu::BufferUsages::VERTEX,
        "shader pack effect vertices",
    );
    fit(
        &mut buffers.indices,
        index_bytes,
        wgpu::BufferUsages::INDEX,
        "shader pack effect indices",
    );
}
