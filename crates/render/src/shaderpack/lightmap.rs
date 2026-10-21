//! The `lightmap` texture geometry programs sample. Iris binds Minecraft's own light texture,
//! which the game rebuilds whenever the sky darkens, a flash or effect changes or the
//! brightness setting moves; here that is the world lightmap Cinnabar's terrain lights with,
//! so packs see the same sky and block light, night vision and darkness.

use bevy::prelude::{Local, Res};
use bevy::render::renderer::RenderQueue;
use render_model::LightmapInputs;
use render_scene::{WorldFullbright, WorldLighting};

use super::plugin::LoadedShaderPack;
use crate::lighting::{light_table, world_lightmap_inputs};

/// Side of the light texture: one texel per block (U) and sky (V) light level.
pub(crate) const LIGHTMAP_SIDE: u32 = 16;

/// RGBA8 texels of the light texture for `inputs` (`None` under full brightness), row by sky
/// level, as Minecraft's `LightTexture` stores them: texel `(block, sky)` holds the light
/// colour of that pair.
pub(crate) fn lightmap_texels(inputs: Option<LightmapInputs>) -> [u8; 1024] {
    let table = light_table(inputs);
    let mut texels = [0; 1024];
    for (texel, colour) in texels.as_chunks_mut::<4>().0.iter_mut().zip(table) {
        for (byte, channel) in texel.iter_mut().zip(colour) {
            *byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    texels
}

/// The light texture a pack's texture `index` was last written with, so an unchanged frame
/// uploads nothing.
#[derive(Default)]
pub(crate) struct WrittenLightmap(Option<(usize, Option<LightmapInputs>)>);

/// Rewrites the shown pack's `lightmap` when the world lightmap or the pack changes.
pub(crate) fn update_lightmap(
    pack: Option<Res<LoadedShaderPack>>,
    lighting: Option<Res<WorldLighting>>,
    fullbright: Option<Res<WorldFullbright>>,
    queue: Res<RenderQueue>,
    mut written: Local<WrittenLightmap>,
) {
    let Some(pack) = pack.and_then(|pack| pack.0.clone()) else {
        written.0 = None;
        return;
    };
    let (Some(index), Some(lighting)) = (pack.lightmap, lighting) else {
        return;
    };
    let fullbright = fullbright.as_deref().copied().unwrap_or_default();
    let inputs = world_lightmap_inputs(&lighting, &fullbright);
    let key = (std::sync::Arc::as_ptr(&pack) as usize, inputs);
    if written.0 == Some(key) {
        return;
    }
    written.0 = Some(key);
    queue.write_texture(
        pack.constants.textures[index].texture.as_image_copy(),
        &lightmap_texels(inputs),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LIGHTMAP_SIDE * 4),
            rows_per_image: Some(LIGHTMAP_SIDE),
        },
        wgpu::Extent3d {
            width: LIGHTMAP_SIDE,
            height: LIGHTMAP_SIDE,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texel(texels: &[u8; 1024], block: usize, sky: usize) -> [u8; 4] {
        let at = (sky * LIGHTMAP_SIDE as usize + block) * 4;
        texels[at..at + 4].try_into().unwrap()
    }

    fn at_time(ticks: f64) -> LightmapInputs {
        let angle = render_model::celestial_angle(ticks);
        LightmapInputs {
            sky_darken: render_model::lightmap_sky_darken(angle, 0.0, 0.0),
            brightness: 0.5,
            ambient_adjustment: true,
            ..LightmapInputs::default()
        }
    }

    #[test]
    fn texels_are_the_world_light_table_with_block_along_u_and_sky_along_v() {
        let noon = at_time(6_000.0);
        let texels = lightmap_texels(Some(noon));
        let table = noon.build();
        for (block, sky) in [(0, 0), (15, 0), (0, 15), (7, 12)] {
            let expected = table[sky << 4 | block].map(|c| (c * 255.0).round() as u8);
            assert_eq!(texel(&texels, block, sky), expected, "({block}, {sky})");
        }
        // Block light is warm: red leads blue at a dim torch level.
        let torch = texel(&texels, 8, 0);
        assert!(torch[0] > torch[2], "{torch:?}");
    }

    #[test]
    fn sky_light_follows_the_time_of_day_and_night_vision() {
        let noon = lightmap_texels(Some(at_time(6_000.0)));
        let midnight = at_time(18_000.0);
        let night = lightmap_texels(Some(midnight));
        assert!(texel(&night, 0, 15)[0] < texel(&noon, 0, 15)[0] / 2);
        // Torchlight does not depend on the sky.
        assert_eq!(texel(&night, 15, 0)[0], texel(&noon, 15, 0)[0]);
        let seeing = lightmap_texels(Some(LightmapInputs {
            night_vision: 1.0,
            ..midnight
        }));
        assert!(texel(&seeing, 0, 15)[2] > texel(&night, 0, 15)[2]);
        assert_eq!(lightmap_texels(None), [255; 1024], "full brightness");
    }
}
