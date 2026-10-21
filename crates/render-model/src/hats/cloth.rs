//! Shared-vertex cosmetic cloth with fixed-step constraints and world-space inertia.
use super::{HatRig, Item, RenderBoneTransform, rig};
use crate::{ActorRigGeometry, MAX_RENDER_BONES_PER_ACTOR, UNIT_AXIS_SCALE};
use glam::{Mat4, Quat, Vec3, Vec4};
use serde::Deserialize;
use std::sync::Arc;

const STEP: f32 = 1.0 / 120.0;
const ITERATIONS: usize = 9;
/// Small fabric separation from the actual skin or worn geometry, in model blocks.
const CONTACT_MARGIN: f32 = 0.0125;
const CONTACT_SEPARATION: f32 = 0.001;
/// Head contacts leave the neck open so the first free fabric row can meet its pins.
const HEAD_MARGIN: f32 = 0.018;
/// Keep most vertical avatar travel while retaining a small jump/landing response.
const VERTICAL_FOLLOW: f32 = 0.8;
/// Follow part of a run or sudden stop so the remaining inertia forms a gentle trailing drape.
const HORIZONTAL_FOLLOW: f32 = 0.4;
/// Retain the original air resistance during steady travel.
const WORLD_DRAG: f32 = 1.94;
/// Extra settling opposes motion relative to the attachment rather than steady running speed.
const SETTLING_DAMPING: f32 = 3.8 - WORLD_DRAG;

mod fitting;
use fitting::DepthFit;

/// Previous and current tick endpoints with identically sampled fabric particles.
pub type ClothPose = (Arc<[RenderBoneTransform]>, Arc<[RenderBoneTransform]>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    bone: String,
    columns: usize,
    rows: usize,
}

pub(super) struct ClothRig {
    source: usize,
    first: usize,
    columns: usize,
    rows: usize,
    rest: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    constraints: Vec<Constraint>,
}

#[derive(Clone)]
struct Constraint {
    a: usize,
    b: usize,
    length: f32,
    compliance: f32,
    lambda: f32,
}

impl ClothRig {
    /// Admit a bounded rectangular mesh, welding duplicate triangle corners to shared joints.
    pub(super) fn build(
        definition: &Definition,
        names: &[&str],
        geometry: &mut ActorRigGeometry,
        parents: &mut Vec<Option<usize>>,
    ) -> Option<Self> {
        let nx = definition.columns;
        let ny = definition.rows;
        if !(2..=16).contains(&nx) || !(2..=32).contains(&ny) {
            return None;
        }
        let source = names.iter().position(|name| *name == definition.bone)?;
        let first = parents.len();
        if first + (nx + 1) * (ny + 1) > MAX_RENDER_BONES_PER_ACTOR {
            return None;
        }
        let mut rest = Vec::<Vec3>::new();
        for vertex in geometry
            .vertices
            .iter()
            .filter(|v| v.bone_index as usize == source)
        {
            let point = Vec3::from_array(vertex.position);
            if !rest.contains(&point) {
                rest.push(point);
            }
        }
        if rest.len() != (nx + 1) * (ny + 1) {
            return None;
        }
        rest.sort_by(|a, b| b.y.total_cmp(&a.y).then(a.x.total_cmp(&b.x)));
        let width = rest[nx].x - rest[0].x;
        let length = rest[0].y - rest[ny * (nx + 1)].y;
        if !(0.1..=2.0).contains(&width) || !(0.1..=2.0).contains(&length) {
            return None;
        }
        for (i, point) in rest.iter().enumerate() {
            let x = i % (nx + 1);
            let y = i / (nx + 1);
            if (point.x - rest[0].x - width * x as f32 / nx as f32).abs() > 1e-4
                || (point.y - rest[0].y + length * y as f32 / ny as f32).abs() > 1e-4
            {
                return None;
            }
        }
        let mut vertices = geometry.vertices.to_vec();
        let mut triangles = Vec::new();
        for triangle in vertices.as_chunks_mut::<3>().0 {
            if triangle.iter().all(|v| v.bone_index as usize == source) {
                let mut indices = [0; 3];
                for (i, vertex) in triangle.iter_mut().enumerate() {
                    let index = rest.iter().position(|p| p.to_array() == vertex.position)?;
                    vertex.bone_index = (first + index) as u32;
                    indices[i] = index;
                }
                // Double-sided render copies must not double the aerodynamic force.
                let mut key = indices;
                key.sort_unstable();
                if !triangles.iter().any(|t: &[usize; 3]| {
                    let mut other = *t;
                    other.sort_unstable();
                    other == key
                }) {
                    if (rest[indices[1]] - rest[indices[0]])
                        .cross(rest[indices[2]] - rest[indices[0]])
                        .z
                        < 0.0
                    {
                        indices.swap(1, 2);
                    }
                    triangles.push(indices);
                }
            }
        }
        let mut pivots = geometry.bone_pivots.to_vec();
        pivots.extend(rest.iter().map(|p| p.to_array()));
        parents.extend(std::iter::repeat_n(Some(source), rest.len()));
        *geometry = ActorRigGeometry::new(geometry.id, vertices, pivots).ok()?;
        let mut constraints = Vec::new();
        for y in 0..=ny {
            for x in 0..=nx {
                let a = y * (nx + 1) + x;
                for (dx, dy, compliance) in [
                    (1, 0, 0.000002),
                    (0, 1, 0.000002),
                    (1, 1, 0.00002),
                    (-1, 1, 0.00002),
                    (2, 0, 0.009),
                    (0, 2, 0.009),
                ] {
                    let bx = x as isize + dx;
                    let by = y as isize + dy;
                    if bx >= 0 && bx <= nx as isize && by <= ny as isize {
                        let b = by as usize * (nx + 1) + bx as usize;
                        constraints.push(Constraint {
                            a,
                            b,
                            length: rest[a].distance(rest[b]),
                            compliance,
                            lambda: 0.0,
                        });
                    }
                }
            }
        }
        Some(Self {
            source,
            first,
            columns: nx,
            rows: ny,
            rest,
            triangles,
            constraints,
        })
    }
}

