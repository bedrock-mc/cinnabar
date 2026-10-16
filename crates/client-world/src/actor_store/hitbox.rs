//! Server-defined interaction hitboxes carried by the `HITBOX` actor data compound.
use protocol::ActorMetadataValue;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use world::{BlockEntityNbt, NbtCompound, NbtValue};

/// Client admission policy for appended interaction geometry, independent of packet size.
pub(super) const MAX_HITBOXES_PER_ACTOR: usize = 4_096;
/// Counts retained boxes across live actors and distinct geometry held by older snapshots.
/// This is a geometry-count limit, not an allocator or process-memory limit.
pub(super) const MAX_RETAINED_HITBOXES: usize = 65_536;

/// One axis-aligned interaction box relative to the actor's native position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HitBox {
    pivot: [f32; 3],
    half_extents: [f32; 3],
}

impl HitBox {
    /// Translates the box without applying render scale or rotation.
    fn world_box(&self, position: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let center: [f32; 3] = std::array::from_fn(|axis| position[axis] + self.pivot[axis]);
        (
            std::array::from_fn(|axis| center[axis] - self.half_extents[axis]),
            std::array::from_fn(|axis| center[axis] + self.half_extents[axis]),
        )
    }
}

/// Metadata updates append boxes; empty updates preserve the existing component.
#[derive(Debug)]
pub(crate) struct State {
    pub(super) boxes: Vec<HitBox>,
    permit: Permit,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.boxes == other.boxes
    }
}

/// Store-wide admission and diagnostics; geometry keeps its own charge after removal or reset.
#[derive(Debug)]
pub(super) struct Budget {
    credits: Arc<Credits>,
    max_per_actor: usize,
    skipped: u64,
    refused: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self::with_limits(MAX_HITBOXES_PER_ACTOR, MAX_RETAINED_HITBOXES)
    }
}

impl Budget {
    /// Uses one retained-geometry owner for the entire store lifetime, including resets.
    pub(super) fn with_limits(max_per_actor: usize, max_retained: usize) -> Self {
        Self {
            credits: Arc::new(Credits {
                used: AtomicUsize::new(0),
                limit: max_retained,
            }),
            max_per_actor,
            skipped: 0,
            refused: 0,
        }
    }

    /// Appends readable boxes in order, skipping excess without changing prior geometry.
    /// Returns whether any well-formed boxes were refused by the retained-geometry limits.
    pub(super) fn apply(
        &mut self,
        state: &mut Option<Arc<State>>,
        value: &ActorMetadataValue,
    ) -> bool {
        let ActorMetadataValue::Compound(bytes) = value else {
            self.skip();
            return false;
        };
        let Some(root) = BlockEntityNbt::decode_prefix(bytes)
            .ok()
            .and_then(|(nbt, _)| nbt.parse())
        else {
            self.skip();
            return false;
        };
        let Some(entries) = root.list("Hitboxes") else {
            return false;
        };
        let mut refused = false;
        for entry in entries {
            if let NbtValue::Compound(entry) = entry
                && let Some(hitbox) = decode_box(entry)
            {
                if !self.append(state, hitbox) {
                    self.refuse();
                    refused = true;
                }
            } else {
                self.skip();
            }
        }
        refused
    }

    /// Reserves new boxes, including any snapshot copy, before allocating retained geometry.
    fn append(&self, state: &mut Option<Arc<State>>, hitbox: HitBox) -> bool {
        let len = state.as_ref().map_or(0, |state| state.boxes.len());
        let Some(next_len) = len
            .checked_add(1)
            .filter(|next| *next <= self.max_per_actor)
        else {
            return false;
        };
        if let Some(shared) = state.as_mut()
            && let Some(unique) = Arc::get_mut(shared)
        {
            let Some(permit) = self.credits.reserve(1) else {
                return false;
            };
            unique.boxes.push(hitbox);
            unique.permit.merge(permit);
            return true;
        }
        // A shared snapshot keeps its original allocation and charge. The new allocation
        // must fit alongside it; an unusable or refused update never makes a copy.
        let Some(permit) = self.credits.reserve(next_len) else {
            return false;
        };
        let mut boxes = Vec::with_capacity(next_len);
        if let Some(previous) = state {
            boxes.extend_from_slice(&previous.boxes);
        }
        boxes.push(hitbox);
        *state = Some(Arc::new(State { boxes, permit }));
        true
    }

