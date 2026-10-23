//! The textures effect draws bind as `gtexture`, as plain `Rgba8Unorm` so packs read the
//! stored (gamma) bytes as GL does: uploaded from the CPU copies Cinnabar's own renderers
//! keep, or for the atlases those renderers upload, an `Rgba8Unorm` view of theirs:
//!
//! | effect | texture | source |
//! |---|---|---|
//! | sun | `textures/environment/sun.png` | `AtmosphereTextureAssets` |
//! | moon | `textures/environment/moon_phases.png` | `AtmosphereTextureAssets` |
//! | clouds | 1×1 white (source RGBA is baked into vertices) | |
//! | weather | `textures/environment/weather.png` | `WeatherTextureAssets` |
//! | particles | the particle atlas and its dynamic tiles | `ParticleGpu` (a view) |
//! | cracks | the block-entity atlas (destroy stages) | `BlockEntityGpu` (a view) |
//! | stars, lightning, outline | 1×1 white | |
//!
//! `normals` binds a flat normal and `specular` zero, Iris's defaults without PBR maps.

use super::super::plugin::{LoadedShaderPack, ShaderPackViews};
use super::super::resources::ATLAS_FORMAT;
use super::super::targets::AtlasViews;
use super::Effect;
use crate::block_entity::BlockEntityGpu;
use crate::particle_render::ParticleGpu;
use crate::shaderpack::draws::DrawKind;
use assets::AtmosphereRole;
use bevy::prelude::*;
use bevy::render::render_resource::{Texture, TextureId};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use render_scene::{AtmosphereTextureAssets, WeatherTextureAssets};
use std::sync::Arc;
use wgpu::{TextureUsages, TextureView};

/// One uploaded texture (its view holds it) and the identity of what it holds.
struct Uploaded<K> {
    key: K,
    view: TextureView,
}

/// A gamma-byte view of a texture another renderer owns and keeps current.
struct Shared {
    texture: TextureId,
    view: TextureView,
}

/// Every effect texture of the shown pack, in the render world.
#[derive(Resource, Default)]
pub(crate) struct ShaderpackEffectTextures {
    /// Bumped whenever a texture view is replaced, so draw bind groups rebuild.
    pub generation: u64,
    constants: Option<[(wgpu::Texture, TextureView); 3]>,
    sun: Option<Uploaded<[u8; 32]>>,
    moon: Option<Uploaded<[u8; 32]>>,
    /// Keyed by the sheet's allocation.
    weather: Option<Uploaded<usize>>,
    particles: Option<Shared>,
    particle_side: u32,
    cracks: Option<Shared>,
}

const WHITE: usize = 0;
const FLAT_NORMAL: usize = 1;
const NO_SPECULAR: usize = 2;

/// Shared atlas owners that must release their views before a native atlas is replaced.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct SharedAtlasRelease<'w> {
    textures: Option<ResMut<'w, ShaderpackEffectTextures>>,
    views: Option<ResMut<'w, ShaderPackViews>>,
}

impl SharedAtlasRelease<'_> {
    /// Drops shared views and cached draw bindings of `texture` before its replacement allocates.
    pub(crate) fn release(&mut self, texture: Option<&Texture>) {
        let Some(texture) = texture else {
            return;
        };
        if let Some(textures) = self.textures.as_deref_mut() {
            let mut changed = false;
            if textures
                .particles
                .as_ref()
                .is_some_and(|shared| shared.texture == texture.id())
            {
                textures.particles = None;
                textures.particle_side = 0;
                changed = true;
            }
            if textures
                .cracks
                .as_ref()
                .is_some_and(|shared| shared.texture == texture.id())
            {
                textures.cracks = None;
                changed = true;
            }
            if changed {
                textures.generation += 1;
            }
        }
        if let Some(views) = self.views.as_deref_mut() {
            for state in views
                .states
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .values_mut()
            {
                state.actors.release_texture(texture.id());
                // Geometry bindings rebuild together when the texture generation changes.
                for binding in &mut state.bindings.draws {
                    binding.groups = Default::default();
                }
                state.atlas_generation = None;
            }
        }
    }
}

