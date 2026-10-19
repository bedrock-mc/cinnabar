//! Shared-vertex cosmetic cloth with fixed-step constraints and world-space inertia.
use super::{HatRig, Item, RenderBoneTransform, rig};
use crate::{ActorRigGeometry, MAX_RENDER_BONES_PER_ACTOR, UNIT_AXIS_SCALE};
use glam::{Mat4, Quat, Vec3, Vec4};
use serde::Deserialize;
use std::sync::Arc;

const STEP: f32 = 1.0 / 120.0;
const ITERATIONS: usize = 9;

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
    /// Build standard-avatar collision proxies from the evaluated head, body, arms and legs.
    pub fn biped(name: &str, bone: RenderBoneTransform, actor: Mat4) -> Option<Self> {
        let (min, max, back_only) = match name.to_ascii_lowercase().as_str() {
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
        Some(Self {
            world,
            inverse: world.inverse(),
            min: Vec3::from_array(min) / 16.0 - Vec3::splat(0.018),
            max: Vec3::from_array(max) / 16.0 + Vec3::splat(0.018),
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
            self.max[axis] + 0.001
        } else {
            self.min[axis] - 0.001
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
}

impl ClothState {
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
        let roots = [bases[0][cloth.source], bases[1][cloth.source]];
        let root_scales =
            roots.map(|root| Vec3::from_slice(&root.axis_scale[..3]) * root.translation_scale[3]);
        if roots.iter().any(|root| !root.is_finite())
            || root_scales.iter().any(|scale| scale.min_element() <= 1e-5)
        {
            self.previous_center = None;
            return None;
        }
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
        }
        self.previous_center = Some(center);
        self.previous_scale = Some(scale);
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
            );
            self.accumulator -= STEP;
        }
        for (i, pin) in pins.iter().enumerate() {
            self.points[i] = *pin;
            self.old[i] = *pin;
        }
        self.last_pins = pins;
        self.update_normals(cloth);
        let mut poses = bases.map(|base| base.to_vec());
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
    fn step(&mut self, cloth: &ClothRig, pins: &[Vec3], colliders: &[Collider], wind: Vec3) {
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
                    0.14 * (self.seconds * 2.1 + v * 5.0).sin(),
                    0.08 * (self.seconds * 3.7 + u * 4.0).sin(),
                    0.18 * (self.seconds * 4.3 + u * 7.0 + v * 6.0).sin(),
                );
            let relative = air - velocity;
            let normal_speed = relative.dot(self.normals[i]);
            let pressure = (0.8 * normal_speed * normal_speed.abs()).clamp(-28.0, 28.0);
            let acceleration =
                Vec3::new(0.0, -9.81, 0.0) + self.normals[i] * pressure + relative * 0.16;
            let old = self.points[i];
            self.points[i] = old + (old - self.old[i]) * 0.984 + acceleration * (STEP * STEP);
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
mod tests {
    use super::*;
    use crate::{ActorRigSurface, ActorRigVertex, EntityRigId};

    /// Admits a small synthetic fabric through the public asset contract.
    fn admitted_fixture() -> Item {
        use sha2::{Digest, Sha256};
        let texture = include_bytes!("bunny_ears.png");
        let image = image::load_from_memory(texture).unwrap();
        let points: Vec<_> = (0..=2)
            .flat_map(|y| (0..=2).map(move |x| [x as f32 * 5.0 - 5.0, 23.8 - y as f32 * 8.5, 3.2]))
            .collect();
        let mut polygons = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                let a = y * 3 + x;
                polygons.push([a, a + 3, a + 4, a + 1].map(|i| [i, 0, i]));
            }
        }
        let geometry=serde_json::to_vec(&serde_json::json!({"format_version":"1.12.0", "minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture.cloth","texture_width":image.width(),"texture_height":image.height()},
            "bones":[{"name":"body","pivot":[0,24,0]}, {"name":"fabric","parent":"body","pivot":[0,23.8,3.2],
            "poly_mesh":{"normalized_uvs":true,"positions":points,"normals":[[0,0,1]],"uvs":vec![[0.0,0.0];9],"polys":polygons}}]
        }]})).unwrap();
        let animation = br#"{"cloth":{"bone":"fabric","columns":2,"rows":2}}"#;
        let asset = |bytes: &[u8]| cosmetics::Asset {
            url: "/fixture".into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len(),
        };
        let mut descriptor = crate::hats::fixture::bunny().descriptor().clone();
        descriptor.id = "fixture:continuous_cloth".into();
        descriptor.slot = "back".into();
        descriptor.attachment_bone = "body".into();
        descriptor.geometry_id = "geometry.fixture.cloth".into();
        descriptor.assets = cosmetics::Assets {
            geometry: asset(&geometry),
            texture: asset(texture),
            animation: Some(asset(animation)),
        };
        crate::hats::admit(descriptor, &geometry, texture, animation).unwrap()
    }

    #[test]
    fn admitted_cloth_expands_all_pose_channels_and_runs_through_the_native_rig() {
        let item = admitted_fixture();
        let model = rig(item);
        assert!(model.has_cloth());
        let root = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: UNIT_AXIS_SCALE,
        };
        let base = model.pose(root, super::super::Motion::default());
        assert_eq!(base.len(), 11);
        let mut state = ClothState::default();
        for _ in 0..30 {
            let (_, pose) = state
                .pose(
                    item,
                    [base.clone(), base.clone()],
                    [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                    ],
                    (1.0 / 20.0, 0.5),
                    &[],
                )
                .unwrap();
            assert!(pose.iter().all(|bone| bone.is_finite()));
        }
    }

    #[test]
    fn scaled_rest_lengths_and_frame_poses_preserve_attachment_endpoints() {
        let item = admitted_fixture();
        let model = rig(item);
        let cloth = model.cloth.as_ref().unwrap();
        let mut state = ClothState::default();
        for scale in [Vec3::splat(0.5), Vec3::splat(2.0), Vec3::new(0.7, 1.3, 0.9)] {
            let root = RenderBoneTransform {
                rotation: Quat::IDENTITY.to_array(),
                translation_scale: [0.0, 1.5, 0.0, 1.0],
                axis_scale: [scale.x, scale.y, scale.z, 1.0],
            };
            let previous = model.pose(root, super::super::Motion::default());
            let mut current = previous.to_vec();
            for bone in &mut current {
                bone.translation_scale[0] += 0.1;
            }
            let current: Arc<[RenderBoneTransform]> = current.into();
            let actor = [
                [1.2, 0.0, 0.0, 2.0],
                [0.0, 0.8, 0.0, 0.0],
                [0.0, 0.0, 1.1, 0.0],
            ];
            for frame in 0..120 {
                let alpha = (frame % 3) as f32 * 0.4;
                let (a, b) = state
                    .pose(
                        item,
                        [previous.clone(), current.clone()],
                        actor,
                        (1.0 / 120.0, alpha),
                        &[],
                    )
                    .unwrap();
                assert_eq!(&a[..cloth.first], &previous[..cloth.first]);
                assert_eq!(&b[..cloth.first], &current[..cloth.first]);
                assert_eq!(&a[cloth.first..], &b[cloth.first..]);
                let pivot = Vec3::from_array(model.geometry.bone_pivots[cloth.source]);
                let mut offset = cloth.rest[0] - pivot;
                offset.x *= 0.89;
                let pin = crate::actor::sampled_bone_offset(
                    offset.to_array(),
                    previous[cloth.source],
                    current[cloth.source],
                    actor,
                    alpha,
                )
                .unwrap();
                assert!(state.points[0].abs_diff_eq(Vec3::from_array(pin.position), 1e-5));
                for c in &state.constraints {
                    let expected =
                        ((cloth.rest[c.b] - cloth.rest[c.a]) * scale * Vec3::new(1.2, 0.8, 1.1))
                            .length();
                    assert!((c.length - expected).abs() < 1e-5);
                }
            }
            let stretch = state
                .constraints
                .iter()
                .filter(|c| c.compliance < 0.001)
                .map(|c| state.points[c.a].distance(state.points[c.b]) / c.length - 1.0)
                .fold(0.0_f32, f32::max);
            assert!(
                stretch < 0.12,
                "scaled fabric lost its dimensions: {stretch}"
            );
        }
    }

    /// A rectangular fabric fixture whose neighbouring triangles repeat their shared corners.
    fn fixture() -> (ClothRig, ActorRigGeometry) {
        let (nx, ny) = (6, 12);
        let points: Vec<_> = (0..=ny)
            .flat_map(|y| {
                (0..=nx).map(move |x| {
                    [
                        -0.3125 + x as f32 * 0.625 / nx as f32,
                        1.4875 - y as f32 * 1.0625 / ny as f32,
                        0.2,
                    ]
                })
            })
            .collect();
        let mut vertices = Vec::new();
        for y in 0..ny {
            for x in 0..nx {
                let a = y * (nx + 1) + x;
                for index in [a, a + nx + 1, a + 1, a + 1, a + nx + 1, a + nx + 2] {
                    vertices.push(ActorRigVertex {
                        position: points[index],
                        normal: [0.0, 0.0, 1.0],
                        uv: [0.0; 2],
                        back_uv: [0.0; 2],
                        bone_index: 1,
                        surface: ActorRigSurface::SINGLE_FACE,
                        glint_origin: [0.0; 2],
                    });
                }
            }
        }
        let mut geometry = ActorRigGeometry::new(
            EntityRigId(0),
            vertices,
            vec![[0.0, 1.5, 0.0], [0.0, 1.4875, 0.2]],
        )
        .unwrap();
        let cloth = ClothRig::build(
            &Definition {
                bone: "fabric".into(),
                columns: nx,
                rows: ny,
            },
            &["body", "fabric"],
            &mut geometry,
            &mut vec![None, Some(0)],
        )
        .unwrap();
        (cloth, geometry)
    }

    #[test]
    fn adjacent_triangles_share_the_same_simulated_particle() {
        let (cloth, geometry) = fixture();
        assert_eq!(cloth.rest.len(), 91);
        for pair in geometry.vertices.as_chunks::<6>().0 {
            assert_eq!(pair[1].bone_index, pair[4].bone_index);
            assert_eq!(pair[2].bone_index, pair[3].bone_index);
        }
    }

    #[test]
    fn fabric_trails_motion_and_survives_run_stop_turn_jump() {
        let (cloth, _) = fixture();
        let mut state = ClothState {
            points: cloth.rest.clone(),
            old: cloth.rest.clone(),
            normals: vec![Vec3::Z; cloth.rest.len()],
            constraints: cloth.constraints.clone(),
            ..Default::default()
        };
        let mut center = Vec3::ZERO;
        let mut trail = 0.0_f32;
        for frame in 0..1800 {
            let t = frame as f32 * STEP;
            let speed = if (1.0..5.0).contains(&t) { 5.6 } else { 0.0 };
            center.z += speed * STEP;
            let yaw = if t > 7.0 {
                (t - 7.0).min(1.0) * std::f32::consts::PI
            } else {
                0.0
            };
            let rotation = Quat::from_rotation_y(yaw);
            let jump = if (9.0..10.0).contains(&t) {
                ((t - 9.0) * std::f32::consts::PI).sin() * 0.6
            } else {
                0.0
            };
            let pins: Vec<_> = cloth.rest[..=cloth.columns]
                .iter()
                .map(|p| center + rotation * (*p) + Vec3::Y * jump)
                .collect();
            state.step(&cloth, &pins, &[], Vec3::ZERO);
            assert!(state.points.iter().all(|p| p.is_finite()));
            assert_eq!(&state.points[..=cloth.columns], pins.as_slice());
            if (3.0..5.0).contains(&t) {
                trail = trail.max(pins[0].z - state.points.last().unwrap().z);
            }
        }
        assert!(
            trail > 0.25,
            "moving attachments must leave the hem trailing: {trail}"
        );
        let max_stretch = cloth
            .constraints
            .iter()
            .filter(|c| c.compliance < 0.001)
            .map(|c| state.points[c.a].distance(state.points[c.b]) / c.length - 1.0)
            .fold(0.0_f32, f32::max);
        assert!(
            max_stretch < 0.12,
            "the settled fabric must retain its dimensions: {max_stretch}"
        );
    }

    #[test]
    fn torso_contact_exits_through_the_back_after_rotation() {
        let bone = RenderBoneTransform {
            rotation: Quat::from_rotation_y(1.2).to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: UNIT_AXIS_SCALE,
        };
        let collider = Collider::biped("body", bone, Mat4::IDENTITY).unwrap();
        let mut point = collider.world.transform_point3(Vec3::new(0.0, -0.4, 0.0));
        collider.project(&mut point);
        assert!(collider.inverse.transform_point3(point).z > collider.max.z);
    }
}
