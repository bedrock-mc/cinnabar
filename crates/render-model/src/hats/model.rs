//! Loads the original continuous surface and binds each shared ring to its joint.

use super::*;

/// Builds immutable headwear with no internal caps or disconnected joint surfaces.
pub(super) fn build() -> HatRig {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.cinnabar.bunny_ears"}}"#,
        include_str!("bunny_ears.geo.json"),
    )
    .expect("original bunny ears geometry is valid")
    .expect("original bunny ears geometry exists");
    let original = crate::skin_geometry(&model, EntityRigId(0xDFFF_FFFF))
        .expect("original bunny ears mesh is valid");
    let head = model
        .bones
        .iter()
        .position(|bone| bone.name.eq_ignore_ascii_case("head"))
        .expect("headwear has a head attachment");
    let parents: Vec<_> = model
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| {
            bone.parent.as_ref().map(|name| {
                let parent = model
                    .bones
                    .iter()
                    .position(|bone| &bone.name == name)
                    .expect("headwear parent exists");
                assert!(parent < index, "headwear parents precede their children");
                parent
            })
        })
        .collect();
    let mut vertices = original.vertices.to_vec();
    let mut flex = vec![0.0; model.bones.len()];
    for (base, mesh) in model.poly_meshes.iter().enumerate() {
        if mesh.is_none() {
            continue;
        }
        let joints: Vec<_> = (0..model.bones.len())
            .filter(|&index| {
                let mut cursor = parents[index];
                while let Some(parent) = cursor {
                    if parent == base {
                        return true;
                    }
                    cursor = parents[parent];
                }
                false
            })
            .collect();
        let first = joints.first().copied().expect("flexible ear has joints");
        let last = joints.last().copied().expect("flexible ear has joints");
        let low = original.bone_pivots[first][1];
        let span = original.bone_pivots[last][1] - low;
        assert!(span > 0.0, "flexible ear joints have height");
        for &joint in &joints {
            flex[joint] = (original.bone_pivots[joint][1] - low) / span;
        }
        for vertex in vertices
            .iter_mut()
            .filter(|vertex| vertex.bone_index as usize == base)
        {
            if let Some(&joint) = joints
                .iter()
                .find(|&&joint| (original.bone_pivots[joint][1] - vertex.position[1]).abs() < 1e-5)
            {
                vertex.bone_index = joint as u32;
            }
        }
    }
    let geometry =
        crate::ActorRigGeometry::new(original.id, vertices, original.bone_pivots.clone())
            .expect("continuous headwear mesh is valid");
    let image = image::load_from_memory(include_bytes!("bunny_ears.png"))
        .expect("original bunny ears texture is valid")
        .into_rgba8();
    assert_eq!(
        (image.width(), image.height()),
        (
            u32::from(model.texture_width),
            u32::from(model.texture_height)
        ),
        "headwear geometry and texture dimensions agree"
    );
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
    let rest_vertices = geometry
        .vertices
        .iter()
        .map(|vertex| ActorVertex {
            position: [-vertex.position[0], vertex.position[1], -vertex.position[2]],
            uv: vertex.uv,
            part: 0,
        })
        .collect::<Vec<_>>()
        .into();
    HatRig {
        geometry,
        parents,
        flex,
        head,
        rest_vertices,
        texture,
        pixels,
    }
}