/// A conservative animated body box in world coordinates; cloth cannot enter its interior.
pub struct Collider {
    world: Mat4,
    inverse: Mat4,
    min: Vec3,
    max: Vec3,
    back_only: bool,
}

/// Convert the renderer's row-major placement to an affine matrix.
pub fn placement(rows: [[f32; 4]; 3]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(rows[0][0], rows[1][0], rows[2][0], 0.0),
        Vec4::new(rows[0][1], rows[1][1], rows[2][1], 0.0),
        Vec4::new(rows[0][2], rows[1][2], rows[2][2], 0.0),
        Vec4::new(rows[0][3], rows[1][3], rows[2][3], 1.0),
    )
}

impl Collider {
    /// Uses the rendered bone's own geometry instead of assuming a standard armor thickness.
    pub fn geometry(
        name: &str,
        geometry: &ActorRigGeometry,
        index: usize,
        bone: RenderBoneTransform,
        actor: Mat4,
    ) -> Option<Self> {
        let (min, max) = geometry.bone_bounds(index)?;
        let scale = Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
        if scale.min_element() <= 1e-5 {
            return None;
        }
        let world = actor
            * Mat4::from_scale_rotation_translation(
                scale,
                Quat::from_array(bone.rotation),
                Vec3::from_slice(&bone.translation_scale[..3]),
            );
        let margin = if name.eq_ignore_ascii_case("head") {
            HEAD_MARGIN
        } else {
            CONTACT_MARGIN
        };
        Some(Self {
            world,
            inverse: world.inverse(),
            min: Vec3::from_array(min) - Vec3::splat(margin),
            max: Vec3::from_array(max) + Vec3::splat(margin),
            back_only: name.eq_ignore_ascii_case("body"),
        })
    }

    /// Samples contacts with the renderer's endpoint interpolation, including rotating armor.
    pub fn sampled_geometry(
        name: &str,
        geometry: &ActorRigGeometry,
        index: usize,
        bones: [RenderBoneTransform; 2],
        actor: [[f32; 4]; 3],
        alpha: f32,
    ) -> Option<Self> {
        let sampled =
            crate::actor::sampled_bone_offset([0.0; 3], bones[0], bones[1], actor, alpha)?;
        let world = Mat4::from_cols(
            Vec3::from_array(sampled.basis[0]).extend(0.0),
            Vec3::from_array(sampled.basis[1]).extend(0.0),
            Vec3::from_array(sampled.basis[2]).extend(0.0),
            Vec3::from_array(sampled.position).extend(1.0),
        );
        if world.determinant().abs() < 1e-8 {
            return None;
        }
        let mut collider = Self::geometry(name, geometry, index, bones[1], Mat4::IDENTITY)?;
        collider.world = world;
        collider.inverse = world.inverse();
        Some(collider)
    }

