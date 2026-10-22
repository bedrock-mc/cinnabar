//! Builds bounded catalog attachments from verified geometry, texture and animation bytes.
use super::*;

/// Decodes an admitted attachment and checks parents before producing immutable render data.
pub(super) fn build(
    item: &cosmetics::Descriptor,
    geometry: &str,
    texture: &[u8],
    animation: &[u8],
) -> Option<HatRig> {
    let patch = format!(
        r#"{{"geometry":{{"default":{}}}}}"#,
        serde_json::to_string(&item.geometry_id).ok()?
    );
    let model = assets::parse_skin_geometry(&patch, geometry).ok()??;
    if model.bones.is_empty() {
        return None;
    }
    let mut original = crate::skin_geometry(&model, EntityRigId(0)).ok()?;
    if original.vertices.len() > 24_576
        || original.vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 4.0)
        })
    {
        return None;
    }
    let head = model
        .bones
        .iter()
        .position(|bone| bone.name.as_ref() == item.attachment_bone.as_str())?;
    let mut parents: Vec<_> = model
        .bones
        .iter()
        .map(|bone| {
            bone.parent
                .as_ref()
                .and_then(|name| model.bones.iter().position(|bone| &bone.name == name))
        })
        .collect();
    if parents
        .iter()
        .enumerate()
        .any(|(index, parent)| index != head && parent.is_none_or(|parent| parent >= index))
    {
        return None;
    }
    let mut definition: cosmetics_animation::Animation = if animation.is_empty() {
        Default::default()
    } else {
        serde_json::from_slice(animation).ok()?
    };
    if definition.cloth.is_some() {
        definition.wind = None;
    }
    if definition.wind.as_ref().is_some_and(|wind| {
        !["x", "y", "z"].contains(&wind.axis.as_str())
            || ![wind.max_degrees, wind.speed, wind.rise, wind.fall]
                .iter()
                .all(|value| value.is_finite() && *value > 0.0)
            || wind.max_degrees > 90.0
            || wind.speed > 100.0
            || wind.rise > 100.0
            || wind.fall > 100.0
    }) {
        return None;
    }
    let mut flex: Vec<_> = model
        .bones
        .iter()
        .map(|bone| {
            definition
                .wind
                .as_ref()
                .and_then(|wind| wind.joints.get(bone.name.as_ref()))
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    if flex
        .iter()
        .any(|weight| !weight.is_finite() || !(0.0..=1.0).contains(weight))
    {
        return None;
    }
    let mut vertices = original.vertices.to_vec();
    for vertex in &mut vertices {
        if let Some(joint) = model.bones.iter().enumerate().position(|(index, _)| {
            flex[index] > 0.0 && {
                let mut parent = parents[index];
                let mut descendant = false;
                while let Some(index) = parent {
                    if index == vertex.bone_index as usize {
                        descendant = true;
                        break;
                    }
                    parent = parents[index];
                }
                descendant && (original.bone_pivots[index][1] - vertex.position[1]).abs() < 1e-5
            }
        }) {
            vertex.bone_index = joint as u32;
        }
    }
    if item.render_flags.double_sided {
        let back = vertices
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|triangle| [triangle[2], triangle[1], triangle[0]])
            .collect::<Vec<_>>();
        vertices.extend(back);
    }
    original.vertices = vertices.into();
    let decoder = image::ImageReader::new(std::io::Cursor::new(texture))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(4 * 1024 * 1024);
    let mut decoder = decoder;
    decoder.limits(limits);
    let image = decoder.decode().ok()?.into_rgba8();
    if image.width() > 256
        || image.height() > 256
        || image.width() != u32::from(model.texture_width)
        || image.height() != u32::from(model.texture_height)
    {
        return None;
    }
    let pixels: SkinRgba8 = image::imageops::resize(
        &image,
        crate::STANDARD_SKIN_SIDE as u32,
        crate::STANDARD_SKIN_SIDE as u32,
        image::imageops::FilterType::Nearest,
    )
    .into_raw()
    .into();
    let world_pixels = item
        .render_flags
        .emissive
        .then(|| emissive_raster(&pixels).into());
    let texture = CapeImage {
        width: image.width(),
        height: image.height(),
        rgba8: image.into_raw().into(),
    };
    let names: Vec<_> = model.bones.iter().map(|bone| bone.name.as_ref()).collect();
    let emitters = emitters::build(
        std::mem::take(&mut definition.particles),
        &names,
        item.assets
            .animation
            .as_ref()
            .map(|asset| asset.sha256.as_str()),
        (&texture, &item.assets.texture.sha256),
    )?;
    let companion = match definition.companion.take() {
        Some(controller) => {
            if item.slot != "pet"
                || definition.cloth.is_some()
                || definition.tip_flex.is_some()
                || definition.wind.is_some()
                || definition.playback.is_some()
            {
                return None;
            }
            Some(super::companion::Rig::build(
                controller,
                std::mem::take(&mut definition.animations),
                &names,
            )?)
        }
        None => None,
    };
    let playback =
        cosmetics_animation::Playback::build(&mut definition, &names, &mut original, &mut parents)?;
    while flex.len() < parents.len() {
        flex.push(flex[parents[flex.len()]?]);
    }
    let mut rest_rotations: Vec<_> = model
        .bones
        .iter()
        .map(|bone| {
            bone.rotation.map_or(Quat::IDENTITY, |rotation| {
                let [x, y, z] = rotation.map(|value| value.get().to_radians());
                Quat::from_euler(glam::EulerRot::ZYX, -z, y, -x)
            })
        })
        .collect();
    let cloth = match definition.cloth.as_ref() {
        Some(definition) => Some(super::cloth::ClothRig::build(
            definition,
            &names,
            &mut original,
            &mut parents,
        )?),
        None => None,
    };
    flex.resize(parents.len(), 0.0);
    rest_rotations.resize(parents.len(), Quat::IDENTITY);
    let mut rig = HatRig {
        playback,
        companion,
        geometry: original,
        parents,
        flex,
        rest_rotations,
        head,
        offset: item.offset,
        rest_vertices: Arc::from([]),
        preview_part: crate::STANDARD_BIPED_BONES
            .iter()
            .position(|bone| bone.eq_ignore_ascii_case(&item.attachment_bone))
            .map_or(u32::MAX, |part| part as u32),
        preview_frame: Default::default(),
        texture,
        pixels,
        world_pixels,
        emitters,
        cloth,
        wind: definition.wind,
    };
    rig.rest_vertices = rig.preview_vertices(rig.preview_motion(0.0)).into();
    let clearance = super::companion::clearance::build(&rig);
    if let Some(companion) = &mut rig.companion {
        companion.clearance = clearance;
    }
    Some(rig)
}

/// Converts preview texels to the emissive-alpha world raster: opaque texels keep their colour
/// with zero alpha, which the actor shader draws fullbright, and every other texel becomes the
/// all-zero value it discards. Opaque black is lifted to the darkest colour that still draws.
pub(super) fn emissive_raster(rgba8: &[u8]) -> Arc<[u8]> {
    rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| match a {
            128.. if [r, g, b] == [0; 3] => [1, 1, 1, 0],
            128.. => [r, g, b, 0],
            _ => [0; 4],
        })
        .collect()
}

impl HatRig {
    /// Whether the world draws this item fullbright from [`Self::world_pixels`].
    pub fn emissive(&self) -> bool {
        self.world_pixels.is_some()
    }

    /// The raster world submissions sample; previews and thumbnails keep [`Self::pixels`].
    pub fn world_pixels(&self) -> &SkinRgba8 {
        self.world_pixels.as_ref().unwrap_or(&self.pixels)
    }
}
