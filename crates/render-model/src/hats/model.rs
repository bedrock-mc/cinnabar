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
    let pixels = image::imageops::resize(
        &image,
        crate::STANDARD_SKIN_SIDE as u32,
        crate::STANDARD_SKIN_SIDE as u32,
        image::imageops::FilterType::Nearest,
    )
    .into_raw()
    .into();
    let texture = CapeImage {
        width: image.width(),
        height: image.height(),
        rgba8: image.into_raw().into(),
    };
    let playback = cosmetics_animation::Playback::build(
        &mut definition,
        &model
            .bones
            .iter()
            .map(|bone| bone.name.as_ref())
            .collect::<Vec<_>>(),
        &mut original,
        &mut parents,
    )?;
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
    rest_rotations.resize(parents.len(), Quat::IDENTITY);
    let mut rig = HatRig {
        playback,
        geometry: original,
        parents,
        flex,
        rest_rotations,
        head,
        offset: item.offset,
        rest_vertices: Arc::from([]),
        texture,
        pixels,
        wind: definition.wind,
    };
    let root = rig.geometry.bone_pivots[head];
    let poses = rig.pose(
        RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [root[0], root[1], root[2], 1.0],
            axis_scale: crate::UNIT_AXIS_SCALE,
        },
        Motion::default(),
    );
    let part = crate::STANDARD_BIPED_BONES
        .iter()
        .position(|bone| bone.eq_ignore_ascii_case(&item.attachment_bone))
        .map_or(u32::MAX, |part| part as u32);
    rig.rest_vertices = rig
        .geometry
        .vertices
        .iter()
        .map(|vertex| {
            let bone = vertex.bone_index as usize;
            let pose = poses[bone];
            let position = Vec3::from_slice(&pose.translation_scale[..3])
                + Quat::from_array(pose.rotation)
                    * (Vec3::from_array(vertex.position)
                        - Vec3::from_array(rig.geometry.bone_pivots[bone]));
            ActorVertex {
                position: [-position.x, position.y, -position.z],
                uv: vertex.uv,
                part,
            }
        })
        .collect::<Vec<_>>()
        .into();
    Some(rig)
}
