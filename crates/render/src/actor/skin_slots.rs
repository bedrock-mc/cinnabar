//! Stable player-skin texture slots: one array per native resolution, LRU-recycled.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use render_api::SkinRgba8;
use render_model::MAX_RENDERED_PLAYERS;
#[cfg(test)]
use render_model::{STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE};

use render_model::{PLAYER_SKIN_BUDGET_BYTES, SKIN_CLASS_SIDES};
const SLOT_LAYER_BITS: u32 = 24;
/// Arrays grow past their in-use skins only while total allocation stays under this.
const RETAINED_SKIN_BYTES: usize = PLAYER_SKIN_BUDGET_BYTES / 8;
/// A new array starts at this many bytes, or one layer.
const MIN_CLASS_BYTES: usize = 256 * 1024;

#[must_use]
pub const fn pack_skin_slot(class: usize, layer: usize) -> u32 {
    ((class as u32) << SLOT_LAYER_BITS) | layer as u32
}

#[must_use]
const fn unpack_skin_slot(slot: u32) -> (usize, usize) {
    (
        (slot >> SLOT_LAYER_BITS) as usize,
        (slot & ((1 << SLOT_LAYER_BITS) - 1)) as usize,
    )
}

const fn layer_bytes(class: usize) -> usize {
    SKIN_CLASS_SIDES[class] * SKIN_CLASS_SIDES[class] * 4
}

/// One admitted skin: its source raster, the native texels its class array holds, and an
/// admission number that changes whenever the layer's contents do.
#[derive(Clone, Debug)]
pub struct ResidentSkin {
    pub skin: SkinRgba8,
    pub texels: Arc<[u8]>,
    pub admission: u64,
}

/// Every skin array's layers as the renderer must hold them; layer count is array capacity.
#[derive(Clone, Debug, Default)]
pub struct ActorSkinResidency {
    pub classes: [Arc<[Option<ResidentSkin>]>; 4],
}

impl ActorSkinResidency {
    #[must_use]
    pub fn resident(&self, slot: u32) -> Option<&ResidentSkin> {
        let (class, layer) = unpack_skin_slot(slot);
        self.classes.get(class)?.get(layer)?.as_ref()
    }

    /// Texture bytes the arrays allocate.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        (0..4)
            .map(|class| self.classes[class].len() * layer_bytes(class))
            .sum()
    }
}

#[derive(Debug)]
struct Slot {
    resident: ResidentSkin,
}

#[derive(Debug, Default)]
pub(crate) struct SkinSlots {
    classes: [Vec<Option<Slot>>; 4],
    free: [Vec<usize>; 4],
    recency: [render_model::FrameSlotRecency; SKIN_CLASS_SIDES.len()],
    index: HashMap<SkinRgba8, u32>,
    frame: u64,
    admissions: u64,
    residency: Arc<ActorSkinResidency>,
    dirty: bool,
    assigned: Vec<u32>,
}

impl SkinSlots {
    /// Stable slots for admitted square skins; no source in this frame's bounded set is evicted.
    pub(crate) fn assign(&mut self, skins: &[SkinRgba8]) -> &[u32] {
        debug_assert!(skins.len() <= MAX_RENDERED_PLAYERS);
        self.frame += 1;
        // Mark every resident skin of this frame before admitting any, so none is evicted.
        for skin in skins {
            if let Some(&slot) = self.index.get(skin) {
                self.touch(slot);
            }
        }
        let mut slots = std::mem::take(&mut self.assigned);
        slots.clear();
        for skin in skins {
            let slot = match self.index.get(skin) {
                Some(&slot) => slot,
                None => {
                    let (class, texels) = native_class(skin);
                    let Some(layer) = self.claim(class) else {
                        self.assigned = slots;
                        self.repack(skins);
                        return &self.assigned;
                    };
                    self.admit(skin, class, layer, texels)
                }
            };
            slots.push(slot);
        }
        self.assigned = slots;
        &self.assigned
    }

    /// The slots of the latest [`Self::assign`].
    pub(crate) fn assigned(&self) -> &[u32] {
        &self.assigned
    }

    /// Marks a resident slot without scanning or allocating a recency queue.
    fn touch(&mut self, slot: u32) {
        let (class, layer) = unpack_skin_slot(slot);
        self.recency[class].touch(layer, self.frame);
    }

