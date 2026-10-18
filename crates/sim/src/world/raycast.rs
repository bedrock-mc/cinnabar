use std::collections::{BTreeMap, BTreeSet};

use world::{ChunkCollisionRevision, ChunkKey};

use super::{
    MAX_COLLISION_QUERY_EXTENT, PaletteWorld, WorldCollisionIdentity, WorldQueryError,
    section::SectionCache,
};
use crate::{Aabb, Vec3};

mod camera;
mod placement;
mod precision;
pub use camera::CameraBlockHit;
pub use placement::{BLOCK_USE_SUPPORT_DEPTH, BLOCK_USE_SUPPORT_MAX_Y};
use precision::FloatRay;

// Normalization plus boundary subtraction/division can separate one exact
// geometric event by several representable values. Eight ULPs covers those
// operations while retaining a scale-relative, finite comparison window.
const SIMULTANEOUS_CROSSING_ULPS: u32 = 8;

/// Authoritative selection-shape intercept for a block interaction ray.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockHit {
    /// Absolute block coordinates in the active dimension.
    pub block_pos: [i32; 3],
    /// `0` down, `1` up, `2` north (-Z), `3` south (+Z), `4` west (-X), `5` east (+X).
    pub face: u8,
    /// Finite block-local intercept coordinates, each clamped to `[0, 1]`.
    pub hit_local: Vec3,
    /// Unclamped world-space intercept, including selection geometry outside the block cell.
    pub hit_world: Vec3,
    /// Runtime ID whose authoritative selection shape was intercepted.
    pub runtime_id: u32,
    /// Finite physical distance from the origin along the normalized ray.
    pub distance: f64,
    /// Exact registry and inspected-column revisions that authorize the hit.
    pub identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone)]
struct Candidate {
    block_pos: [i32; 3],
    face: u8,
    hit_local: Vec3,
    hit_world: Vec3,
    runtime_id: u32,
    distance: f64,
    fraction: f32,
    shape_index: usize,
}

