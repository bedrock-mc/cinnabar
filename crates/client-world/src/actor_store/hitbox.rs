//! Server-defined interaction hitboxes carried by the `HITBOX` actor data compound.
use std::sync::Arc;

use world::{BlockEntityNbt, NbtValue};

/// Actor data id of the hitbox compound; an empty compound restores the collision box.
pub const HITBOX_METADATA_KEY: u32 = 118;
/// Boxes retained per actor; further entries are ignored.
const MAX_HITBOXES: usize = 64;

/// One axis-aligned hitbox centred on a pivot that turns with the actor's yaw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HitBox {
    /// Box centre relative to the feet, before yaw rotation.
    pivot: [f32; 3],
    half_extents: [f32; 3],
}

impl HitBox {
    /// World `(min, max)` with the feet at `position`; `(sin, cos)` is the actor's yaw.
    fn world_box(&self, position: [f32; 3], (sin, cos): (f32, f32)) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.pivot;
        let center = [
            position[0] + x * cos - z * sin,
            position[1] + y,
            position[2] + z * cos + x * sin,
        ];
        (
            std::array::from_fn(|axis| center[axis] - self.half_extents[axis]),
            std::array::from_fn(|axis| center[axis] + self.half_extents[axis]),
        )
    }
}

/// Decodes the `Hitboxes` list. `None` when the compound is unreadable or holds no usable box,
/// leaving the actor on its collision box; malformed entries are skipped.
pub(crate) fn parse(bytes: &[u8]) -> Option<Arc<[HitBox]>> {
    let (nbt, _) = BlockEntityNbt::decode_prefix(bytes).ok()?;
    let root = nbt.parse()?;
    let boxes: Vec<_> = root
        .list("Hitboxes")?
        .iter()
        .filter_map(|entry| {
            let NbtValue::Compound(entry) = entry else {
                return None;
            };
            let field = |name| entry.float(name).filter(|value| value.is_finite());
            let min = [field("MinX")?, field("MinY")?, field("MinZ")?];
            let max = [field("MaxX")?, field("MaxY")?, field("MaxZ")?];
            let pivot = [field("PivotX")?, field("PivotY")?, field("PivotZ")?];
            let half_extents: [f32; 3] = std::array::from_fn(|axis| (max[axis] - min[axis]) * 0.5);
            half_extents
                .iter()
                .all(|half| half.is_finite() && *half >= 0.0)
                .then_some(HitBox {
                    pivot,
                    half_extents,
                })
        })
        .take(MAX_HITBOXES)
        .collect();
    (!boxes.is_empty()).then(|| boxes.into())
}

/// World `(min, max)` interaction boxes of one actor: its custom hitboxes, or else its
/// collision box. Yielding allocates nothing.
#[derive(Debug, Clone)]
pub struct ActorHitBoxes<'a> {
    custom: std::slice::Iter<'a, HitBox>,
    position: [f32; 3],
    yaw: (f32, f32),
    fallback: Option<([f32; 3], [f32; 3])>,
}

impl<'a> ActorHitBoxes<'a> {
    pub(super) fn new(
        custom: &'a [HitBox],
        position: [f32; 3],
        yaw_degrees: f32,
        fallback: Option<([f32; 3], [f32; 3])>,
    ) -> Self {
        Self {
            custom: custom.iter(),
            position,
            yaw: yaw_degrees.to_radians().sin_cos(),
            fallback,
        }
    }
}

impl Iterator for ActorHitBoxes<'_> {
    type Item = ([f32; 3], [f32; 3]);

    fn next(&mut self) -> Option<Self::Item> {
        match self.custom.next() {
            Some(hit_box) => Some(hit_box.world_box(self.position, self.yaw)),
            None => self.fallback.take(),
        }
    }
}
