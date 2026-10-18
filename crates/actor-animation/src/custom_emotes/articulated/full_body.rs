//! Original texture-preserving elbows, wrists and lower torso for catalog clip playback.
use super::*;

/// Splits arm and torso cubes, preserving bind positions and recording their source bones.
pub(super) fn extend(
    source: &SkinGeometry,
    model: &mut SkinGeometry,
    sources: &mut Vec<u32>,
) -> Option<()> {
    let find = |name: &str| {
        source
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(name))
    };
    if source.bones.iter().any(|bone| {
        bone.name.to_ascii_lowercase().contains(".cinnabar_") || validate(bone).is_none()
    }) {
        return None;
    }
    let body = find("body")?;
    if source.bones[body].cubes.is_empty() {
        return None;
    }
    let head = find("head")?;
    let arms = [find("leftarm")?, find("rightarm")?];
    let hip_y = source.bones[find("leftleg")?].pivot?[1].get();
    let mut bones = model.bones.to_vec();
    for root in arms {
        let pivot = source.bones[root].pivot?;
        let bottom = source.bones[root]
            .cubes
            .iter()
            .map(|cube| cube.origin[1].get())
            .reduce(f32::min)?;
        let length = pivot[1].get() - bottom;
        if !(8.0..=16.0).contains(&length) {
            return None;
        }
        let elbow = pivot[1].get() - length * 0.5;
        let wrist = bottom + length / 6.0;
        for (index, bone) in source
            .bones
            .iter()
            .enumerate()
            .filter(|(index, _)| descendant(source, *index, root))
        {
            if bone.cubes.is_empty() {
                if bone.name.eq_ignore_ascii_case("leftitem")
                    || bone.name.eq_ignore_ascii_case("rightitem")
                {
                    bones[index].parent =
                        Some(format!("{}.cinnabar_wrist", source.bones[root].name).into());
                }
                continue;
            }
            let overlap = length / 12.0;
            let mut upper = Vec::new();
            let mut lower = Vec::new();
            let mut hand = Vec::new();
            for cube in &bone.cubes {
                upper.push(split(cube, elbow - overlap)?.0);
                let forearm = split(cube, elbow + overlap)?.1;
                // The hand encloses the wrist. End the forearm at its pivot so
                // its corners cannot protrude through a flat supporting palm.
                lower.push(split(&forearm, wrist)?.0);
                hand.push(split(&forearm, wrist + overlap)?.1);
            }
            bones[index].cubes = upper.into();
            let mut forearm = bone.clone();
            forearm.name = format!("{}.cinnabar_elbow", bone.name).into();
            forearm.parent = Some(if index == root {
                bone.name.clone()
            } else {
                format!("{}.cinnabar_elbow", source.bones[root].name).into()
            });
            forearm.pivot = Some([pivot[0], Scalar::new(elbow)?, pivot[2]]);
            forearm.cubes = lower.into();
            let mut palm = forearm.clone();
            palm.name = format!("{}.cinnabar_wrist", bone.name).into();
            palm.parent = Some(if index == root {
                forearm.name.clone()
            } else {
                format!("{}.cinnabar_wrist", source.bones[root].name).into()
            });
            palm.pivot = Some([pivot[0], Scalar::new(wrist)?, pivot[2]]);
            palm.cubes = hand.into();
            bones.push(forearm);
            bones.push(palm);
            sources.extend([index as u32; 2]);
        }
    }
    let pivot = source.bones[body].pivot?;
    let middle = (pivot[1].get() + hip_y) * 0.5;
    for (index, bone) in source.bones.iter().enumerate().filter(|(index, _)| {
        descendant(source, *index, body)
            && !descendant(source, *index, head)
            && !arms.iter().any(|root| descendant(source, *index, *root))
    }) {
        if bone.cubes.is_empty() {
            continue;
        }
        let overlap = (pivot[1].get() - hip_y) / 24.0;
        let mut upper = Vec::new();
        let mut lower = Vec::new();
        for cube in &bone.cubes {
            upper.push(split(cube, middle - overlap)?.0);
            lower.push(split(cube, middle + overlap)?.1);
        }
        bones[index].cubes = upper.into();
        let mut waist = bone.clone();
        waist.name = format!("{}.cinnabar_spine", bone.name).into();
        waist.parent = Some(if index == body {
            bone.name.clone()
        } else {
            format!("{}.cinnabar_spine", source.bones[body].name).into()
        });
        waist.pivot = Some([pivot[0], Scalar::new(middle)?, pivot[2]]);
        waist.cubes = lower.into();
        bones.push(waist);
        sources.push(index as u32);
    }
    if bones.len() > assets::MAX_SKIN_GEOMETRY_BONES {
        return None;
    }
    model.poly_meshes = vec![None; bones.len()].into();
    model.bones = bones.into();
    Some(())
}

/// Declines authored rotations and mesh/binding features that cannot be split faithfully.
fn validate(bone: &EntityGeometryBone) -> Option<()> {
    if bone
        .rotation
        .into_iter()
        .chain(bone.bind_pose_rotation)
        .flatten()
        .any(|value| value.get() != 0.0)
        || bone.binding.is_some()
        || !bone.texture_meshes.is_empty()
        || bone
            .cubes
            .iter()
            .any(|cube| cube.rotation.iter().any(|value| value.get() != 0.0))
    {
        return None;
    }
    Some(())
}