impl PaletteWorld<'_> {
    /// Finds the nearest hit and freezes the exact registry and column
    /// revisions inspected through that hit.
    pub fn block_interaction_ray_current(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
    ) -> Result<Option<BlockHit>, WorldQueryError> {
        self.block_interaction_ray_with_identity(origin, direction, max_distance, None)
    }

    /// Finds the nearest selection-shape intercept along a caller-bounded ray.
    ///
    /// `expected_identity` must cover every column whose collision data could
    /// occlude the result. Missing or changed data fails closed.
    pub fn block_interaction_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        expected_identity: &WorldCollisionIdentity,
    ) -> Result<Option<BlockHit>, WorldQueryError> {
        if expected_identity.registry != self.registry.identity() {
            return Err(WorldQueryError::RegistryIdentityMismatch);
        }
        self.block_interaction_ray_with_identity(
            origin,
            direction,
            max_distance,
            Some(expected_identity),
        )
    }

    fn block_interaction_ray_with_identity(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        expected_identity: Option<&WorldCollisionIdentity>,
    ) -> Result<Option<BlockHit>, WorldQueryError> {
        let origin = origin.rounded();
        let max_distance = f64::from(max_distance as f32);
        let direction = validate_ray(origin, direction, max_distance)?;
        let halo = self.registry.selection_ray_halo();
        let inspection_limit =
            inspection_limit(FloatRay::new(origin, direction, max_distance), halo)?;
        let mut state = TraversalState::new(origin, direction, max_distance)?;
        let mut inspected = BTreeSet::new();
        let mut inspected_count = 0_usize;
        let mut revisions = BTreeMap::new();
        let mut best: Option<Candidate> = None;
        let mut section = SectionCache::default();

        loop {
            self.inspect_halo(
                state.cell,
                origin,
                direction,
                max_distance,
                expected_identity,
                &mut inspected,
                &mut inspected_count,
                inspection_limit,
                &mut revisions,
                &mut best,
                &mut section,
            )?;

            let next = state.next_crossing();
            if best
                .as_ref()
                .is_some_and(|hit| crossing_order(hit.fraction, next).is_lt())
                || next > 1.0
            {
                break;
            }
            for tied_cell in state.advance(next)? {
                self.inspect_halo(
                    tied_cell,
                    origin,
                    direction,
                    max_distance,
                    expected_identity,
                    &mut inspected,
                    &mut inspected_count,
                    inspection_limit,
                    &mut revisions,
                    &mut best,
                    &mut section,
                )?;
            }
        }

        let Some(best) = best else {
            return Ok(None);
        };
        let identity =
            WorldCollisionIdentity::new(self.registry.identity(), revisions.into_values())?;
        Ok(Some(BlockHit {
            block_pos: best.block_pos,
            face: best.face,
            hit_local: best.hit_local,
            hit_world: best.hit_world,
            runtime_id: best.runtime_id,
            distance: best.distance,
            identity,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_halo(
        &self,
        cell: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        expected: Option<&WorldCollisionIdentity>,
        inspected: &mut BTreeSet<[i32; 3]>,
        inspected_count: &mut usize,
        inspection_limit: usize,
        revisions: &mut BTreeMap<ChunkKey, ChunkCollisionRevision>,
        best: &mut Option<Candidate>,
        section: &mut SectionCache,
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.selection_ray_halo();
        for x in halo[0].0..=halo[0].1 {
            for y in halo[1].0..=halo[1].1 {
                for z in halo[2].0..=halo[2].1 {
                    let block = checked_offset(cell, [x, y, z])?;
                    if inspected.insert(block) {
                        *inspected_count = inspected_count
                            .checked_add(1)
                            .ok_or(WorldQueryError::RayInspectionLimitExceeded)?;
                        if *inspected_count > inspection_limit {
                            return Err(WorldQueryError::RayInspectionLimitExceeded);
                        }
                        self.inspect_block(
                            block,
                            origin,
                            direction,
                            max_distance,
                            expected,
                            revisions,
                            best,
                            section,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_block(
        &self,
        block: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        expected: Option<&WorldCollisionIdentity>,
        revisions: &mut BTreeMap<ChunkKey, ChunkCollisionRevision>,
        best: &mut Option<Candidate>,
        section: &mut SectionCache,
    ) -> Result<(), WorldQueryError> {
        let chunk = ChunkKey::new(self.dimension, block[0] >> 4, block[2] >> 4);
        self.inspect_pick_revision(chunk, expected, revisions)?;

        let offset = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        for runtime_id in self.layers_at(block, section)?.iter() {
            let physics = self
                .registry
                .physics(runtime_id)
                .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
            if !self.may_pick(physics) {
                continue;
            }
            if let Some(owner) = self.actor_selection_owner(block, physics)? {
                self.inspect_pick_revision(
                    ChunkKey::new(self.dimension, owner[0] >> 4, owner[2] >> 4),
                    expected,
                    revisions,
                )?;
            }
            let pickable = match physics.pick_shapes.as_deref() {
                Some(_) => self.selection_shapes_with_lookup(block, physics, |neighbour| {
                    self.inspect_pick_revision(
                        ChunkKey::new(self.dimension, neighbour[0] >> 4, neighbour[2] >> 4),
                        expected,
                        revisions,
                    )?;
                    self.primary_runtime_id(neighbour)
                })?,
                None => self.block_collision_shapes(
                    block,
                    physics,
                    Aabb::new(offset, offset + Vec3::ONE),
                )?,
            };
            for (shape_index, shape) in pickable.iter().copied().enumerate() {
                if shape.min.x == shape.max.x
                    || shape.min.y == shape.max.y
                    || shape.min.z == shape.max.z
                {
                    continue;
                }
                let Some((fraction, distance, face, point)) = ray_union_box(
                    origin,
                    direction,
                    max_distance,
                    shape.translated(physics.shape_offset(block)),
                    &pickable,
                    physics.shape_offset(block),
                ) else {
                    continue;
                };
                let local = point - offset;
                let candidate = Candidate {
                    block_pos: block,
                    face,
                    hit_local: Vec3::new(
                        local.x.clamp(0.0, 1.0),
                        local.y.clamp(0.0, 1.0),
                        local.z.clamp(0.0, 1.0),
                    ),
                    hit_world: point,
                    runtime_id,
                    distance,
                    fraction,
                    shape_index,
                };
                if best
                    .as_ref()
                    .is_none_or(|previous| candidate_precedes(&candidate, previous))
                {
                    *best = Some(candidate);
                }
            }
        }
        Ok(())
    }

    /// Records and checks every column supplying palette or actor geometry to the ray.
    fn inspect_pick_revision(
        &self,
        chunk: ChunkKey,
        expected: Option<&WorldCollisionIdentity>,
        revisions: &mut BTreeMap<ChunkKey, ChunkCollisionRevision>,
    ) -> Result<(), WorldQueryError> {
        // The store is immutable for the whole ray, so a recorded column already passed.
        if revisions.contains_key(&chunk) {
            return Ok(());
        }
        let current = self.record_column_read(
            self.store
                .collision_revision(chunk)
                .ok_or(WorldQueryError::UnloadedChunk(chunk)),
        )?;
        if let Some(expected) = expected {
            let expected_revision = expected
                .chunks
                .binary_search_by_key(&chunk, |revision| revision.chunk)
                .ok()
                .map(|index| expected.chunks[index]);
            if expected_revision != Some(current) {
                return Err(WorldQueryError::StaleCollisionIdentity { chunk });
            }
        }
        revisions.insert(chunk, current);
        Ok(())
    }
}

fn validate_ray(origin: Vec3, direction: Vec3, max_distance: f64) -> Result<Vec3, WorldQueryError> {
    let coordinate_min = f64::from(i32::MIN) + 2.0;
    let coordinate_max = f64::from(i32::MAX) - 2.0;
    if !origin.is_finite()
        || [origin.x, origin.y, origin.z]
            .into_iter()
            .any(|value| value < coordinate_min || value > coordinate_max)
    {
        return Err(WorldQueryError::InvalidRayOrigin);
    }
    if !direction.is_finite() {
        return Err(WorldQueryError::InvalidRayDirection);
    }
    if !max_distance.is_finite() || max_distance <= 0.0 || max_distance > MAX_COLLISION_QUERY_EXTENT
    {
        return Err(WorldQueryError::InvalidRayDistance);
    }
    let normalized =
        precision::normalized(direction).ok_or(WorldQueryError::InvalidRayDirection)?;
    let endpoint = origin + normalized * max_distance;
    if !endpoint.is_finite()
        || [endpoint.x, endpoint.y, endpoint.z]
            .into_iter()
            .any(|value| value < coordinate_min || value > coordinate_max)
    {
        return Err(WorldQueryError::InvalidRayOrigin);
    }
    Ok(normalized)
}

/// Bounds inspections by supercover traversal length and the registered owner-cell reach.
fn inspection_limit(ray: FloatRay, halo: [(i32, i32); 3]) -> Result<usize, WorldQueryError> {
    let halo_cells = halo
        .into_iter()
        .try_fold(1_usize, |count, (min, max)| {
            count.checked_mul(usize::try_from(i64::from(max) - i64::from(min) + 1).ok()?)
        })
        .ok_or(WorldQueryError::RayInspectionLimitExceeded)?;
    let crossings = ray
        .delta
        .into_iter()
        .try_fold(0_usize, |total, component| {
            let axis = component.abs().ceil() as usize;
            total.checked_add(axis.checked_add(2)?)
        })
        .ok_or(WorldQueryError::RayInspectionLimitExceeded)?;
    crossings
        // One simultaneous three-axis event has seven supercover cells for
        // three crossings, so three cells per crossing is a safe ceiling.
        .checked_mul(3)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_mul(halo_cells))
        .ok_or(WorldQueryError::RayInspectionLimitExceeded)
}

pub(super) fn checked_offset(
    cell: [i32; 3],
    offset: [i32; 3],
) -> Result<[i32; 3], WorldQueryError> {
    Ok([
        cell[0]
            .checked_add(offset[0])
            .ok_or(WorldQueryError::CoordinateOutOfRange)?,
        cell[1]
            .checked_add(offset[1])
            .ok_or(WorldQueryError::CoordinateOutOfRange)?,
        cell[2]
            .checked_add(offset[2])
            .ok_or(WorldQueryError::CoordinateOutOfRange)?,
    ])
}

fn candidate_precedes(candidate: &Candidate, previous: &Candidate) -> bool {
    crossing_order(candidate.fraction, previous.fraction)
        .then_with(|| candidate.block_pos.cmp(&previous.block_pos))
        .then_with(|| candidate.runtime_id.cmp(&previous.runtime_id))
        .then_with(|| candidate.shape_index.cmp(&previous.shape_index))
        .then_with(|| candidate.face.cmp(&previous.face))
        .is_lt()
}

pub(super) fn strictly_precedes(left: f64, right: f64) -> bool {
    crossing_order(left, right).is_lt()
}

fn crossing_order(left: impl Into<f64>, right: impl Into<f64>) -> std::cmp::Ordering {
    let left = left.into() as f32;
    let right = right.into() as f32;
    if simultaneous_crossing(left, right) {
        std::cmp::Ordering::Equal
    } else {
        left.total_cmp(&right)
    }
}

fn simultaneous_crossing(left: f32, right: f32) -> bool {
    if !left.is_finite()
        || !right.is_finite()
        || left.is_sign_negative()
        || right.is_sign_negative()
    {
        return left == right;
    }
    left.to_bits().abs_diff(right.to_bits()) <= SIMULTANEOUS_CROSSING_ULPS
}

/// Clips a single box while rejecting rays parallel to an exposed face.
fn ray_box(
    origin: Vec3,
    direction: Vec3,
    max_distance: f64,
    bounds: Aabb,
) -> Option<(f32, f64, u8, Vec3)> {
    ray_union_box(
        origin,
        direction,
        max_distance,
        bounds,
        std::slice::from_ref(&bounds),
        Vec3::ZERO,
    )
}

/// Clips one piece, admitting parallel faces only where the surrounding union is solid.
fn ray_union_box(
    origin: Vec3,
    direction: Vec3,
    max_distance: f64,
    bounds: Aabb,
    pieces: &[Aabb],
    offset: Vec3,
) -> Option<(f32, f64, u8, Vec3)> {
    let bounds = Aabb::new(bounds.min.rounded(), bounds.max.rounded());
    let ray = FloatRay::new(origin, direction, max_distance);
    let origin = ray.point(0.0);
    let low = [
        bounds.min.x as f32,
        bounds.min.y as f32,
        bounds.min.z as f32,
    ];
    let high = [
        bounds.max.x as f32,
        bounds.max.y as f32,
        bounds.max.z as f32,
    ];
    if strictly_inside(origin, bounds) {
        return Some((0.0, 0.0, opposite_dominant_face(direction), origin));
    }
    let mut entry = 0.0_f32;
    let mut exit = 1.0_f32;
    let mut face = None;
    let mut entry_faces = 0_u8;
    let mut parallel_face = false;
    for axis in 0..3 {
        let component = ray.delta[axis];
        if component == 0.0 {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return None;
            }
            parallel_face |= origin[axis] == bounds.min[axis] || origin[axis] == bounds.max[axis];
            continue;
        }
        let (near, far, near_face) = if component > 0.0 {
            (
                (low[axis] - ray.origin[axis]) / component,
                (high[axis] - ray.origin[axis]) / component,
                min_face(axis),
            )
        } else {
            (
                (high[axis] - ray.origin[axis]) / component,
                (low[axis] - ray.origin[axis]) / component,
                max_face(axis),
            )
        };
        match crossing_order(near, entry) {
            std::cmp::Ordering::Greater => {
                entry = near;
                face = Some(near_face);
                entry_faces = 1 << near_face;
            }
            std::cmp::Ordering::Equal => {
                entry = entry.max(near);
                entry_faces |= 1 << near_face;
                if face.is_none_or(|current| near_face < current) {
                    face = Some(near_face);
                }
            }
            std::cmp::Ordering::Less => {}
        }
        exit = exit.min(far);
        if crossing_order(exit, entry).is_lt() {
            return None;
        }
    }
    if exit <= 0.0 || entry > 1.0 {
        return None;
    }
    let point = ray.point(entry.max(0.0));
    let distance = ray.distance(point);
    let clipped = std::array::from_fn::<_, 3, _>(|axis| {
        f64::from((point[axis] as f32).clamp(low[axis], high[axis]))
    });
    let geometric_point = Vec3::new(clipped[0], clipped[1], clipped[2]);
    if distance == 0.0 && union_covers_parallel_neighbourhood(origin, Vec3::ZERO, pieces, offset) {
        return Some((0.0, 0.0, opposite_dominant_face(direction), origin));
    }
    let delta = Vec3::new(
        f64::from(ray.delta[0]),
        f64::from(ray.delta[1]),
        f64::from(ray.delta[2]),
    );
    if parallel_face && !union_covers_parallel_neighbourhood(geometric_point, delta, pieces, offset)
    {
        return None;
    }
    let face = if pieces.len() > 1 && entry_faces.count_ones() > 1 {
        exposed_entry_face(
            geometric_point,
            bounds,
            direction,
            pieces,
            offset,
            entry_faces,
        )
        .or(face)
    } else {
        face
    };
    let face = match face {
        Some(face) => face,
        None if parallel_face => opposite_dominant_face(direction),
        None => surface_face(origin, bounds, direction),
    };
    Some((entry.max(0.0), distance, face, point))
}

/// Prefers an exposed entry face when a ray also crosses an internal piece plane.
fn exposed_entry_face(
    point: Vec3,
    bounds: Aabb,
    direction: Vec3,
    pieces: &[Aabb],
    offset: Vec3,
    entry_faces: u8,
) -> Option<u8> {
    let mut exposed = None;
    for axis in 0..3 {
        if direction[axis] == 0.0 {
            continue;
        }
        let (face, surface) = if direction[axis] > 0.0 {
            (min_face(axis), bounds.min[axis])
        } else {
            (max_face(axis), bounds.max[axis])
        };
        if entry_faces & (1 << face) == 0 {
            continue;
        }
        let mut face_point = point;
        face_point[axis] = surface;
        let mut normal_neighbourhood = Vec3::ONE;
        normal_neighbourhood[axis] = 0.0;
        if !union_covers_parallel_neighbourhood(face_point, normal_neighbourhood, pieces, offset) {
            exposed = Some(exposed.map_or(face, |current: u8| current.min(face)));
        }
    }
    exposed
}

/// Checks transverse quadrants exactly; a zero direction checks every volume octant.
/// Exposed edges stay outside the union without adding an epsilon.
fn union_covers_parallel_neighbourhood(
    point: Vec3,
    direction: Vec3,
    pieces: &[Aabb],
    offset: Vec3,
) -> bool {
    (0..8)
        .filter(|bits| (0..3).all(|axis| direction[axis] == 0.0 || bits & (1 << axis) == 0))
        .all(|bits| {
            pieces.iter().any(|piece| {
                let piece = piece.translated(offset);
                (0..3).all(|axis| piece.min[axis] < piece.max[axis])
                    && (0..3).all(|axis| {
                        if direction[axis] != 0.0 {
                            piece.min[axis] <= point[axis] && point[axis] <= piece.max[axis]
                        } else if bits & (1 << axis) == 0 {
                            piece.min[axis] < point[axis] && point[axis] <= piece.max[axis]
                        } else {
                            piece.min[axis] <= point[axis] && point[axis] < piece.max[axis]
                        }
                    })
            })
        })
}

fn strictly_inside(point: Vec3, bounds: Aabb) -> bool {
    point.x > bounds.min.x
        && point.x < bounds.max.x
        && point.y > bounds.min.y
        && point.y < bounds.max.y
        && point.z > bounds.min.z
        && point.z < bounds.max.z
}

fn opposite_dominant_face(direction: Vec3) -> u8 {
    let components = [direction.x.abs(), direction.y.abs(), direction.z.abs()];
    let axis = (0..3)
        .max_by(|left, right| components[*left].total_cmp(&components[*right]))
        .expect("three axes are present");
    if direction[axis] >= 0.0 {
        min_face(axis)
    } else {
        max_face(axis)
    }
}

/// Chooses the lowest numbered outward face without allocating candidate storage.
fn surface_face(point: Vec3, bounds: Aabb, direction: Vec3) -> u8 {
    let mut face = None;
    for axis in 0..3 {
        for (on_surface, candidate) in [
            (
                point[axis] == bounds.min[axis] && direction[axis] <= 0.0,
                min_face(axis),
            ),
            (
                point[axis] == bounds.max[axis] && direction[axis] >= 0.0,
                max_face(axis),
            ),
        ] {
            if on_surface {
                face = Some(face.map_or(candidate, |current: u8| current.min(candidate)));
            }
        }
    }
    face.unwrap_or_else(|| opposite_dominant_face(direction))
}

const fn min_face(axis: usize) -> u8 {
    match axis {
        0 => 4,
        1 => 0,
        2 => 2,
        _ => unreachable!(),
    }
}

const fn max_face(axis: usize) -> u8 {
    match axis {
        0 => 5,
        1 => 1,
        2 => 3,
        _ => unreachable!(),
    }
}

pub(super) struct TraversalState {
    pub(super) cell: [i32; 3],
    step: [i32; 3],
    next: [f32; 3],
    ray: FloatRay,
}

impl TraversalState {
    /// Starts traversal of the same rounded segment used by shape clipping.
    pub(super) fn new(origin: Vec3, direction: Vec3, reach: f64) -> Result<Self, WorldQueryError> {
        let ray = FloatRay::new(origin, direction, reach);
        let cell = [
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            origin.z.floor() as i32,
        ];
        let step = ray.delta.map(|component| {
            if component > 0.0 {
                1
            } else if component < 0.0 {
                -1
            } else {
                0
            }
        });
        let mut state = Self {
            cell,
            step,
            next: [f32::INFINITY; 3],
            ray,
        };
        for axis in 0..3 {
            state.next[axis] = state.axis_crossing(axis)?;
        }
        Ok(state)
    }

    /// Computes a boundary fraction from its integer cell, without accumulating rounding error.
    fn axis_crossing(&self, axis: usize) -> Result<f32, WorldQueryError> {
        let boundary = match self.step[axis] {
            1 => self.cell[axis]
                .checked_add(1)
                .ok_or(WorldQueryError::CoordinateOutOfRange)?,
            -1 => self.cell[axis],
            _ => return Ok(f32::INFINITY),
        };
        Ok((boundary as f32 - self.ray.origin[axis]) / self.ray.delta[axis])
    }

    /// Steps one axis and recomputes its next boundary on the original rounded segment.
    fn advance_axis(&mut self, axis: usize) -> Result<(), WorldQueryError> {
        self.cell[axis] = self.cell[axis]
            .checked_add(self.step[axis])
            .ok_or(WorldQueryError::CoordinateOutOfRange)?;
        self.next[axis] = self.axis_crossing(axis)?;
        Ok(())
    }

    /// Fraction of the next boundary along the rounded segment.
    pub(super) fn next_crossing(&self) -> f32 {
        self.next[0].min(self.next[1]).min(self.next[2])
    }

    /// Returns the at most six partial cells of a simultaneous boundary crossing.
    pub(super) fn advance(
        &mut self,
        crossing: f32,
    ) -> Result<impl Iterator<Item = [i32; 3]>, WorldQueryError> {
        let mut axes = [0; 3];
        let mut count = 0;
        for axis in 0..3 {
            if simultaneous_crossing(self.next[axis], crossing) {
                axes[count] = axis;
                count += 1;
            }
        }
        let mut tied_cells = [[0; 3]; 6];
        let full_mask = (1_usize << count) - 1;
        let mut length = 0;
        for mask in 1..full_mask {
            let mut cell = self.cell;
            for (bit, &axis) in axes[..count].iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    cell[axis] = cell[axis]
                        .checked_add(self.step[axis])
                        .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                }
            }
            tied_cells[length] = cell;
            length += 1;
        }
        for &axis in &axes[..count] {
            self.advance_axis(axis)?;
        }
        Ok(tied_cells.into_iter().take(length))
    }
}
