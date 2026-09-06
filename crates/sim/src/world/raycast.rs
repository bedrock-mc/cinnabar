use std::collections::{BTreeMap, BTreeSet};

use world::{ChunkCollisionRevision, ChunkKey};

use super::{MAX_COLLISION_QUERY_EXTENT, PaletteWorld, WorldCollisionIdentity, WorldQueryError};
use crate::{Aabb, Vec3};

const HALO_WIDTH: usize = 3;
const HALO_CELLS: usize = HALO_WIDTH * HALO_WIDTH * HALO_WIDTH;
// Normalization plus boundary subtraction/division can separate one exact
// geometric event by several representable values. Eight ULPs covers those
// operations while retaining a scale-relative, finite comparison window.
const SIMULTANEOUS_CROSSING_ULPS: u64 = 8;

/// Authoritative collision-shape intercept for a block interaction ray.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockHit {
    /// Absolute block coordinates in the active dimension.
    pub block_pos: [i32; 3],
    /// `0` down, `1` up, `2` north (-Z), `3` south (+Z), `4` west (-X), `5` east (+X).
    pub face: u8,
    /// Finite block-local intercept coordinates, each clamped to `[0, 1]`.
    pub hit_local: Vec3,
    /// Runtime ID whose authoritative collision shape was intercepted.
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
    runtime_id: u32,
    distance: f64,
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

    /// Finds the nearest collision-shape intercept along a caller-bounded ray.
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
        let direction = validate_ray(origin, direction, max_distance)?;
        let inspection_limit = inspection_limit(direction, max_distance)?;
        let mut state = TraversalState::new(origin, direction)?;
        let mut inspected = BTreeSet::new();
        let mut inspected_count = 0_usize;
        let mut revisions = BTreeMap::new();
        let mut best: Option<Candidate> = None;

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
            )?;

            let next = state.next_crossing();
            if best
                .as_ref()
                .is_some_and(|hit| strictly_precedes(hit.distance, next))
                || next > max_distance
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
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.collision_halo;
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
    ) -> Result<(), WorldQueryError> {
        let chunk = ChunkKey::new(self.dimension, block[0] >> 4, block[2] >> 4);
        let current = self
            .store
            .collision_revision(chunk)
            .ok_or(WorldQueryError::UnloadedChunk(chunk))?;
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

        let offset = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        for runtime_id in self.runtime_ids_at(block)? {
            let physics = self
                .registry
                .physics(runtime_id)
                .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
            for (shape_index, shape) in physics.shapes.iter().copied().enumerate() {
                if shape.min.x == shape.max.x
                    || shape.min.y == shape.max.y
                    || shape.min.z == shape.max.z
                {
                    continue;
                }
                let Some((distance, face, point)) =
                    ray_box(origin, direction, max_distance, shape.translated(offset))
                else {
                    continue;
                };
                let candidate = Candidate {
                    block_pos: block,
                    face,
                    hit_local: Vec3::new(
                        (point.x - offset.x).clamp(0.0, 1.0),
                        (point.y - offset.y).clamp(0.0, 1.0),
                        (point.z - offset.z).clamp(0.0, 1.0),
                    ),
                    runtime_id,
                    distance,
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
    let length = direction.x.hypot(direction.y).hypot(direction.z);
    if !length.is_finite() || length == 0.0 {
        return Err(WorldQueryError::InvalidRayDirection);
    }
    if !max_distance.is_finite() || max_distance <= 0.0 || max_distance > MAX_COLLISION_QUERY_EXTENT
    {
        return Err(WorldQueryError::InvalidRayDistance);
    }
    let normalized = Vec3::new(
        direction.x / length,
        direction.y / length,
        direction.z / length,
    );
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

fn inspection_limit(direction: Vec3, max_distance: f64) -> Result<usize, WorldQueryError> {
    let crossings = [direction.x, direction.y, direction.z]
        .into_iter()
        .try_fold(0_usize, |total, component| {
            let axis = (component.abs() * max_distance).ceil() as usize;
            total.checked_add(axis.checked_add(2)?)
        })
        .ok_or(WorldQueryError::RayInspectionLimitExceeded)?;
    crossings
        // One simultaneous three-axis event has seven supercover cells for
        // three crossings, so three cells per crossing is a safe ceiling.
        .checked_mul(3)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_mul(HALO_CELLS))
        .ok_or(WorldQueryError::RayInspectionLimitExceeded)
}

fn checked_offset(cell: [i32; 3], offset: [i32; 3]) -> Result<[i32; 3], WorldQueryError> {
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
    crossing_order(candidate.distance, previous.distance)
        .then_with(|| candidate.block_pos.cmp(&previous.block_pos))
        .then_with(|| candidate.runtime_id.cmp(&previous.runtime_id))
        .then_with(|| candidate.shape_index.cmp(&previous.shape_index))
        .then_with(|| candidate.face.cmp(&previous.face))
        .is_lt()
}

fn strictly_precedes(left: f64, right: f64) -> bool {
    crossing_order(left, right).is_lt()
}

fn crossing_order(left: f64, right: f64) -> std::cmp::Ordering {
    if simultaneous_crossing(left, right) {
        std::cmp::Ordering::Equal
    } else {
        left.total_cmp(&right)
    }
}

fn simultaneous_crossing(left: f64, right: f64) -> bool {
    if !left.is_finite()
        || !right.is_finite()
        || left.is_sign_negative()
        || right.is_sign_negative()
    {
        return left == right;
    }
    left.to_bits().abs_diff(right.to_bits()) <= SIMULTANEOUS_CROSSING_ULPS
}

fn ray_box(
    origin: Vec3,
    direction: Vec3,
    max_distance: f64,
    bounds: Aabb,
) -> Option<(f64, u8, Vec3)> {
    if strictly_inside(origin, bounds) {
        return Some((0.0, opposite_dominant_face(direction), origin));
    }
    let mut entry = 0.0_f64;
    let mut exit = max_distance;
    let mut face = None;
    for axis in 0..3 {
        let component = direction[axis];
        if component == 0.0 {
            if origin[axis] <= bounds.min[axis] || origin[axis] >= bounds.max[axis] {
                return None;
            }
            continue;
        }
        let (near, far, near_face) = if component > 0.0 {
            (
                (bounds.min[axis] - origin[axis]) / component,
                (bounds.max[axis] - origin[axis]) / component,
                min_face(axis),
            )
        } else {
            (
                (bounds.max[axis] - origin[axis]) / component,
                (bounds.min[axis] - origin[axis]) / component,
                max_face(axis),
            )
        };
        match crossing_order(near, entry) {
            std::cmp::Ordering::Greater => {
                entry = near;
                face = Some(near_face);
            }
            std::cmp::Ordering::Equal => {
                entry = entry.max(near);
                if face.is_none_or(|current| near_face < current) {
                    face = Some(near_face);
                }
            }
            std::cmp::Ordering::Less => {}
        }
        exit = exit.min(far);
        if strictly_precedes(exit, entry) {
            return None;
        }
    }
    if exit <= 0.0 || entry > max_distance {
        return None;
    }
    let distance = entry.max(0.0);
    let face = face.unwrap_or_else(|| surface_face(origin, bounds, direction));
    Some((distance, face, origin + direction * distance))
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

fn surface_face(point: Vec3, bounds: Aabb, direction: Vec3) -> u8 {
    let mut faces = Vec::with_capacity(6);
    for axis in 0..3 {
        if point[axis] == bounds.min[axis] && direction[axis] <= 0.0 {
            faces.push(min_face(axis));
        }
        if point[axis] == bounds.max[axis] && direction[axis] >= 0.0 {
            faces.push(max_face(axis));
        }
    }
    faces
        .into_iter()
        .min()
        .unwrap_or_else(|| opposite_dominant_face(direction))
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

struct TraversalState {
    cell: [i32; 3],
    step: [i32; 3],
    next: [f64; 3],
    delta: [f64; 3],
}

impl TraversalState {
    fn new(origin: Vec3, direction: Vec3) -> Result<Self, WorldQueryError> {
        let cell = [
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            origin.z.floor() as i32,
        ];
        let mut step = [0; 3];
        let mut next = [f64::INFINITY; 3];
        let mut delta = [f64::INFINITY; 3];
        for axis in 0..3 {
            if direction[axis] > 0.0 {
                step[axis] = 1;
                next[axis] = (f64::from(cell[axis] + 1) - origin[axis]) / direction[axis];
                delta[axis] = 1.0 / direction[axis];
            } else if direction[axis] < 0.0 {
                step[axis] = -1;
                next[axis] = (origin[axis] - f64::from(cell[axis])) / -direction[axis];
                delta[axis] = -1.0 / direction[axis];
            }
        }
        Ok(Self {
            cell,
            step,
            next,
            delta,
        })
    }

    fn next_crossing(&self) -> f64 {
        self.next[0].min(self.next[1]).min(self.next[2])
    }

    fn advance(&mut self, crossing: f64) -> Result<Vec<[i32; 3]>, WorldQueryError> {
        let axes = (0..3)
            .filter(|&axis| simultaneous_crossing(self.next[axis], crossing))
            .collect::<Vec<_>>();
        let mut tied_cells = Vec::new();
        let full_mask = (1_usize << axes.len()) - 1;
        for mask in 1..full_mask {
            let mut cell = self.cell;
            for (bit, &axis) in axes.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    cell[axis] = cell[axis]
                        .checked_add(self.step[axis])
                        .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                }
            }
            tied_cells.push(cell);
        }
        for axis in axes {
            self.cell[axis] = self.cell[axis]
                .checked_add(self.step[axis])
                .ok_or(WorldQueryError::CoordinateOutOfRange)?;
            self.next[axis] += self.delta[axis];
        }
        Ok(tied_cells)
    }
}
