//! Server-defined interaction hitboxes carried by the `HITBOX` actor data compound.
use protocol::ActorMetadataValue;
use world::{BlockEntityNbt, NbtCompound, NbtValue};

/// Actor data id of the hitbox compound.
pub const HITBOX_METADATA_KEY: u32 = 118;

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
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(super) boxes: Vec<HitBox>,
    skipped: u64,
}

impl State {
    /// Appends readable boxes and retains previous geometry when an update is unusable.
    pub(super) fn apply(&mut self, value: &ActorMetadataValue) {
        let ActorMetadataValue::Compound(bytes) = value else {
            self.skip();
            return;
        };
        let Some(root) = BlockEntityNbt::decode_prefix(bytes)
            .ok()
            .and_then(|(nbt, _)| nbt.parse())
        else {
            self.skip();
            return;
        };
        let Some(entries) = root.list("Hitboxes") else {
            return;
        };
        for entry in entries {
            if let NbtValue::Compound(entry) = entry
                && let Some(hitbox) = decode_box(entry)
            {
                self.boxes.push(hitbox);
            } else {
                self.skip();
            }
        }
    }

    /// Counts malformed data and limits repeated diagnostics to powers of two.
    fn skip(&mut self) {
        self.skipped = self.skipped.saturating_add(1);
        if self.skipped.is_power_of_two() {
            tracing::warn!(skipped = self.skipped, "Skipping invalid actor hitbox data");
        }
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