    /// Build standard-avatar collision proxies from the evaluated head, body, arms and legs.
    pub fn biped(name: &str, bone: RenderBoneTransform, actor: Mat4) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        let (min, max, back_only) = match name.as_str() {
            "body" => ([-4.0, -12.0, -2.0], [4.0, 0.0, 2.0], true),
            "head" => ([-4.0, 0.0, -4.0], [4.0, 8.0, 4.0], false),
            "leftarm" | "rightarm" => ([-2.0, -10.0, -2.0], [2.0, 2.0, 2.0], false),
            "leftleg" | "rightleg" => ([-2.0, -12.0, -2.0], [2.0, 0.0, 2.0], false),
            _ => return None,
        };
        let scale = Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
        if scale.min_element() <= 1e-5 {
            return None;
        }
        let world = actor
            * Mat4::from_scale_rotation_translation(
                scale,
                Quat::from_array(bone.rotation),
                Vec3::from_slice(&bone.translation_scale[..3]),
            );
        let margin = if name == "head" {
            HEAD_MARGIN
        } else {
            CONTACT_MARGIN
        };
        Some(Self {
            world,
            inverse: world.inverse(),
            min: Vec3::from_array(min) / 16.0 - Vec3::splat(margin),
            max: Vec3::from_array(max) / 16.0 + Vec3::splat(margin),
            back_only,
        })
    }

    /// Project an interior point to the back of the torso or the nearest limb face.
    fn project(&self, point: &mut Vec3) {
        let mut local = self.inverse.transform_point3(*point);
        if !local.cmpgt(self.min).all() || !local.cmplt(self.max).all() {
            return;
        }
        let (mut axis, mut positive, mut distance) = (2, true, f32::INFINITY);
        if !self.back_only {
            for i in 0..3 {
                for sign in [false, true] {
                    let d = if sign {
                        self.max[i] - local[i]
                    } else {
                        local[i] - self.min[i]
                    };
                    if d < distance {
                        axis = i;
                        positive = sign;
                        distance = d;
                    }
                }
            }
        }
        local[axis] = if positive {
            self.max[axis] + CONTACT_SEPARATION
        } else {
            self.min[axis] - CONTACT_SEPARATION
        };
        *point = self.world.transform_point3(local);
    }
}

/// One actor's cloth history; immutable catalog geometry remains shared between all actors.
#[derive(Default)]
pub struct ClothState {
    points: Vec<Vec3>,
    old: Vec<Vec3>,
    normals: Vec<Vec3>,
    constraints: Vec<Constraint>,
    accumulator: f32,
    seconds: f32,
    previous_center: Option<Vec3>,
    last_pins: Vec<Vec3>,
    previous_scale: Option<Vec3>,
    previous_frame: Option<Mat4>,
    reanchor_pending: bool,
    fitting: Option<DepthFit>,
    retain_fitting: bool,
}

impl ClothState {
    /// Keeps the last drawn armor fit during one hidden-view update without freezing simulation.
    pub fn retain_fitting(&mut self) {
        self.retain_fitting = true;
    }

    /// Preserve the drape and velocity in attachment space when a view changes its body pose.
    pub fn reanchor(&mut self) {
        self.reanchor_pending = true;
    }