fn texture(device: &wgpu::Device, label: &str, size: [u32; 2]) -> (wgpu::Texture, TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ATLAS_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Writes `rgba` (tightly packed rows) at `origin`.
fn write(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
    rgba: &[u8],
) {
    if size[0] == 0 || size[1] == 0 || rgba.len() < (size[0] * size[1] * 4) as usize {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

/// Replaces `slot` with a fresh `size` texture holding `rgba` in its top `rows` when its key
/// changed; returns whether it did.
#[allow(
    clippy::too_many_arguments,
    reason = "one texture's identity, shape and pixels"
)]
fn upload<K: PartialEq>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    slot: &mut Option<Uploaded<K>>,
    key: K,
    label: &str,
    size: [u32; 2],
    rows: u32,
    rgba: &[u8],
) -> bool {
    if slot.as_ref().is_some_and(|s| s.key == key) {
        return false;
    }
    let (texture, view) = texture(device, label, size);
    write(queue, &texture, [0, 0], [size[0], rows.min(size[1])], rgba);
    *slot = Some(Uploaded { key, view });
    true
}

/// Points `slot` at an `Rgba8Unorm` view of `texture` when it is not already one; returns
/// whether it changed. Without a texture the slot keeps what it had.
fn share(slot: &mut Option<Shared>, label: &str, texture: Option<&Texture>) -> bool {
    let Some(texture) = texture else {
        return false;
    };
    if slot.as_ref().is_some_and(|s| s.texture == texture.id()) {
        return false;
    }
    let view = wgpu::Texture::create_view(
        texture,
        &wgpu::TextureViewDescriptor {
            label: Some(label),
            format: Some(ATLAS_FORMAT),
            ..Default::default()
        },
    );
    *slot = Some(Shared {
        texture: texture.id(),
        view,
    });
    true
}

impl ShaderpackEffectTextures {
    fn view<'a>(&'a self, view: Option<&'a TextureView>) -> Option<&'a TextureView> {
        view.or_else(|| self.constants.as_ref().map(|c| &c[WHITE].1))
    }

    /// The views `effect`'s draw binds as `gtexture`, `normals` and `specular`; `None` until
    /// the textures exist.
    pub(crate) fn views(&self, effect: Effect) -> Option<AtlasViews<'_>> {
        let constants = self.constants.as_ref()?;
        let color = match effect {
            Effect::Sun => self.view(self.sun.as_ref().map(|s| &s.view)),
            Effect::Moon => self.view(self.moon.as_ref().map(|s| &s.view)),
            Effect::Clouds => Some(&constants[WHITE].1),
            Effect::Weather => self.weather.as_ref().map(|s| &s.view),
            Effect::Particles | Effect::ParticlesTranslucent | Effect::ParticlesAdditive => {
                self.view(self.particles.as_ref().map(|s| &s.view))
            }
            Effect::DamagedBlock => self.view(self.cracks.as_ref().map(|s| &s.view)),
            Effect::Stars | Effect::Lightning | Effect::Outline => Some(&constants[WHITE].1),
        }?;
        Some(AtlasViews {
            color,
            normals: &constants[FLAT_NORMAL].1,
            specular: &constants[NO_SPECULAR].1,
        })
    }

    /// The source atlas size for damaged-block draws, in texels.
    pub(crate) fn crack_atlas_size(&self) -> [i32; 2] {
        self.cracks.as_ref().map_or([0; 2], |source| {
            let texture = source.view.texture();
            [texture.width() as i32, texture.height() as i32]
        })
    }

    /// The particle atlas's side, Iris's `atlasSize` for particle draws.
    pub(crate) fn particle_atlas_size(&self) -> [i32; 2] {
        let side = if self.particles.is_some() {
            self.particle_side as i32
        } else {
            0
        };
        [side; 2]
    }
}

/// The render-world sources of the effect textures.
type Sources<'w> = (
    Option<Res<'w, AtmosphereTextureAssets>>,
    Option<Res<'w, WeatherTextureAssets>>,
    Option<Res<'w, ParticleGpu>>,
    Option<Res<'w, BlockEntityGpu>>,
);