    /// The residency of the latest assignment; a new `Arc` only when a layer or capacity changed.
    pub(crate) fn residency(&mut self) -> &Arc<ActorSkinResidency> {
        if std::mem::take(&mut self.dirty) {
            self.residency = Arc::new(ActorSkinResidency {
                classes: std::array::from_fn(|class| {
                    self.classes[class]
                        .iter()
                        .map(|slot| slot.as_ref().map(|slot| slot.resident.clone()))
                        .collect()
                }),
            });
        }
        &self.residency
    }

    fn admit(&mut self, skin: &SkinRgba8, class: usize, layer: usize, texels: Arc<[u8]>) -> u32 {
        self.admissions += 1;
        let slot = pack_skin_slot(class, layer);
        self.classes[class][layer] = Some(Slot {
            resident: ResidentSkin {
                skin: skin.clone(),
                texels,
                admission: self.admissions,
            },
        });
        self.recency[class].touch(layer, self.frame);
        self.index.insert(skin.clone(), slot);
        self.dirty = true;
        slot
    }

    fn allocated_bytes(&self) -> usize {
        (0..4)
            .map(|class| self.classes[class].len() * layer_bytes(class))
            .sum()
    }

    /// A free layer in `class`: an unused one, then growth while retention allows, then the
    /// least recently used skin outside this frame, then any growth the budget allows.
    fn claim(&mut self, class: usize) -> Option<usize> {
        if let Some(layer) = self.free[class].pop() {
            return Some(layer);
        }
        let capacity = self.classes[class].len();
        let room = (PLAYER_SKIN_BUDGET_BYTES - self.allocated_bytes()) / layer_bytes(class);
        let grown = (capacity * 2)
            .max((MIN_CLASS_BYTES / layer_bytes(class)).max(1))
            .min(MAX_RENDERED_PLAYERS)
            .min(capacity + room);
        #[cfg(test)]
        tests::record_probe();
        let lru = self.recency[class].oldest_unused(self.frame);
        let retains = self.allocated_bytes() + (grown - capacity.min(grown)) * layer_bytes(class)
            <= RETAINED_SKIN_BYTES;
        match lru {
            Some(layer) if grown <= capacity || !retains => {
                let evicted = self.classes[class][layer]
                    .take()
                    .expect("lru layer is resident");
                self.recency[class].remove(layer);
                self.index.remove(&evicted.resident.skin);
                self.dirty = true;
                Some(layer)
            }
            _ if grown > capacity => {
                self.classes[class].resize_with(grown, || None);
                self.free[class].extend((capacity + 1..grown).rev());
                self.dirty = true;
                Some(capacity)
            }
            _ => None,
        }
    }

    /// Reassigns this frame's skins to exactly sized arrays, dropping every other skin; only
    /// reached when unused capacity in other classes holds the budget.
    fn repack(&mut self, skins: &[SkinRgba8]) {
        let mut seen = HashSet::with_capacity(skins.len());
        let mut unique = Vec::with_capacity(skins.len());
        for skin in skins {
            #[cfg(test)]
            tests::record_probe();
            if !seen.insert(skin.clone()) {
                continue;
            }
            let prepared = self.index.get(skin).and_then(|&slot| {
                let (class, layer) = unpack_skin_slot(slot);
                self.classes[class][layer]
                    .as_ref()
                    .map(|slot| (class, Arc::clone(&slot.resident.texels)))
            });
            let (class, texels) = prepared.unwrap_or_else(|| native_class(skin));
            unique.push((skin.clone(), class, texels));
        }
        *self = Self {
            frame: self.frame,
            admissions: self.admissions,
            dirty: true,
            assigned: std::mem::take(&mut self.assigned),
            ..Self::default()
        };
        for class in 0..4 {
            let count = unique.iter().filter(|entry| entry.1 == class).count();
            self.classes[class].resize_with(count, || None);
        }
        let mut next = [0; 4];
        for (skin, class, texels) in unique {
            self.admit(&skin, class, next[class], texels);
            next[class] += 1;
        }
        self.assigned.clear();
        self.assigned
            .extend(skins.iter().map(|skin| self.index[skin]));
    }
}

/// The smallest class whose nearest upscale reproduces `skin`, and that class's texels.
fn native_class(skin: &SkinRgba8) -> (usize, Arc<[u8]>) {
    let mut side = render_model::actor_skin_side(skin).expect("admitted square skin");
    let mut texels: Option<Vec<u8>> = None;
    while side > SKIN_CLASS_SIDES[1] {
        let Some(half) = halve(texels.as_deref().unwrap_or(skin), side) else {
            break;
        };
        texels = Some(half);
        side /= 2;
    }
    let class = SKIN_CLASS_SIDES
        .iter()
        .position(|&class_side| class_side == side)
        .expect("halving stops at a class side");
    let texels = texels.map_or_else(|| Arc::clone(skin.pixels()), Arc::from);
    (class, texels)
}

