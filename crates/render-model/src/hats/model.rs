//! Loads original headwear and binds flexible surface rings to their authored joints.

use super::*;

/// Builds immutable headwear with no internal caps or disconnected joint surfaces.
pub(super) fn build(hat: Hat) -> HatRig {
    let source = source(hat);
    let patch = format!(
        r#"{{"geometry":{{"default":"geometry.cinnabar.{}"}}}}"#,
        hat.id()
    );
    let model = assets::parse_skin_geometry(&patch, source.geometry)
        .expect("original headwear geometry is valid")
        .expect("original headwear geometry exists");
    let index = Hat::ALL
        .iter()
        .position(|entry| *entry == hat)
        .expect("catalogued hat");
    let original = crate::skin_geometry(&model, EntityRigId(0xDFFF_FFFF - index as u32))
        .expect("original headwear mesh is valid");
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
        let Some(first) = joints.first().copied() else {
            continue;
        };
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
    let image = image::load_from_memory(source.texture)
        .expect("original headwear texture is valid")
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

/// The editable project and its runtime export travel with their original texture.
pub(super) struct Source {
    #[cfg(test)]
    pub project: &'static str,
    pub geometry: &'static str,
    pub texture: &'static [u8],
}

/// Exhaustive asset resolution keeps every catalog choice backed by an authored model.
pub(super) fn source(hat: Hat) -> Source {
    macro_rules! source {
        ($name:literal) => {
            Source {
                #[cfg(test)]
                project: include_str!(concat!($name, ".bbmodel")),
                geometry: include_str!(concat!($name, ".geo.json")),
                texture: include_bytes!(concat!($name, ".png")),
            }
        };
    }
    match hat {
        Hat::BunnyEars => source!("bunny_ears"),
        Hat::Crown => source!("crown"),
    }
}
