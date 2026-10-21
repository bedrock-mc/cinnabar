//! Visible prepared mesh contacts shared by the body and its attached layers.
use super::*;
use articulated::{BoneBounds, Model};

/// Excludes a piece only when every draw of the body hides it, including generated joints.
pub(super) fn contacts(
    rig: &ActorRigSnapshot<'_>,
    model: Option<&Model>,
    names: &[Box<str>],
    height: f32,
) -> Vec<Option<BoneBounds>> {
    let hidden: Vec<_> = rig
        .render
        .iter()
        .filter(|layer| layer.geometry.is_none())
        .map(|layer| {
            model.map_or_else(
                || Arc::clone(&layer.hidden_bones),
                |model| model.hidden_bones(&layer.hidden_bones),
            )
        })
        .collect();
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if !hidden.is_empty() && hidden.iter().all(|draw| draw.contains(&(index as u32))) {
                return None;
            }
            if let Some(model) = model {
                return model.bounds[index];
            }
            let (width, bottom, top, depth) = match name.as_ref() {
                "head" => (4.0, 0.0, 8.0, 4.0),
                "body" => (4.0, -12.0, 0.0, 2.0),
                "leftarm" | "rightarm" | "leftleg" | "rightleg" => (2.0, -12.0, 0.0, 2.0),
                _ => return None,
            };
            let scale = height / 12.0;
            Some(BoneBounds {
                min: [-width * scale, bottom * scale, -depth * scale],
                max: [width * scale, top * scale, depth * scale],
            })
        })
        .collect()
}

/// Corrects visible contacts independently of authored root height and shares the correction with attachments.
pub(super) fn ground_pose(
    pose: &mut [BoneTransform],
    bounds: &[Option<BoneBounds>],
    root_height: f32,
) -> f32 {
    let lowest = pose
        .iter()
        .zip(bounds)
        .filter_map(|(target, bounds)| {
            let bounds = bounds.as_ref()?;
            let mut minimum = f32::INFINITY;
            for x in [bounds.min[0], bounds.max[0]] {
                for y in [bounds.min[1], bounds.max[1]] {
                    for z in [bounds.min[2], bounds.max[2]] {
                        minimum = minimum.min(
                            target.translation_scale[1]
                                + rotate_vector(
                                    target.rotation,
                                    [x, y, z].map(|v| v * target.translation_scale[3]),
                                )[1],
                        );
                    }
                }
            }
            Some(minimum)
        })
        .fold(f32::INFINITY, f32::min);
    if !lowest.is_finite() {
        return 0.0;
    }
    let correction = root_height - lowest;
    for target in pose {
        target.translation_scale[1] += correction;
    }
    correction
}
