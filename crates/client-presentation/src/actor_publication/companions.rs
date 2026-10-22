//! World collision adapter and per-wearer pose smoothing for ground cosmetics.
use glam::{Mat4, Quat, Vec3};
use render_model::{ActorRigSubmission, RenderBoneTransform, hats};
use sim::CollisionWorld;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct State {
    follower: hats::companion::Follower,
    pose: Arc<[RenderBoneTransform]>,
    lifetime: Option<(u64, i32, u64, u64)>,
}

type Frame = ([[f32; 4]; 3], Arc<[RenderBoneTransform]>);

impl State {
    /// Retains movement across body pose and perspective resets, but not actor lifetimes.
    fn retain_owner(&mut self, body: &render_model::ActorRigRenderInput) {
        let lifetime = (
            body.identity.session_id,
            body.identity.dimension,
            body.identity.runtime_id,
            body.identity.spawn_revision,
        );
        if self.lifetime != Some(lifetime) {
            *self = Self {
                lifetime: Some(lifetime),
                ..Default::default()
            };
        }
    }

    /// Resolves an independent world placement against current authoritative block shapes.
    pub(super) fn advance(
        &mut self,
        item: cosmetics::Item,
        body: &ActorRigSubmission,
        _yaw: f32,
        seconds: f32,
        stream: &chunk_pipeline::WorldStream,
        collisions: &dyn crate::observations::CollisionLookup,
    ) -> Option<Frame> {
        self.retain_owner(&body.input);
        let world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        );
        let model = hats::rig(item);
        let rig = model.companion()?;
        let owner = hats::cloth::placement(body.world_from_actor).transform_point3(Vec3::ZERO);
        let owner_heading = body.world_from_actor[0][2].atan2(-body.world_from_actor[2][2]);
        let result = self.follower.advance(
            rig,
            owner,
            owner_heading,
            seconds,
            |feet, rise, drop| floor(&world, feet, rise, drop),
            |feet, yaw, delta| clip_body_motion(&world, feet, yaw, delta, rig.clearance()),
            |feet, yaw| clear_body(&world, feet, yaw, rig.clearance()),
        );
        if let Some(feet) = self.follower.position() {
            tracing::trace!(
                target: "cinnabar::companion",
                dt = seconds,
                owner_x = owner.x, owner_y = owner.y, owner_z = owner.z, owner_yaw = owner_heading,
                feet_x = feet.x, feet_y = feet.y, feet_z = feet.z,
                visible = result.is_some(),
                yaw = result.as_ref().map(|frame| frame.yaw), gait = ?result.as_ref().map(|frame| frame.pose.gait),
                body_clear = result.as_ref().is_some_and(|frame| clear_body(&world, feet, frame.yaw, rig.clearance())),
                "companion motion"
            );
        }
        let frame = result?;
        let current = model.companion_pose(frame.pose);
        let weight = 1.0 - (-18.0 * seconds.max(0.0)).exp();
        let smoothed: Arc<[RenderBoneTransform]> = current
            .iter()
            .enumerate()
            .map(|(index, current)| {
                let Some(previous) = self.pose.get(index) else {
                    return *current;
                };
                let mut pose = *current;
                pose.rotation = Quat::from_array(previous.rotation)
                    .slerp(Quat::from_array(current.rotation), weight)
                    .to_array();
                for axis in 0..3 {
                    pose.translation_scale[axis] = previous.translation_scale[axis]
                        + (current.translation_scale[axis] - previous.translation_scale[axis])
                            * weight;
                }
                pose
            })
            .collect();
        self.pose = Arc::clone(&smoothed);
        let placement = Mat4::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::PI - frame.yaw),
            frame.feet,
        );
        let cols = placement.to_cols_array_2d();
        Some((
            [0, 1, 2].map(|row| [cols[0][row], cols[1][row], cols[2][row], cols[3][row]]),
            smoothed,
        ))
    }
}

/// Finds a support top intersecting the collision footprint with room for the companion above it.
/// Unknown chunks remain unavailable rather than becoming safe ground.
fn floor(world: &impl CollisionWorld, feet: Vec3, rise: f32, drop: f32) -> Option<f32> {
    let vector = |p: Vec3| sim::Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
    let min = feet - Vec3::new(RADIUS, drop + 0.01, RADIUS);
    let max = feet + Vec3::new(RADIUS, rise + HEIGHT + 0.01, RADIUS);
    let boxes = world
        .collision_boxes(sim::Aabb::new(vector(min), vector(max)))
        .ok()?
        .value;
    let mut supports: Vec<_> = boxes
        .iter()
        .filter_map(|bounds| {
            let y = bounds.max.y as f32;
            (y <= feet.y + rise + 0.001 && y >= feet.y - drop - 0.001).then_some(y)
        })
        .collect();
    supports.sort_by(|a, b| b.total_cmp(a));
    supports.into_iter().find(|&y| {
        let body = sim::Aabb::new(
            vector(Vec3::new(min.x, y + 0.001, min.z)),
            vector(Vec3::new(max.x, y + HEIGHT, max.z)),
        );
        if boxes.iter().any(|bounds| bounds.intersects(body)) {
            return false;
        }
        true
    })
}

