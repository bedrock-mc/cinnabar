//! How much ordering a section's translucent quads need, judged at mesh time.
//!
//! Each face's translucent quads are emitted from the far side to the near side as seen
//! from in front of them, main list first; a border list lies on the section face, the
//! nearest plane of its direction, so it is drawn after the main list. Quads of one
//! direction never need another order, and quads of opposite directions are never both
//! seen overlapping: each is seen only from its own side of its plane. Directions on
//! different axes can overlap, so their order is decided from their extents.

use crate::{
    geometry::Face,
    mesh::{
        Bucket, GROUP_COUNT, MeshList, group_index,
        quad::{Quad, face_bounds},
    },
};

/// Ordering a section's translucent quads need within the section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranslucentClass {
    /// No translucent quads.
    None,
    /// Every translucent quad faces one direction (coplanar water tops are the common
    /// case): drawing them as emitted is back to front from every view.
    CoplanarOrOneNormal,
    /// Directions drawn in this order, each as emitted, are back to front from every view.
    /// Directions without quads are listed last.
    StaticPerNormal { order: [Face; 6] },
    /// No view-independent order exists; sort per view.
    General,
}

/// Extents of one direction's translucent quads in section-local corner coordinates.
#[derive(Clone, Copy)]
struct Extent {
    min: [usize; 3],
    max: [usize; 3],
}

/// Classifies the translucent quads of every list of `groups`.
pub(super) fn classify(groups: &[Vec<Quad>; GROUP_COUNT]) -> TranslucentClass {
    let extents = Face::ALL.map(|face| {
        MeshList::ALL
            .iter()
            .filter_map(|&list| {
                face_bounds(face, &groups[group_index(list, Bucket::Translucent, face)])
            })
            .reduce(|(min, max), (other_min, other_max)| {
                (
                    [0, 1, 2].map(|axis| min[axis].min(other_min[axis])),
                    [0, 1, 2].map(|axis| max[axis].max(other_max[axis])),
                )
            })
            .map(|(min, max)| Extent { min, max })
    });
    let present: Vec<Face> = Face::ALL
        .into_iter()
        .filter(|face| extents[face.index()].is_some())
        .collect();
    match present.len() {
        0 => return TranslucentClass::None,
        1 => return TranslucentClass::CoplanarOrOneNormal,
        _ => {}
    }
    let extent = |face: Face| extents[face.index()].expect("present");
    // before[a] holds every direction that must be drawn before direction a.
    let mut before = [0_u8; 6];
    for &a in &present {
        for &b in &present {
            if may_cover(a, extent(a), b, extent(b)) {
                before[a.index()] |= 1 << b.index();
            }
        }
    }
    let mut order = Vec::with_capacity(6);
    let mut placed = 0_u8;
    while order.len() < present.len() {
        let Some(&next) = present
            .iter()
            .find(|face| placed & 1 << face.index() == 0 && before[face.index()] & !placed == 0)
        else {
            return TranslucentClass::General;
        };
        placed |= 1 << next.index();
        order.push(next);
    }
    order.extend(Face::ALL.into_iter().filter(|face| !present.contains(face)));
    TranslucentClass::StaticPerNormal {
        order: order.try_into().expect("every face once"),
    }
}

/// True when some quad of direction `a` may hide part of some quad of direction `b` from
/// some view, so `b` must be drawn first. Needs a point of `b` strictly behind a plane of
/// `a` and a point of `a` strictly in front of a plane of `b`; directions on one axis never
/// cover each other.
fn may_cover(a: Face, a_extent: Extent, b: Face, b_extent: Extent) -> bool {
    if a.axis() == b.axis() {
        return false;
    }
    let (a_axis, b_axis) = (a.axis(), b.axis());
    let b_behind_a = if a.is_positive() {
        b_extent.min[a_axis] < a_extent.max[a_axis]
    } else {
        b_extent.max[a_axis] > a_extent.min[a_axis]
    };
    let a_before_b = if b.is_positive() {
        a_extent.max[b_axis] > b_extent.min[b_axis]
    } else {
        a_extent.min[b_axis] < b_extent.max[b_axis]
    };
    b_behind_a && a_before_b
}