    /// Counts capacity refusals separately from malformed data, logging at powers of two.
    fn refuse(&mut self) {
        self.refused = self.refused.saturating_add(1);
        if self.refused.is_power_of_two() {
            tracing::warn!(
                refused = self.refused,
                retained = self.credits.used.load(Ordering::Relaxed),
                per_actor_limit = self.max_per_actor,
                retained_limit = self.credits.limit,
                "Skipping actor hitboxes over the retained geometry budget"
            );
        }
    }

    /// Counts malformed data and limits repeated diagnostics to powers of two.
    fn skip(&mut self) {
        self.skipped = self.skipped.saturating_add(1);
        if self.skipped.is_power_of_two() {
            tracing::warn!(skipped = self.skipped, "Skipping invalid actor hitbox data");
        }
    }

    /// Reports geometry charged to live actors and any surviving published snapshots.
    #[cfg(test)]
    pub(super) fn retained(&self) -> usize {
        self.credits.used.load(Ordering::Relaxed)
    }

    /// Reports well-formed boxes refused by either geometry limit.
    #[cfg(test)]
    pub(super) fn refused(&self) -> u64 {
        self.refused
    }
}

#[derive(Debug)]
struct Credits {
    used: AtomicUsize,
    limit: usize,
}

impl Credits {
    /// Atomically admits a checked count; dropping the returned permit releases it once.
    fn reserve(self: &Arc<Self>, boxes: usize) -> Option<Permit> {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(boxes).filter(|next| *next <= self.limit)
            })
            .ok()?;
        Some(Permit {
            owner: Arc::clone(self),
            boxes,
        })
    }
}

#[derive(Debug)]
struct Permit {
    owner: Arc<Credits>,
    boxes: usize,
}

impl Permit {
    /// Transfers an admitted append into the same allocation's existing charge.
    fn merge(&mut self, mut additional: Self) {
        debug_assert!(Arc::ptr_eq(&self.owner, &additional.owner));
        self.boxes += std::mem::take(&mut additional.boxes);
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.owner.used.fetch_sub(self.boxes, Ordering::Relaxed);
    }
}

/// Missing and differently typed fields have the same zero default as an empty compound.
fn decode_box(entry: &NbtCompound) -> Option<HitBox> {
    let field = |name| match entry.get(name) {
        Some(NbtValue::Float(value)) => *value,
        _ => 0.0,
    };
    let min = [field("MinX"), field("MinY"), field("MinZ")];
    let max = [field("MaxX"), field("MaxY"), field("MaxZ")];
    let pivot = [field("PivotX"), field("PivotY"), field("PivotZ")];
    let half_extents = std::array::from_fn(|axis| (max[axis] - min[axis]).abs() * 0.5);
    min.iter()
        .chain(&max)
        .chain(&pivot)
        .chain(&half_extents)
        .all(|value| value.is_finite())
        .then_some(HitBox {
            pivot,
            half_extents,
        })
}

/// World interaction boxes: the custom boxes, or the collision box when none are present.
/// Yielding allocates nothing.
#[derive(Debug, Clone)]
pub struct ActorHitBoxes<'a> {
    custom: std::slice::Iter<'a, HitBox>,
    position: [f32; 3],
    fallback: Option<([f32; 3], [f32; 3])>,
}

impl<'a> ActorHitBoxes<'a> {
    /// Borrows custom geometry and a collision fallback at the requested position.
    pub(super) fn new(
        custom: &'a [HitBox],
        position: [f32; 3],
        fallback: Option<([f32; 3], [f32; 3])>,
    ) -> Self {
        Self {
            custom: custom.iter(),
            position,
            fallback,
        }
    }
}

impl Iterator for ActorHitBoxes<'_> {
    type Item = ([f32; 3], [f32; 3]);

    fn next(&mut self) -> Option<Self::Item> {
        match self.custom.next() {
            Some(hit_box) => Some(hit_box.world_box(self.position)),
            None => self.fallback.take(),
        }
    }
}