#[cfg(test)]
mod tests;

/// Horizontal half-width and standing body height used by support and sweep queries.
const RADIUS: f32 = 0.23;
const HEIGHT: f32 = 0.85;

/// Clips the actual airborne body against floor, wall and ceiling collision shapes.
#[cfg(test)]
fn clip_motion(world: &impl CollisionWorld, feet: Vec3, delta: Vec3) -> Vec3 {
    clip_body_motion(world, feet, 0.0, delta, &[])
}

/// Sweeps the support body together with all protruding animated body sections.
fn clip_body_motion(
    world: &impl CollisionWorld,
    feet: Vec3,
    yaw: f32,
    delta: Vec3,
    sections: &[hats::companion::Clearance],
) -> Vec3 {
    let vector = |p: Vec3| sim::Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
    let support = sim::Aabb::new(
        vector(feet - Vec3::new(RADIUS, 0.0, RADIUS)),
        vector(feet + Vec3::new(RADIUS, HEIGHT, RADIUS)),
    );
    let mut bodies = collision_bodies(feet, yaw, sections);
    let movement = vector(delta);
    let mut query = support;
    for body in &bodies {
        query.min = query.min.component_min(body.min);
        query.max = query.max.component_max(body.max);
    }
    let Ok(boxes) = world.collision_boxes(query.swept(movement).grown(0.001)) else {
        return Vec3::ZERO;
    };
    let mut result = sim::Vec3::ZERO;
    for axis in [0, 1, 2] {
        if movement[axis] == 0.0 {
            continue;
        }
        let mut step = sim::Vec3::ZERO;
        step[axis] = movement[axis];
        for body in &bodies {
            for bounds in &boxes.value {
                if body.intersects(*bounds) {
                    // Changed terrain may overlap an existing body. Permit bounded escape
                    // without increasing its intersection with any of the changed shapes.
                    let overlap = |body: sim::Aabb| {
                        let min = body.min.component_max(bounds.min);
                        let max = body.max.component_min(bounds.max);
                        (max.x - min.x).max(0.0)
                            * (max.y - min.y).max(0.0)
                            * (max.z - min.z).max(0.0)
                    };
                    if overlap(body.translated(step)) > overlap(*body) + 1e-8 {
                        step[axis] = 0.0;
                    }
                } else {
                    // Sweeps own only their selected axis, including near touching faces.
                    step[axis] = body.clip_against(*bounds, step)[axis];
                }
            }
        }
        for body in &mut bodies {
            *body = body.translated(step);
        }
        result += step;
    }
    Vec3::new(result.x as f32, result.y as f32, result.z as f32)
}

/// Builds the support body first, followed by the model's protruding clearance sections.
fn collision_bodies(
    feet: Vec3,
    yaw: f32,
    sections: &[hats::companion::Clearance],
) -> Vec<sim::Aabb> {
    let vector = |p: Vec3| sim::Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
    let mut bodies = Vec::with_capacity(sections.len() + 1);
    bodies.push(sim::Aabb::new(
        vector(feet - Vec3::new(RADIUS, 0.0, RADIUS)),
        vector(feet + Vec3::new(RADIUS, HEIGHT, RADIUS)),
    ));
    for section in sections {
        let centre = feet
            + Quat::from_rotation_y(std::f32::consts::PI - yaw)
                * Vec3::new(section.centre[0], 0.0, section.centre[1]);
        bodies.push(sim::Aabb::new(
            vector(centre + Vec3::new(-section.radius, section.bottom, -section.radius)),
            vector(centre + Vec3::new(section.radius, section.top, section.radius)),
        ));
    }
    bodies
}

/// Rejects standing destinations where any body section is embedded in a block.
fn clear_body(
    world: &impl CollisionWorld,
    feet: Vec3,
    yaw: f32,
    sections: &[hats::companion::Clearance],
) -> bool {
    collision_bodies(feet, yaw, sections)
        .into_iter()
        .all(|body| {
            world
                .collision_boxes(body)
                .is_ok_and(|boxes| !boxes.value.iter().any(|bounds| bounds.intersects(body)))
        })
}

/// Samples the companion's own ground and upper body, identically in every perspective.
pub(super) fn lighting(
    placement: [[f32; 4]; 3],
    mut sample: impl FnMut([f32; 3]) -> Option<(u8, u8)>,
) -> u32 {
    let feet = placement.map(|row| row[3]);
    let bottom = sample([feet[0], feet[1] + 0.01, feet[2]]);
    let top = sample([feet[0], feet[1] + HEIGHT + 0.01, feet[2]]);
    let light = match (bottom, top) {
        (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.max(b.1))),
        (a, b) => a.or(b),
    };
    light.map_or(0, |(block, sky)| render_model::pack_actor_light(block, sky))
}
