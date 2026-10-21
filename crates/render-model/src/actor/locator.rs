//! Sampled actor attachment transforms shared with engine-free particle simulation.
use super::RenderBoneTransform;
use glam::{EulerRot, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorEmitterTransform {
    pub position: [f32; 3],
    /// Local emitter axes in world space, including inherited scale when requested.
    pub basis: [[f32; 3]; 3],
}

/// Samples a locator with the same point interpolation as the actor vertex path.
/// The origin retains inherited scale; the optional scale policy normalizes only its axes.
pub fn sampled_locator_transform(
    locator: &assets::EntityGeometryLocator,
    pivot: [f32; 3],
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    world: [[f32; 4]; 3],
    alpha: f32,
) -> Option<ActorEmitterTransform> {
    let [x, y, z] = locator.offset.map(|value| value.get());
    let local = Vec3::new(-x, y, z) / 16.0 - Vec3::from_array(pivot);
    let [x, y, z] = locator.rotation.map(|value| value.get().to_radians());
    let rotation = Quat::from_euler(EulerRot::ZYX, z, -y, -x);
    let mut result = sampled_frame(local, rotation, previous, current, world, alpha)?;
    if locator.ignore_inherited_scale {
        for axis in &mut result.basis {
            *axis = Vec3::from_array(*axis).try_normalize()?.to_array();
        }
    }
    Some(result)
}

/// Samples a point fixed in a posed bone, given as its offset from the bone's bind pivot in the
/// rig frame (blocks, authored X mirrored), with the same interpolation as the actor vertex path.
/// The basis is the bone's posed axes in world space, including inherited scale.
pub fn sampled_bone_offset(
    local: [f32; 3],
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    world: [[f32; 4]; 3],
    alpha: f32,
) -> Option<ActorEmitterTransform> {
    sampled_frame(
        Vec3::from_array(local),
        Quat::IDENTITY,
        previous,
        current,
        world,
        alpha,
    )
}

/// Places a pivot-relative point and rotated axes, rejecting any non-finite input or result.
fn sampled_frame(
    local: Vec3,
    rotation: Quat,
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    world: [[f32; 4]; 3],
    alpha: f32,
) -> Option<ActorEmitterTransform> {
    if !previous.is_finite()
        || !current.is_finite()
        || !alpha.is_finite()
        || !local.is_finite()
        || world.iter().flatten().any(|x| !x.is_finite())
    {
        return None;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    let position = to_world(world, sample(previous, current, local, alpha, true), true);
    let basis = [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| {
        to_world(
            world,
            sample(previous, current, rotation * axis, alpha, false),
            false,
        )
    });
    let result = ActorEmitterTransform {
        position: position.to_array(),
        basis: basis.map(|axis| axis.to_array()),
    };
    result
        .position
        .iter()
        .chain(result.basis.iter().flatten())
        .all(|x| x.is_finite())
        .then_some(result)
}

/// Applies each endpoint's completed bone transform before interpolating its point or axis.
fn sample(
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    local: Vec3,
    alpha: f32,
    point: bool,
) -> Vec3 {
    let transform = |bone: RenderBoneTransform| {
        let scaled = local * Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
        let rotated = Quat::from_array(bone.rotation) * scaled;
        if point {
            rotated + Vec3::from_slice(&bone.translation_scale[..3])
        } else {
            rotated
        }
    };
    transform(previous).lerp(transform(current), alpha)
}

/// Converts a model-space point or direction with the actor's sampled placement rows.
fn to_world(rows: [[f32; 4]; 3], value: Vec3, point: bool) -> Vec3 {
    Vec3::from_array(
        rows.map(|row| Vec3::from_slice(&row[..3]).dot(value) + if point { row[3] } else { 0.0 }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locator_tracks_sampled_bone_pivot_rotation_and_scale_policy() {
        let scalar = |x| assets::EntityGeometryScalar::new(x).unwrap();
        let mut locator = assets::EntityGeometryLocator {
            name: "wing".into(),
            offset: [-16.0, 16.0, 0.0].map(scalar),
            rotation: [0.0, 0.0, 90.0].map(scalar),
            ignore_inherited_scale: false,
        };
        let previous = RenderBoneTransform::from_model_space_scaled(
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 16.0, 0.0, 2.0],
            [1.0; 3],
        )
        .unwrap();
        let current = RenderBoneTransform::from_model_space_scaled(
            [0.0, 0.0, 0.0, 1.0],
            [32.0, 16.0, 0.0, 2.0],
            [1.0; 3],
        )
        .unwrap();
        let world = [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
        ];
        let sampled =
            sampled_locator_transform(&locator, [0.0, 1.0, 0.0], previous, current, world, 0.5)
                .unwrap();
        assert_eq!(sampled.position, [13.0, 21.0, 30.0]);
        assert!((Vec3::from_array(sampled.basis[0]) - Vec3::Y * 2.0).length() < 1e-5);
        locator.ignore_inherited_scale = true;
        let unscaled =
            sampled_locator_transform(&locator, [0.0, 1.0, 0.0], previous, current, world, 0.5)
                .unwrap();
        assert_eq!(unscaled.position, sampled.position);
        assert!((Vec3::from_array(unscaled.basis[0]) - Vec3::Y).length() < 1e-5);
    }
}