/// `texels` at half the side when every 2x2 block is one colour.
fn halve(texels: &[u8], side: usize) -> Option<Vec<u8>> {
    let half = side / 2;
    let texel = |x: usize, y: usize| -> [u8; 4] {
        let at = (y * side + x) * 4;
        texels[at..at + 4].try_into().expect("four bytes")
    };
    let mut out = Vec::with_capacity(half * half * 4);
    for y in (0..side).step_by(2) {
        for x in (0..side).step_by(2) {
            let corner = texel(x, y);
            if texel(x + 1, y) != corner
                || texel(x, y + 1) != corner
                || texel(x + 1, y + 1) != corner
            {
                return None;
            }
            out.extend_from_slice(&corner);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A standard raster that is the nearest upscale of a `side`-texel noise skin.
    fn skin(seed: u64, side: usize) -> SkinRgba8 {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let native: Vec<u8> = (0..side * side * 4)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let scale = STANDARD_SKIN_SIDE / side;
        let mut out = vec![0; STANDARD_SKIN_BYTES];
        for y in 0..STANDARD_SKIN_SIDE {
            for x in 0..STANDARD_SKIN_SIDE {
                let from = ((y / scale) * side + x / scale) * 4;
                let to = (y * STANDARD_SKIN_SIDE + x) * 4;
                out[to..to + 4].copy_from_slice(&native[from..from + 4]);
            }
        }
        out.into()
    }

    #[test]
    fn upscaled_skins_are_stored_at_their_native_side() {
        for (class, side) in SKIN_CLASS_SIDES.iter().copied().enumerate() {
            let standard = skin(side as u64, side);
            let (found, texels) = native_class(&standard);
            assert_eq!((found, texels.len()), (class, side * side * 4));
        }
        let standard = skin(1, STANDARD_SKIN_SIDE);
        assert!(Arc::ptr_eq(&native_class(&standard).1, standard.pixels()));
    }

    #[test]
    fn warm_frames_over_an_unchanged_skin_set_allocate_nothing() {
        let skins = [skin(1, 64), skin(2, 128), skin(3, STANDARD_SKIN_SIDE)];
        let mut slots = SkinSlots::default();
        slots.assign(&skins);
        slots.residency();
        let before = crate::alloc_count::thread_allocations();
        for _ in 0..8 {
            std::hint::black_box(slots.assign(&skins));
            std::hint::black_box(slots.residency());
        }
        assert_eq!(crate::alloc_count::thread_allocations() - before, 0);
    }

    #[test]
    fn a_visible_set_change_keeps_every_resident_slot() {
        let skins = [skin(1, 64), skin(2, 128), skin(3, 256)];
        let mut slots = SkinSlots::default();
        let first = slots.assign(&skins).to_vec();
        let residency = Arc::clone(slots.residency());
        let again = slots.assign(&[skins[2].clone(), skins[0].clone()]).to_vec();
        assert_eq!(again, [first[2], first[0]]);
        assert!(Arc::ptr_eq(slots.residency(), &residency));
    }

    #[test]
    fn a_new_skin_changes_only_its_own_layer() {
        let skins = [skin(1, 64), skin(2, 64)];
        let mut slots = SkinSlots::default();
        slots.assign(&skins);
        let before = Arc::clone(slots.residency());
        let added = slots.assign(&[skins[0].clone(), skins[1].clone(), skin(3, 64)])[2];
        let after = slots.residency();
        let admission = |residency: &ActorSkinResidency, slot| {
            residency.resident(slot).map(|resident| resident.admission)
        };
        for class in 0..4 {
            for layer in 0..after.classes[class].len() {
                let slot = pack_skin_slot(class, layer);
                if slot != added {
                    assert_eq!(
                        admission(after, slot),
                        admission(&before, slot),
                        "{slot:#x}"
                    );
                }
            }
        }
        assert!(admission(&before, added).is_none() && admission(after, added).is_some());
    }

    #[test]
    fn eviction_recycles_only_skins_outside_the_current_frame() {
        // Sixteen standard rasters fill the retention allowance.
        let standard: Vec<_> = (0..17).map(|seed| skin(seed, STANDARD_SKIN_SIDE)).collect();
        let mut slots = SkinSlots::default();
        let first = slots.assign(&standard[..16]).to_vec();
        assert_eq!(slots.residency().classes[0].len(), 16);

        // The new skin takes the one layer this frame does not use.
        let second = slots.assign(&standard[1..17]).to_vec();
        assert_eq!(second[..15], first[1..]);
        assert_eq!(second[15], first[0]);
        assert!(!slots.index.contains_key(&standard[0]));

        // With every layer in use, the array grows instead of evicting.
        let mut all = standard[1..17].to_vec();
        all.push(skin(99, STANDARD_SKIN_SIDE));
        let third = slots.assign(&all).to_vec();
        assert_eq!(third[..16], second[..]);
        assert_eq!(third[16], pack_skin_slot(0, 16));
        assert_eq!(slots.residency().classes[0].len(), 32);
    }

    #[test]
    fn an_exhausted_budget_repacks_without_dropping_this_frames_skins() {
        let mut skins: Vec<_> = (0..MAX_RENDERED_PLAYERS as u64)
            .map(|seed| skin(seed, STANDARD_SKIN_SIDE))
            .collect();
        let mut slots = SkinSlots::default();
        slots.assign(&skins);
        assert_eq!(
            slots.residency().allocated_bytes(),
            PLAYER_SKIN_BUDGET_BYTES
        );
        skins[0] = skin(500, 64);
        let assigned = slots.assign(&skins).to_vec();
        let residency = slots.residency();
        assert!(residency.allocated_bytes() <= PLAYER_SKIN_BUDGET_BYTES);
        for (skin, slot) in skins.iter().zip(assigned) {
            assert_eq!(&residency.resident(slot).unwrap().skin, skin);
        }
    }
    thread_local! {
        static LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Counts one explicit candidate inspection without changing production code.
    pub(super) fn record_probe() {
        LOOKUP_PROBES.with(|count| count.set(count.get() + 1));
    }

    /// Starts an independent work sample on this test thread.
    fn reset_probes() {
        LOOKUP_PROBES.with(|count| count.set(0));
    }

    /// Returns deterministic lookup work rather than elapsed time.
    fn probes() -> usize {
        LOOKUP_PROBES.with(std::cell::Cell::get)
    }

    #[test]
    fn bounded_skin_repack_work_retains_prepared_texels() {
        let skins: Vec<_> = (0..MAX_RENDERED_PLAYERS as u64)
            .map(|seed| skin(seed, 64))
            .collect();
        let mut slots = SkinSlots::default();
        let before = slots.assign(&skins).to_vec();
        let residency = Arc::clone(slots.residency());
        reset_probes();
        slots.repack(&skins);
        assert!(
            probes() <= skins.len() * 4,
            "{} duplicate candidates for {} skins",
            probes(),
            skins.len()
        );
        let after = slots.assigned().to_vec();
        let replaced = slots.residency();
        for (old, new) in before.into_iter().zip(after) {
            assert!(Arc::ptr_eq(
                &residency.resident(old).unwrap().texels,
                &replaced.resident(new).unwrap().texels
            ));
        }
    }

    #[test]
    fn bounded_lookup_work_when_all_previous_skins_are_replaced() {
        let count = MAX_RENDERED_PLAYERS / 4;
        let skins: Vec<_> = (0..count * 2)
            .map(|seed| skin(seed as u64, STANDARD_SKIN_SIDE))
            .collect();
        let mut slots = SkinSlots::default();
        slots.assign(&skins[..count]);
        reset_probes();
        let assigned = slots.assign(&skins[count..]).to_vec();
        assert!(
            probes() <= count * 4,
            "{} eviction candidates for {count} new skins",
            probes()
        );
        let residency = slots.residency();
        for (skin, slot) in skins[count..].iter().zip(assigned) {
            assert_eq!(skin, &residency.resident(slot).unwrap().skin);
        }
    }
    #[test]
    fn native_skin_classes_keep_exact_pixels_and_warm_assignments_allocate_nothing() {
        for (class, side) in SKIN_CLASS_SIDES.iter().copied().enumerate() {
            let pixels: Vec<u8> = (0..side * side)
                .flat_map(|index| [index as u8, (index / side) as u8, 17, 255])
                .collect();
            let skin: SkinRgba8 = pixels.into();
            let mut slots = SkinSlots::default();
            let slot = slots.assign(std::slice::from_ref(&skin))[0];
            let resident = slots.residency().resident(slot).unwrap();
            assert_eq!(unpack_skin_slot(slot).0, class);
            assert!(Arc::ptr_eq(&resident.texels, skin.pixels()));
            let allocations = crate::alloc_count::thread_allocations();
            for _ in 0..3 {
                slots.assign(std::slice::from_ref(&skin));
                slots.residency();
            }
            assert_eq!(crate::alloc_count::thread_allocations(), allocations);
        }
    }
}