    /// Deform only the declared fabric; preserve the item's fixed clasps and ordinary animations.
    /// Timing contains frame duration and tick interpolation; fabric uses identical render endpoints.
    pub fn pose(
        &mut self,
        item: Item,
        bases: [Arc<[RenderBoneTransform]>; 2],
        actor: [[f32; 4]; 3],
        timing: (f32, f32),
        colliders: &[Collider],
    ) -> Option<ClothPose> {
        let model: &HatRig = rig(item);
        let cloth = model.cloth.as_ref()?;
        let (delta, alpha) = timing;
        let world = placement(actor);
        if !world.is_finite() || world.determinant().abs() < 1e-8 {
            self.previous_center = None;
            return None;
        }
        let inverse = world.inverse();
        let original_roots = [bases[0][cloth.source], bases[1][cloth.source]];
        let root_scales = original_roots
            .map(|root| Vec3::from_slice(&root.axis_scale[..3]) * root.translation_scale[3]);
        if original_roots.iter().any(|root| !root.is_finite())
            || root_scales.iter().any(|scale| scale.min_element() <= 1e-5)
        {
            self.previous_center = None;
            return None;
        }
        let retain = std::mem::take(&mut self.retain_fitting);
        let mut fitting = if retain && self.fitting.is_some() {
            self.fitting
        } else {
            DepthFit::resolve(model, &bases, actor, alpha, colliders)
        };
        if let (Some(old), Some(new)) = (self.fitting, fitting)
            && !old.differs(new)
        {
            // World-to-torso roundoff must not change the fit during ordinary movement.
            fitting = Some(old);
        }
        if match (self.fitting, fitting) {
            (Some(old), Some(new)) => old.differs(new),
            (None, None) => false,
            _ => true,
        } {
            self.reanchor_pending = true;
        }
        self.fitting = fitting;
        let mut poses = bases.map(|base| base.to_vec());
        if let Some(fitting) = fitting {
            for pose in &mut poses {
                fitting.apply(model, pose);
            }
        }
        let roots = [poses[0][cloth.source], poses[1][cloth.source]];
        let sampled =
            crate::actor::sampled_bone_offset([0.0; 3], roots[0], roots[1], actor, alpha)?;
        let transform = Mat4::from_cols(
            Vec3::from_array(sampled.basis[0]).extend(0.0),
            Vec3::from_array(sampled.basis[1]).extend(0.0),
            Vec3::from_array(sampled.basis[2]).extend(0.0),
            Vec3::from_array(sampled.position).extend(1.0),
        );
        let scale = root_scales[0].lerp(root_scales[1], alpha.clamp(0.0, 1.0))
            * Vec3::new(
                world.x_axis.length(),
                world.y_axis.length(),
                world.z_axis.length(),
            );
        let pivot = Vec3::from_array(model.geometry.bone_pivots[cloth.source]);
        let center = world.w_axis.truncate();
        let reset = self.points.len() != cloth.rest.len()
            || self
                .previous_scale
                .is_none_or(|old| !old.abs_diff_eq(scale, 1e-5))
            || self
                .previous_center
                .is_none_or(|p| p.distance(center) > 4.0);
        let pins: Vec<_> = cloth.rest[..=cloth.columns]
            .iter()
            .map(|p| {
                let mut offset = *p - pivot;
                offset.x *= 0.89;
                transform.transform_point3(offset)
            })
            .collect();
        if reset {
            self.points = cloth
                .rest
                .iter()
                .map(|p| transform.transform_point3(*p - pivot))
                .collect();
            self.old = self.points.clone();
            self.normals = vec![Vec3::Z; self.points.len()];
            self.constraints = cloth.constraints.clone();
            self.last_pins = pins.clone();
            self.accumulator = 0.0;
        } else if let Some(previous) = self.previous_center {
            if self.reanchor_pending
                && let Some(old_frame) = self.previous_frame
            {
                // Rebase only the pose-space discontinuity; ordinary avatar travel still has inertia.
                let pose_frame = Mat4::from_translation(previous - center) * transform;
                let rebase = pose_frame * old_frame.inverse();
                for point in self
                    .points
                    .iter_mut()
                    .chain(&mut self.old)
                    .chain(&mut self.last_pins)
                {
                    *point = rebase.transform_point3(*point);
                }
            }
            // Move both Verlet endpoints together so following the avatar adds no artificial energy.
            let carry = (center - previous)
                * Vec3::new(HORIZONTAL_FOLLOW, VERTICAL_FOLLOW, HORIZONTAL_FOLLOW);
            for point in self
                .points
                .iter_mut()
                .chain(&mut self.old)
                .chain(&mut self.last_pins)
            {
                *point += carry;
            }
        }
        self.previous_center = Some(center);
        self.previous_scale = Some(scale);
        self.previous_frame = (transform.determinant().abs() > 1e-8).then_some(transform);
        self.reanchor_pending = false;
        for constraint in &mut self.constraints {
            constraint.length = transform
                .transform_vector3(cloth.rest[constraint.b] - cloth.rest[constraint.a])
                .length();
        }
        let delta = if delta.is_finite() {
            delta.clamp(0.0, 0.05)
        } else {
            0.0
        };
        let anchor_velocity = if delta > 0.0 {
            pins.iter()
                .zip(&self.last_pins)
                .map(|(a, b)| *a - *b)
                .sum::<Vec3>()
                / (pins.len() as f32 * delta)
        } else {
            Vec3::ZERO
        };
        self.accumulator += delta;
        let steps = (self.accumulator / STEP) as usize;
        for substep in 0..steps {
            let alpha = (substep + 1) as f32 / steps as f32;
            let current_pins: Vec<_> = self
                .last_pins
                .iter()
                .zip(&pins)
                .map(|(a, b)| a.lerp(*b, alpha))
                .collect();
            self.step(
                cloth,
                &current_pins,
                colliders,
                world.transform_vector3(Vec3::Z) * 0.35,
                anchor_velocity,
            );
            self.accumulator -= STEP;
        }
        for (i, pin) in pins.iter().enumerate() {
            self.points[i] = *pin;
            self.old[i] = *pin;
        }
        self.last_pins = pins;
        self.update_normals(cloth);
        for (i, point) in self.points.iter().enumerate() {
            let point = inverse.transform_point3(*point);
            let normal = inverse
                .transform_vector3(self.normals[i])
                .normalize_or_zero();
            let pose = RenderBoneTransform {
                translation_scale: [point.x, point.y, point.z, 1.0],
                rotation: if normal.length_squared() > 0.5 {
                    Quat::from_rotation_arc(Vec3::Z, normal).to_array()
                } else {
                    Quat::IDENTITY.to_array()
                },
                axis_scale: UNIT_AXIS_SCALE,
            };
            poses[0][cloth.first + i] = pose;
            poses[1][cloth.first + i] = pose;
        }
        let [previous, current] = poses;
        Some((previous.into(), current.into()))
    }

