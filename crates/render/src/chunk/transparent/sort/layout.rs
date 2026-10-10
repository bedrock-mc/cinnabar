//! Where each sub-chunk's sorted refs live in a snapshot slot, and how a new sort moves in.
//!
//! Every group keeps its own range of the committed slot. A sort of a changed manifest is
//! planned against that slot: unchanged groups keep their range and refs, a new or
//! re-meshed group is written into free space, and a removed group frees its range. Only
//! the changed groups upload, so one new shore costs its own refs, not the whole slot.
use super::groups::{TransparentGroupInput, sort_group};
use super::state::TransparentAllocationIdentity;
use super::{MAX_TRANSPARENT_DRAW_REFS, PackedTransparentDrawRef, TransparentLiquidPhaseGroup};
use crate::chunk::transparent::face_metric::{FaceOrderClass, TransparentFaceMetric};
use crate::chunk::*;

/// Free space a patched slot may leave beyond its live refs before it is packed afresh.
const MIN_FRAGMENTED_REFS: usize = 65_536;

/// A slot's groups in key order, the class each was sorted for, and its free ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentSnapshotLayout {
    pub(in crate::chunk) groups: Arc<[TransparentLiquidPhaseGroup]>,
    /// Parallel to `groups`; `None` for refs that arrived without a class.
    pub(in crate::chunk) classes: Arc<[Option<FaceOrderClass>]>,
    /// Unused ranges below the slot's length, sorted and coalesced.
    pub(in crate::chunk) free: Arc<[Range<u32>]>,
}

impl TransparentSnapshotLayout {
    /// The layout of refs packed in key order, each group sorted for its class.
    pub(in crate::chunk) fn packed(
        groups: Vec<TransparentLiquidPhaseGroup>,
        classes: Vec<Option<FaceOrderClass>>,
    ) -> Self {
        Self {
            groups: groups.into(),
            classes: classes.into(),
            free: Arc::from([]),
        }
    }

    /// Refs the groups draw, excluding free space.
    pub(in crate::chunk) fn live_refs(&self) -> usize {
        self.groups.iter().map(|group| group.ref_range.len()).sum()
    }
}

/// The committed slot a new sort is planned against.
#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentLayoutBase {
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) allocations: Arc<[TransparentAllocationIdentity]>,
    pub(in crate::chunk) layout: TransparentSnapshotLayout,
}

/// The slot ranges whose refs differ from the base slot's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentRefPatch {
    /// The slot the patch was planned against; it applies only on top of exactly that.
    pub(in crate::chunk) base: Arc<[PackedTransparentDrawRef]>,
    /// Ranges a draw would misread until written: new, moved or re-meshed groups.
    pub(in crate::chunk) urgent: Vec<Range<usize>>,
    /// Ranges whose base refs remain an older but valid order of the same allocation.
    pub(in crate::chunk) deferred: Vec<Range<usize>>,
}

impl TransparentRefPatch {
    /// Refs the urgent ranges write.
    pub(in crate::chunk) fn urgent_refs(&self) -> usize {
        self.urgent.iter().map(ExactSizeIterator::len).sum()
    }
}

/// A sorted slot for one manifest.
#[derive(Debug)]
pub(in crate::chunk) struct TransparentSortOutput {
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) layout: TransparentSnapshotLayout,
    pub(in crate::chunk) patch: Option<TransparentRefPatch>,
    /// Refs sorted for this output rather than kept from the base.
    pub(in crate::chunk) sorted_refs: usize,
}

/// How one manifest group relates to the base slot.
enum Placement {
    /// Same allocation and class: the base range already holds this order.
    Keep(Range<u32>),
    /// Same allocation and length: rewrite the base range with a new order.
    InPlace(Range<u32>, Arc<[PackedTransparentDrawRef]>),
    /// New or changed allocation: needs a range of its own.
    Moved(Arc<[PackedTransparentDrawRef]>),
}

/// The base slot's range, class and allocation for `key`, found by advancing cursors
/// through the base's key-sorted groups and allocations.
struct BaseCursor<'a> {
    base: &'a TransparentLayoutBase,
    group: usize,
    allocation: usize,
}

