//! Bounded numbered sky textures from the admitted resource stack.

use assets::{PackSkybox, SkyboxDimension, SkyboxFace};
use image::ImageFormat;
use resource_pack::LayeredPackView;

const IMAGE_EXTENSIONS: [(&str, Option<ImageFormat>); 6] = [
    ("tga", Some(ImageFormat::Tga)),
    ("png", Some(ImageFormat::Png)),
    ("jpg", Some(ImageFormat::Jpeg)),
    ("jpeg", Some(ImageFormat::Jpeg)),
    ("hdr", None),
    ("ktx", None),
];

/// Resolves each face independently so upper packs can replace individual images.
pub fn prepare_pack_skyboxes(view: &LayeredPackView) -> [PackSkybox; SkyboxDimension::ALL.len()] {
    SkyboxDimension::ALL.map(|dimension| PackSkybox {
        faces: std::array::from_fn(|face| {
            let path = format!("{}{face}", dimension.texture_prefix());
            let texture = decode_face(view, &path)?;
            Some(SkyboxFace {
                width: texture.width,
                height: texture.height,
                rgba8: texture.rgba8,
            })
        }),
    })
}

/// Chooses the highest pack, then its first image extension; an invalid winner stays absent.
fn decode_face(view: &LayeredPackView, path: &str) -> Option<crate::pack_textures::DecodedTexture> {
    let limit = resource_pack::MAX_PACK_TEXTURE_BYTES;
    let candidates =
        IMAGE_EXTENSIONS.map(|(extension, format)| (format!("{path}.{extension}"), format));
    if let Some(dependencies) = view.dependencies() {
        dependencies.extend(candidates.iter().map(|(path, _)| {
            resource_pack::PackDependency::File {
                path: path.clone(),
                limit,
            }
        }));
    }
    for layer in view.layers().rev() {
        for (path, format) in &candidates {
            if let Some(bytes) = layer.read_file_with_limit(path, limit).ok()? {
                // The bounded raster decoder does not admit HDR or compressed KTX images.
                return crate::pack_textures::decode_image(&bytes, (*format)?);
            }
        }
    }
    None
}