/// Keeps the effect textures equal to their sources while the shown pack draws effects;
/// releases them otherwise.
pub(crate) fn prepare_effect_textures(
    pack: Option<Res<LoadedShaderPack>>,
    mut textures: ResMut<ShaderpackEffectTextures>,
    (atmosphere, weather, particles, blocks): Sources,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let drawn = pack.as_ref().and_then(|p| p.0.as_ref()).map_or(0, |pack| {
        pack.draws
            .iter()
            .filter(|d| matches!(d.kind, DrawKind::Effect(_)))
            .count()
    });
    if drawn == 0 {
        if textures.constants.is_some() {
            let generation = textures.generation + 1;
            *textures = ShaderpackEffectTextures {
                generation,
                ..Default::default()
            };
        }
        return;
    }
    let device = device.wgpu_device();
    let queue: &wgpu::Queue = &queue;
    let mut changed = false;
    if textures.constants.is_none() {
        let constants = [
            ("shader pack effect white", [255, 255, 255, 255]),
            (
                "shader pack effect normals",
                crate::shaderpack::atlas::NEUTRAL_NORMAL,
            ),
            (
                "shader pack effect specular",
                crate::shaderpack::atlas::NEUTRAL_SPECULAR,
            ),
        ]
        .map(|(label, texel)| {
            let (texture, view) = texture(device, label, [1, 1]);
            write(queue, &texture, [0, 0], [1, 1], &texel);
            (texture, view)
        });
        textures.constants = Some(constants);
        changed = true;
    }
    if let Some(runtime) = atmosphere.as_ref().and_then(|a| a.runtime()) {
        for (role, label) in [
            (AtmosphereRole::Sun, "shader pack sun"),
            (AtmosphereRole::MoonPhases, "shader pack moon phases"),
        ] {
            let Some(image) = runtime.texture(role) else {
                continue;
            };
            let slot = match role {
                AtmosphereRole::Sun => &mut textures.sun,
                AtmosphereRole::MoonPhases => &mut textures.moon,
                AtmosphereRole::Clouds => continue,
            };
            changed |= upload(
                device,
                queue,
                slot,
                image.pixels_sha256,
                label,
                [image.width, image.height],
                image.height,
                &image.rgba8,
            );
        }
    }
    if let Some(sheets) = weather.as_ref().and_then(|w| w.textures()) {
        let image = &sheets.weather;
        changed |= upload(
            device,
            queue,
            &mut textures.weather,
            Arc::as_ptr(sheets) as usize,
            "shader pack weather",
            [image.width, image.height],
            image.height,
            &image.rgba8,
        );
    } else if textures.weather.take().is_some() {
        changed = true;
    }
    let particle_atlas = particles.as_ref().and_then(|gpu| gpu.atlas_texture());
    if share(
        &mut textures.particles,
        "shader pack particle atlas view",
        particle_atlas,
    ) {
        textures.particle_side = particle_atlas.map_or(0, |atlas| atlas.width());
        changed = true;
    }
    changed |= share(
        &mut textures.cracks,
        "shader pack block-entity atlas view",
        blocks.as_ref().and_then(|gpu| gpu.atlas_texture()),
    );
    if changed {
        textures.generation += 1;
    }
}

#[cfg(test)]
#[path = "textures/release_tests.rs"]
pub(crate) mod release_tests;

#[cfg(test)]
mod gpu_tests {
    use super::*;

    #[test]
    fn cloud_colour_is_baked_once_and_missing_weather_keeps_its_native_fallback() {
        let Some(gpu) = crate::shaderpack::tests::gpu() else {
            eprintln!("missing fixture: native GPU adapter for the effect texture views");
            return;
        };
        let constants = [
            [255; 4],
            crate::shaderpack::atlas::NEUTRAL_NORMAL,
            crate::shaderpack::atlas::NEUTRAL_SPECULAR,
        ]
        .map(|texel| {
            let (texture, view) = texture(&gpu.device, "effect fixture", [1, 1]);
            write(&gpu.queue, &texture, [0, 0], [1, 1], &texel);
            (texture, view)
        });
        let textures = ShaderpackEffectTextures {
            constants: Some(constants),
            ..Default::default()
        };
        assert!(std::ptr::eq(
            textures.views(Effect::Clouds).unwrap().color,
            &textures.constants.as_ref().unwrap()[WHITE].1
        ));
        assert!(textures.views(Effect::Weather).is_none());
    }
}