impl<'a> BaseCursor<'a> {
    /// The base group and allocation for `key`; keys must be visited in ascending order.
    fn find(
        &mut self,
        key: SubChunkKey,
    ) -> (
        Option<(Range<u32>, Option<FaceOrderClass>)>,
        Option<&'a TransparentAllocationIdentity>,
    ) {
        let groups = &self.base.layout.groups;
        while self.group < groups.len() && groups[self.group].key < key {
            self.group += 1;
        }
        let allocations = &self.base.allocations;
        while self.allocation < allocations.len() && allocations[self.allocation].key < key {
            self.allocation += 1;
        }
        let group = groups
            .get(self.group)
            .filter(|group| group.key == key)
            .map(|group| {
                (
                    group.ref_range.clone(),
                    self.base.layout.classes.get(self.group).copied().flatten(),
                )
            });
        let allocation = allocations
            .get(self.allocation)
            .filter(|identity| identity.key == key);
        (group, allocation)
    }
}

/// Sorts `groups`, parallel to the key-sorted `allocations`, for `camera` into a slot.
///
/// Against `base`, groups whose allocation and class are unchanged keep their range and
/// refs; the others are sorted and written in place or into free space, and groups that
/// left the manifest free their ranges, so the patch covers only what changed. Without a
/// base, or when the patch would write more than `upload_cap` urgent refs or fragment the
/// slot well past its live refs, the groups are packed afresh in key order.
pub(in crate::chunk) fn plan_transparent_slot(
    camera: Vec3,
    allocations: &[TransparentAllocationIdentity],
    groups: &[Arc<TransparentGroupInput>],
    base: Option<&TransparentLayoutBase>,
    upload_cap: usize,
) -> TransparentSortOutput {
    debug_assert_eq!(allocations.len(), groups.len());
    let metric = TransparentFaceMetric::new(camera);
    let mut sorted_refs = 0;
    let mut placements = Vec::with_capacity(groups.len());
    let mut released = Vec::new();
    let mut cursor = base.map(|base| BaseCursor {
        base,
        group: 0,
        allocation: 0,
    });
    for (identity, group) in allocations.iter().zip(groups) {
        let class = metric.class(identity.key);
        let (base_group, base_allocation) = cursor
            .as_mut()
            .map_or((None, None), |cursor| cursor.find(identity.key));
        let same_allocation = base_allocation == Some(identity);
        if let Some((range, base_class)) = base_group.clone()
            && same_allocation
            && base_class == Some(class)
        {
            placements.push((class, Placement::Keep(range)));
            continue;
        }
        let refs = sort_group(metric, group);
        sorted_refs += refs.len();
        let placement = match base_group {
            Some((range, _)) if same_allocation && range.len() == refs.len() => {
                Placement::InPlace(range, refs)
            }
            Some((range, _)) => {
                released.push(range);
                Placement::Moved(refs)
            }
            None => Placement::Moved(refs),
        };
        placements.push((class, placement));
    }
    if let Some(base) = base {
        released.extend(
            base.layout
                .groups
                .iter()
                .filter(|group| {
                    allocations
                        .binary_search_by(|identity| identity.key.cmp(&group.key))
                        .is_err()
                })
                .map(|group| group.ref_range.clone()),
        );
        if let Some(output) = patch_slot(
            base,
            allocations,
            &placements,
            released,
            upload_cap,
            sorted_refs,
        ) {
            return output;
        }
    }
    pack_slot(allocations, placements, base, sorted_refs)
}

