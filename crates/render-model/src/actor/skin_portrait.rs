//! Static front-face portraits from validated skin geometry and native texels.

use assets::{EntityGeometryUv, SkinGeometry};
use render_api::{CLASSIC_SKIN_SIDE, SkinAnimationKind, StandardSkin};

use crate::UiTexturePage;

#[derive(Clone, Copy)]
struct Face {
    origin: [f32; 2],
    across: [f32; 2],
    down: [f32; 2],
    area: f32,
}

/// Extracts a static face with its hat, preserving HD detail. Unsupported geometry,
/// invalid UVs and invisible faces return `None` so the caller can display Steve.
pub fn extract(skin: &StandardSkin) -> Option<UiTexturePage> {
    let side = skin.width as usize;
    if !side.is_power_of_two()
        || !(CLASSIC_SKIN_SIDE..=render_api::MAX_STANDARD_SKIN_SIDE as usize).contains(&side)
        || (skin.height != skin.width && skin.height.checked_mul(2) != Some(skin.width))
        || skin.rgba8.len() != side * skin.height as usize * 4
    {
        return None;
    }
    let mut pixels: &[u8] = &skin.rgba8;
    let mut dimensions = [skin.width, skin.height];
    let mut geometry = None;
    if let Some(source) = &skin.geometry {
        geometry =
            assets::parse_skin_geometry(&source.resource_patch, &source.geometry_data).ok()?;
        if let Some(animation) = source
            .animations
            .iter()
            .find(|a| a.kind == SkinAnimationKind::Face)
            && let Some(animated) = assets::parse_skin_geometry_layer(
                &source.resource_patch,
                &source.geometry_data,
                "animated_face",
            )
            .ok()?
        {
            if animation.frames == 0
                || !animation.height.is_multiple_of(animation.frames)
                || animation.width == 0
                || animation.height == 0
                || (animation.width as usize)
                    .checked_mul(animation.height as usize)
                    .and_then(|n| n.checked_mul(4))
                    != Some(animation.rgba8.len())
            {
                return None;
            }
            dimensions = [animation.width, animation.height / animation.frames];
            pixels = animation
                .rgba8
                .get(..dimensions[0] as usize * dimensions[1] as usize * 4)?;
            geometry = Some(animated);
        }
        // A named custom model without usable geometry must not sample stock UVs.
        if geometry.is_none() && !source.geometry_data.trim().is_empty() {
            return None;
        }
    }
    let (texture, base, hat) = if let Some(model) = &geometry {
        let mut texture = [model.texture_width as f32, model.texture_height as f32];
        if skin.height * 2 == skin.width
            && texture[0] == texture[1]
            && matches!(
                &*model.identifier,
                "geometry.humanoid" | "geometry.humanoid.custom" | "geometry.humanoid.customslim"
            )
        {
            texture[1] /= 2.0;
        }
        (
            texture,
            model_face(model, "head")?,
            model_face(model, "hat"),
        )
    } else {
        let face = |x| Face {
            origin: [x, 8.0],
            across: [8.0, 0.0],
            down: [0.0, 8.0],
            area: 64.0,
        };
        (
            [
                CLASSIC_SKIN_SIDE as f32,
                CLASSIC_SKIN_SIDE as f32 * skin.height as f32 / skin.width as f32,
            ],
            face(8.0),
            Some(face(40.0)),
        )
    };
    let resolution = [Some(base), hat]
        .into_iter()
        .flatten()
        .fold(8.0_f32, |size, face| {
            let extent = |uv: [f32; 2]| {
                (uv[0] * dimensions[0] as f32 / texture[0])
                    .hypot(uv[1] * dimensions[1] as f32 / texture[1])
            };
            size.max(extent(face.across)).max(extent(face.down))
        });
    let size = (resolution
        .clamp(8.0, (render_api::MAX_STANDARD_SKIN_SIDE / 8) as f32)
        .ceil() as u32)
        .next_power_of_two();
    let mut rgba = sample(pixels, dimensions, texture, base, size)?;
    if let Some(hat) = hat {
        let overlay = sample(pixels, dimensions, texture, hat, size)?;
        for (base, hat) in rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(overlay.as_chunks::<4>().0)
        {
            let a = u32::from(hat[3]);
            let remaining = u32::from(base[3]) * (255 - a);
            let alpha = a * 255 + remaining;
            for channel in 0..3 {
                let numerator = u32::from(hat[channel]) * a * 255
                    + u32::from(base[channel]) * remaining
                    + alpha / 2;
                if let Some(colour) = numerator.checked_div(alpha) {
                    base[channel] = colour as u8;
                }
            }
            base[3] = ((alpha + 127) / 255) as u8;
        }
    }
    rgba.as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[3] != 0)
        .then_some(())?;
    UiTexturePage::owned([size; 2], rgba.into()).ok()
}

