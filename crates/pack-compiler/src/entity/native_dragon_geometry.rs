//! Expands the downloaded dragon template into the segments its authored rig animates.

use std::collections::BTreeMap;

use assets::{EntityGeometryScalar, gui_item::SHIELD_MODEL_PART_HEIGHT};
use sha2::{Digest, Sha256};

use super::PendingGeometry;

const PATH: &str = "models/entity/ender_dragon.geo.json";
const GEOMETRY: &str = "geometry.dragon";
const RELATIVE_PARTS: [(&str, &str); 11] = [
    ("jaw", "head"),
    ("wingtip", "wing"),
    ("wingtip1", "wing1"),
    ("rearlegtip", "rearleg"),
    ("rearlegtip1", "rearleg1"),
    ("frontlegtip", "frontleg"),
    ("frontlegtip1", "frontleg1"),
    ("rearfoot", "rearlegtip"),
    ("rearfoot1", "rearlegtip1"),
    ("frontfoot", "frontlegtip"),
    ("frontfoot1", "frontlegtip1"),
];
const SAMPLE_SHA256: [u8; 32] = [
    0x07, 0xc9, 0x33, 0x3d, 0xc9, 0x13, 0xe0, 0x46, 0xed, 0x04, 0x11, 0x43, 0x62, 0x4a, 0x5e, 0xb3,
    0x0a, 0x33, 0xcb, 0x34, 0x0f, 0xa0, 0x7b, 0x10, 0xd5, 0x07, 0xa8, 0xc9, 0x5b, 0xe6, 0x08, 0xc6,
];

pub(super) fn expand_sample(
    path: &str,
    bytes: &[u8],
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) {
    if path != PATH || <[u8; 32]>::from(Sha256::digest(bytes)) != SAMPLE_SHA256 {
        return;
    }
    if let Some(geometry) = geometries.get_mut(&(GEOMETRY.into(), PATH.into())) {
        expand(geometry);
    }
}

fn expand(geometry: &mut PendingGeometry) {
    let Some(mut neck) = geometry
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == "neck")
        .cloned()
    else {
        return;
    };
    neck.parent = Some("root".into());
    let mut bones = geometry.bones.to_vec();
    bones.retain(|bone| bone.name.as_ref() != "neck");
    for bone in &mut bones {
        let parent = match bone.name.as_ref() {
            "root" => None,
            name => RELATIVE_PARTS
                .iter()
                .find_map(|(child, parent)| (*child == name).then_some(*parent))
                .or(bone.parent.as_deref())
                .or(Some("root")),
        };
        bone.parent = parent.map(Into::into);
        if matches!(bone.name.as_ref(), "wing" | "wing1") {
            let mut rotation = bone.rotation.unwrap_or([EntityGeometryScalar::ZERO; 3]);
            rotation[1] = EntityGeometryScalar::new(rotation[1].get() + 14.3)
                .expect("dragon wing base rotation is finite");
            if bone.name.as_ref() == "wing1" {
                rotation[2] = EntityGeometryScalar::new(rotation[2].get() + 180.0)
                    .expect("dragon mirrored wing base rotation is finite");
            }
            bone.rotation = Some(rotation);
        }
    }
    // These parts use parent-relative pivots with ModelPart's Y origin.
    // Keep cube coordinates relative to their pivot while converting to model space.
    for (child, parent) in RELATIVE_PARTS {
        let parent_pivot = bones
            .iter()
            .find(|bone| bone.name.as_ref() == parent)
            .and_then(|bone| bone.pivot)
            .expect("downloaded dragon parent pivot");
        let offset = [
            parent_pivot[0].get(),
            parent_pivot[1].get() - SHIELD_MODEL_PART_HEIGHT,
            parent_pivot[2].get(),
        ];
        let bone = bones
            .iter_mut()
            .find(|bone| bone.name.as_ref() == child)
            .expect("downloaded dragon child part");
        let translate = |point: [EntityGeometryScalar; 3]| {
            std::array::from_fn(|axis| {
                EntityGeometryScalar::new(point[axis].get() + offset[axis])
                    .expect("downloaded dragon child frame is finite")
            })
        };
        bone.pivot = bone.pivot.map(translate);
        for cube in &mut bone.cubes {
            cube.origin = translate(cube.origin);
            cube.pivot = translate(cube.pivot);
        }
    }
    for (prefix, count) in [("neck", 5), ("tail", 12)] {
        for index in 1..=count {
            let mut segment = neck.clone();
            segment.name = format!("{prefix}{index}").into();
            bones.push(segment);
        }
    }
    geometry.bones = bones.into_boxed_slice();
}

#[cfg(test)]
mod tests;