/// Applies `placements` to a copy of the base slot, or `None` when packing is better.
fn patch_slot(
    base: &TransparentLayoutBase,
    allocations: &[TransparentAllocationIdentity],
    placements: &[(FaceOrderClass, Placement)],
    released: Vec<Range<u32>>,
    upload_cap: usize,
    sorted_refs: usize,
) -> Option<TransparentSortOutput> {
    let mut free = base.layout.free.to_vec();
    let mut used = base.refs.len();
    for range in released {
        release_quad_range(&mut used, &mut free, range);
    }
    let mut ranges = Vec::with_capacity(placements.len());
    for (_, placement) in placements {
        ranges.push(match placement {
            Placement::Keep(range) | Placement::InPlace(range, _) => range.clone(),
            Placement::Moved(refs) => {
                let len = u32::try_from(refs.len()).ok()?;
                let start =
                    allocate_quad_range(&mut used, &mut free, len, MAX_TRANSPARENT_DRAW_REFS)?;
                start..start + len
            }
        });
    }
    let live = ranges.iter().map(ExactSizeIterator::len).sum::<usize>();
    if used > (live * 2).max(live + MIN_FRAGMENTED_REFS) {
        return None;
    }
    // The new slot is one copy of the base; changed groups are then written in place.
    let mut refs = base
        .refs
        .iter()
        .copied()
        .chain(std::iter::repeat(PackedTransparentDrawRef::default()))
        .take(used)
        .collect::<Arc<[_]>>();
    let slot = Arc::get_mut(&mut refs).expect("a newly collected slot has one owner");
    let (mut urgent, mut deferred) = (Vec::new(), Vec::new());
    for ((_, placement), range) in placements.iter().zip(&ranges) {
        let span = range.start as usize..range.end as usize;
        match placement {
            Placement::Keep(_) => {}
            Placement::InPlace(_, sorted) => {
                deferred.extend(
                    changed_ref_spans(&slot[span.clone()], sorted)
                        .into_iter()
                        .map(|changed| span.start + changed.start..span.start + changed.end),
                );
                slot[span].copy_from_slice(sorted);
            }
            Placement::Moved(sorted) => {
                slot[span.clone()].copy_from_slice(sorted);
                if !span.is_empty() {
                    urgent.push(span);
                }
            }
        }
    }
    let patch = TransparentRefPatch {
        base: Arc::clone(&base.refs),
        urgent,
        deferred,
    };
    if patch.urgent_refs() > upload_cap {
        return None;
    }
    let (groups, classes) = layout_entries(allocations, placements, &ranges);
    Some(TransparentSortOutput {
        refs,
        layout: TransparentSnapshotLayout {
            groups: groups.into(),
            classes: classes.into(),
            free: free.into(),
        },
        patch: Some(patch),
        sorted_refs,
    })
}

/// Packs every group in key order, taking kept refs from the base slot.
fn pack_slot(
    allocations: &[TransparentAllocationIdentity],
    placements: Vec<(FaceOrderClass, Placement)>,
    base: Option<&TransparentLayoutBase>,
    sorted_refs: usize,
) -> TransparentSortOutput {
    let mut refs = Vec::new();
    let mut ranges = Vec::with_capacity(placements.len());
    for (_, placement) in &placements {
        let start = refs.len() as u32;
        match placement {
            Placement::Keep(range) => refs.extend_from_slice(
                &base.expect("kept groups come from a base").refs
                    [range.start as usize..range.end as usize],
            ),
            Placement::InPlace(_, sorted) | Placement::Moved(sorted) => {
                refs.extend_from_slice(sorted);
            }
        }
        ranges.push(start..refs.len() as u32);
    }
    let (groups, classes) = layout_entries(allocations, &placements, &ranges);
    TransparentSortOutput {
        refs: refs.into(),
        layout: TransparentSnapshotLayout::packed(groups, classes),
        patch: None,
        sorted_refs,
    }
}

/// Key-ordered groups for every non-empty range, with the class each was sorted for.
fn layout_entries(
    allocations: &[TransparentAllocationIdentity],
    placements: &[(FaceOrderClass, Placement)],
    ranges: &[Range<u32>],
) -> (
    Vec<TransparentLiquidPhaseGroup>,
    Vec<Option<FaceOrderClass>>,
) {
    allocations
        .iter()
        .zip(placements)
        .zip(ranges)
        .filter(|(_, range)| !range.is_empty())
        .map(|((identity, (class, _)), range)| {
            (
                TransparentLiquidPhaseGroup {
                    key: identity.key,
                    ref_range: range.clone(),
                },
                Some(*class),
            )
        })
        .unzip()
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