/// Selects the largest unrotated front surface of a named head layer.
fn model_face(model: &SkinGeometry, name: &str) -> Option<Face> {
    let (index, bone) = model
        .bones
        .iter()
        .enumerate()
        .find(|(_, b)| b.name.eq_ignore_ascii_case(name))?;
    if bone.never_render == Some(true)
        || bone
            .rotation
            .is_some_and(|r| r.iter().any(|v| v.get() != 0.0))
        || bone
            .bind_pose_rotation
            .is_some_and(|r| r.iter().any(|v| v.get() != 0.0))
    {
        return None;
    }
    let mut faces = Vec::new();
    for cube in &bone.cubes {
        if cube.rotation.iter().any(|v| v.get() != 0.0) {
            return None;
        }
        let [width, height, depth] = cube.size.map(|v| v.get());
        let (mut origin, mut across, down) = match &cube.uv {
            EntityGeometryUv::Box(uv) => (
                [uv[0].get() + depth, uv[1].get() + depth],
                [width, 0.0],
                [0.0, height],
            ),
            EntityGeometryUv::Faces(uv) => {
                let Some(front) = &uv.north else {
                    continue;
                };
                let size = front
                    .uv_size
                    .map(|v| v.map(|v| v.get()))
                    .unwrap_or([width, height]);
                (front.uv.map(|v| v.get()), [size[0], 0.0], [0.0, size[1]])
            }
        };
        if cube.mirror && matches!(cube.uv, EntityGeometryUv::Box(_)) {
            origin[0] += across[0];
            across[0] = -across[0];
        }
        faces.push(Face {
            origin,
            across,
            down,
            area: width * height,
        });
    }
    if let Some(mesh) = &model.poly_meshes[index] {
        // The skin parser triangulates each quad into six corners.
        for vertices in mesh.vertices.as_chunks::<6>().0 {
            if vertices.iter().any(|v| v.normal != [0.0, 0.0, -1.0]) {
                continue;
            }
            let min_x = vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::INFINITY, f32::min);
            let max_x = vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::NEG_INFINITY, f32::max);
            let min_y = vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::INFINITY, f32::min);
            let max_y = vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::NEG_INFINITY, f32::max);
            let uv = |x, y| {
                let v = vertices
                    .iter()
                    .find(|v| v.position == [x, y, vertices[0].position[2]])?;
                let [mut u, mut v] = v.uv;
                if mesh.normalized_uvs {
                    u *= model.texture_width as f32;
                    v *= model.texture_height as f32;
                }
                Some([u, model.texture_height as f32 - v])
            };
            if let (Some(origin), Some(right), Some(bottom), Some(far)) = (
                uv(min_x, max_y),
                uv(max_x, max_y),
                uv(min_x, min_y),
                uv(max_x, min_y),
            ) {
                if (0..2).any(|i| (far[i] - right[i] - bottom[i] + origin[i]).abs() > 0.00001) {
                    continue;
                }
                faces.push(Face {
                    origin,
                    across: std::array::from_fn(|i| right[i] - origin[i]),
                    down: std::array::from_fn(|i| bottom[i] - origin[i]),
                    area: (max_x - min_x) * (max_y - min_y),
                });
            }
        }
    }
    faces
        .into_iter()
        .filter(|f| f.area > 0.0)
        .max_by(|a, b| a.area.total_cmp(&b.area))
}

/// Samples at texel centers and rejects UVs outside the transmitted image.
fn sample(
    pixels: &[u8],
    dimensions: [u32; 2],
    texture: [f32; 2],
    face: Face,
    size: u32,
) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let [u, v] = [x, y].map(|v| (v as f32 + 0.5) / size as f32);
            let texel: [f32; 2] = std::array::from_fn(|i| {
                (face.origin[i] + face.across[i] * u + face.down[i] * v) * dimensions[i] as f32
                    / texture[i]
            });
            if (0..2).any(|i| {
                !texel[i].is_finite() || texel[i] < 0.0 || texel[i] >= dimensions[i] as f32
            }) {
                return None;
            }
            let offset = (texel[1] as usize * dimensions[0] as usize + texel[0] as usize) * 4;
            output.extend_from_slice(pixels.get(offset..offset + 4)?);
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests;