    /// Compute one smooth normal per welded fabric particle.
    fn update_normals(&mut self, cloth: &ClothRig) {
        self.normals.fill(Vec3::ZERO);
        for &[a, b, c] in &cloth.triangles {
            let n = (self.points[b] - self.points[a]).cross(self.points[c] - self.points[a]);
            for i in [a, b, c] {
                self.normals[i] += n;
            }
        }
        for normal in &mut self.normals {
            *normal = normal.normalize_or_zero();
        }
    }

    /// Integrate gravity and normal airflow, then solve stretch, shear, bend and body contacts.
    fn step(
        &mut self,
        cloth: &ClothRig,
        pins: &[Vec3],
        colliders: &[Collider],
        wind: Vec3,
        anchor_velocity: Vec3,
    ) {
        self.seconds += STEP;
        self.update_normals(cloth);
        for (i, pin) in pins.iter().enumerate() {
            self.points[i] = *pin;
            self.old[i] = *pin;
        }
        for i in cloth.columns + 1..self.points.len() {
            let velocity = (self.points[i] - self.old[i]) / STEP;
            let u = (i % (cloth.columns + 1)) as f32 / cloth.columns as f32;
            let v = (i / (cloth.columns + 1)) as f32 / cloth.rows as f32;
            let air = wind
                + Vec3::new(
                    0.10 * (self.seconds * 2.1 + v * 5.0).sin(),
                    0.04 * (self.seconds * 3.7 + u * 4.0).sin(),
                    0.10 * (self.seconds * 4.3 + u * 7.0 + v * 6.0).sin(),
                );
            let relative = air - velocity;
            let normal_speed = relative.dot(self.normals[i]);
            let pressure = (0.6 * normal_speed * normal_speed.abs()).clamp(-14.0, 14.0);
            let acceleration =
                Vec3::new(0.0, -9.81, 0.0) + self.normals[i] * pressure + relative * 0.16;
            let old = self.points[i];
            let anchor_step = anchor_velocity * STEP;
            let relative_step = old - self.old[i] - anchor_step;
            let damped_step = anchor_step + relative_step * (-SETTLING_DAMPING * STEP).exp();
            self.points[i] =
                old + damped_step * (-WORLD_DRAG * STEP).exp() + acceleration * (STEP * STEP);
            self.old[i] = old;
        }
        for constraint in &mut self.constraints {
            constraint.lambda = 0.0;
        }
        for pass in 0..ITERATIONS {
            for c in &mut self.constraints {
                let wa = if c.a > cloth.columns { 1.0 } else { 0.0 };
                let wb = if c.b > cloth.columns { 1.0 } else { 0.0 };
                if wa + wb == 0.0 {
                    continue;
                }
                let d = self.points[c.b] - self.points[c.a];
                let length = d.length().max(1e-8);
                let compliance = c.compliance / (STEP * STEP);
                let lambda =
                    (-(length - c.length) - compliance * c.lambda) / (wa + wb + compliance);
                c.lambda += lambda;
                let correction = d * (lambda / length);
                self.points[c.a] -= correction * wa;
                self.points[c.b] += correction * wb;
            }
            if pass % 2 == 0 {
                for point in &mut self.points[cloth.columns + 1..] {
                    for collider in colliders {
                        collider.project(point);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
