//! Which texture or image each sampler and image name of a program binds.

use super::*;
use crate::shaderpack::pipelines::{format_kind, sample_kind, storage_format};

/// Resolves every texture and image binding of `reflection`.
#[allow(
    clippy::too_many_arguments,
    reason = "the pack-wide plan pieces a binding draws on"
)]
pub(crate) fn resolve_bindings(
    constants: &mut ConstantBuilder<'_>,
    group: ProgramGroup,
    reflection: &ProgramReflection,
    buffer_formats: &[Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    storage_buffers: u32,
    depth_format: TextureFormat,
    images: &[CustomImage],
    image_formats: &[TextureFormat],
) -> Result<ProgramBindings, String> {
    let stage = group.texture_stage();
    let geometry = matches!(group, ProgramGroup::Gbuffers | ProgramGroup::Shadow);
    let textures = reflection
        .textures
        .iter()
        .map(|t| {
            let wanted = sample_kind(t.sample_type);
            let fits = |format: TextureFormat| wanted.is_none_or(|k| k == format_kind(format));
            let source =
                if let Some((index, format)) = constants.custom(stage, &t.name, t.dimension) {
                    Some(TextureSource::Constant { index, format })
                } else if let Some(layer) = atlas_layer(&t.name).filter(|_| geometry) {
                    Some(TextureSource::Atlas(layer))
                } else if t.name == "noisetex" {
                    let (index, format) = constants.noise()?;
                    Some(TextureSource::Constant { index, format })
                } else if let Some(index) = depth_index(&t.name) {
                    Some(if t.sample_type == SampleType::Depth {
                        TextureSource::DepthCompare(index)
                    } else {
                        TextureSource::Depth {
                            index,
                            format: depth_format,
                        }
                    })
                } else if let Some(index) = match t.name.as_str() {
                    "shadowtex0" | "shadow" | "watershadow" => Some(0),
                    "shadowtex1" => Some(1),
                    _ => None,
                } {
                    let (index, format) = constants.shadow(index, t.sample_type)?;
                    Some(TextureSource::Constant { index, format })
                } else if let Some(BufferId::ShadowColor(i)) = BufferId::parse(&t.name) {
                    let (index, format) = constants.shadow_color(i)?;
                    Some(TextureSource::Constant { index, format })
                } else if let Some(index) = color_buffer_of(&t.name)
                    && let Some(format) = buffer_formats[index as usize]
                {
                    Some(TextureSource::Buffer { index, format })
                } else {
                    images
                        .iter()
                        .position(|i| i.sampler == t.name)
                        .map(|index| TextureSource::Image {
                            index,
                            format: image_formats[index],
                        })
                };
            let dimension_fits = |s: &TextureSource| match *s {
                TextureSource::Constant { index, .. } => constants.has_shape(index, t.dimension),
                TextureSource::Image { index, .. } => {
                    let image = match images[index].size {
                        ImageSize::Absolute { height: None, .. } => TextureDimension::D1,
                        ImageSize::Absolute { depth: Some(_), .. } => TextureDimension::D3,
                        _ => TextureDimension::D2,
                    };
                    storage_shape(image) == storage_shape(t.dimension)
                }
                _ => t.dimension == TextureDimension::D2,
            };
            Ok(match source {
                Some(s)
                    if fits(s.format())
                        && dimension_fits(&s)
                        && (t.sample_type == SampleType::Depth)
                            == s.format().is_depth_stencil_format() =>
                {
                    s
                }
                other => {
                    bevy::log::debug!(
                        target: "cinnabar::shaderpack",
                        texture = %t.name, resolved = ?other,
                        "binding a placeholder"
                    );
                    constants.placeholder_for(t.sample_type, t.dimension)?
                }
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let images = reflection
        .storage_images
        .iter()
        .map(|image| {
            let format = storage_format(image.format);
            if let Some(b) = color_buffer_of(&image.name)
                && storage_buffers & (1 << b) != 0
                && buffer_formats[b as usize] == format
            {
                return Ok(ImageSource::Buffer(b));
            }
            if let Some(index) = images.iter().position(|i| i.name == image.name)
                && Some(image_formats[index]) == format
            {
                return Ok(ImageSource::Image(index));
            }
            let format = format.unwrap_or(TextureFormat::Rgba8Unorm);
            Ok(ImageSource::Constant(
                constants.placeholder(format, image.dimension, true)?.0,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ProgramBindings { textures, images })
}
