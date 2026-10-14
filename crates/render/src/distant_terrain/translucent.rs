//! The draw order of distant water: sections far to near by a counting sort
//! ([`lod::draw::order::TranslucentOrder`]), and within each section the order its mesh-time
//! class allows. Only sections of class [`TranslucentClass::General`] are ordered per view,
//! here by the distance of each direction's clusters, farthest first.

use lod::{
    draw::order::TranslucentOrder,
    geometry::{Face, SectionPos, section_blocks},
    mesh::translucent::TranslucentClass,
};

use super::table::TranslucentClusters;

/// Builds the cluster order and tells when it changed.
#[derive(Debug, Default)]
pub(crate) struct TranslucentSorter {
    sort: TranslucentOrder,
    sections: Vec<SectionPos>,
    order: Vec<u32>,
    next: Vec<u32>,
    /// The camera block the order was last built for.
    built_at: Option<[i64; 3]>,
}

impl TranslucentSorter {
    /// The translucent cluster slots far to near seen from `camera` when they differ from
    /// the last order returned; rebuilt only when `changed` or the camera entered another
    /// block. `max_distance` is the drawing distance in blocks.
    pub(crate) fn update<'a>(
        &mut self,
        translucent: impl Iterator<Item = &'a TranslucentClusters>,
        camera: [f64; 3],
        max_distance: u32,
        changed: bool,
    ) -> Option<&[u32]> {
        let block = camera.map(|value| value.floor() as i64);
        if !changed && self.built_at == Some(block) {
            return None;
        }
        self.built_at = Some(block);
        let mut sections: Vec<&TranslucentClusters> = translucent.collect();
        sections.sort_unstable_by_key(|section| section.pos.key());
        self.sections.clear();
        self.sections
            .extend(sections.iter().map(|section| section.pos));
        self.next.clear();
        for &index in self.sort.sort(camera, &self.sections, max_distance) {
            push_section(&mut self.next, sections[index as usize], camera);
        }
        if self.next == self.order {
            return None;
        }
        std::mem::swap(&mut self.order, &mut self.next);
        Some(&self.order)
    }

    /// The order last returned.
    pub(crate) fn order(&self) -> &[u32] {
        &self.order
    }

    /// Forgets the order, so the next update rebuilds and returns it.
    pub(crate) fn reset(&mut self) {
        self.order.clear();
        self.built_at = None;
    }
}

/// Appends `section`'s cluster slots in its back-to-front order seen from `camera`.
fn push_section(out: &mut Vec<u32>, section: &TranslucentClusters, camera: [f64; 3]) {
    let faces = match section.class {
        TranslucentClass::None | TranslucentClass::CoplanarOrOneNormal => {
            out.extend(section.clusters.iter().map(|&(_, _, slot)| slot));
            return;
        }
        TranslucentClass::StaticPerNormal { order } => order,
        TranslucentClass::General => general_order(section, camera),
    };
    for face in faces {
        out.extend(
            section
                .clusters
                .iter()
                .filter(|&&(cluster_face, _, _)| cluster_face == face)
                .map(|&(_, _, slot)| slot),
        );
    }
}

/// `section`'s directions, farthest clusters first.
fn general_order(section: &TranslucentClusters, camera: [f64; 3]) -> [Face; 6] {
    let scale = section_blocks(section.pos.level()) as f64 / lod::geometry::SECTION_SIDE as f64;
    let min = section.pos.min_block();
    let distance = |face: Face| -> f64 {
        let centre = section.centres[face.index()];
        (0..3)
            .map(|axis| {
                let world = min[axis] as f64 + f64::from(centre[axis]) * scale;
                (world - camera[axis]).powi(2)
            })
            .sum()
    };
    let mut faces = Face::ALL;
    faces.sort_by(|a, b| distance(*b).total_cmp(&distance(*a)));
    faces
}

#[cfg(test)]
mod tests {
    use lod::mesh::MeshList;

    use super::*;

    fn section(x: i32, class: TranslucentClass, slots: &[(Face, u32)]) -> TranslucentClusters {
        let mut centres = [[0.0; 3]; 6];
        centres[Face::PosY.index()] = [16.0, 32.0, 16.0];
        centres[Face::PosX.index()] = [32.0, 16.0, 16.0];
        TranslucentClusters {
            pos: SectionPos::new(0, x, 0, 0).expect("position"),
            class,
            clusters: slots
                .iter()
                .map(|&(face, slot)| (face, MeshList::Main, slot))
                .collect(),
            centres,
        }
    }

    #[test]
    fn sections_go_far_to_near_and_rebuild_only_on_change() {
        let near = section(0, TranslucentClass::CoplanarOrOneNormal, &[(Face::PosY, 1)]);
        let far = section(
            10,
            TranslucentClass::CoplanarOrOneNormal,
            &[(Face::PosY, 2)],
        );
        let mut sorter = TranslucentSorter::default();
        let camera = [16.0, 40.0, 16.0];
        let order = sorter
            .update([&near, &far].into_iter(), camera, 4096, true)
            .map(<[u32]>::to_vec);
        assert_eq!(order, Some(vec![2, 1]));
        assert!(
            sorter
                .update([&near, &far].into_iter(), [16.5, 40.2, 16.9], 4096, false)
                .is_none(),
            "same block"
        );
        assert!(
            sorter
                .update([&near, &far].into_iter(), [17.0, 40.0, 16.0], 4096, false)
                .is_none(),
            "another block with the same order"
        );
        let order = sorter
            .update([&near, &far].into_iter(), [330.0, 40.0, 16.0], 4096, false)
            .map(<[u32]>::to_vec);
        assert_eq!(order, Some(vec![1, 2]));
        assert_eq!(sorter.order(), &[1, 2]);
    }

    #[test]
    fn static_and_general_sections_order_their_directions() {
        let fixed = section(
            0,
            TranslucentClass::StaticPerNormal {
                order: [
                    Face::PosX,
                    Face::PosY,
                    Face::NegX,
                    Face::NegY,
                    Face::NegZ,
                    Face::PosZ,
                ],
            },
            &[(Face::PosY, 1), (Face::PosX, 2)],
        );
        let mut out = Vec::new();
        push_section(&mut out, &fixed, [0.0; 3]);
        assert_eq!(out, [2, 1]);
        let general = section(
            0,
            TranslucentClass::General,
            &[(Face::PosY, 1), (Face::PosX, 2)],
        );
        out.clear();
        // Above the section: the top face is nearer, so the side face goes first.
        push_section(&mut out, &general, [16.0, 100.0, 16.0]);
        assert_eq!(out, [2, 1]);
        out.clear();
        // Far along +X: the side face is nearer.
        push_section(&mut out, &general, [500.0, 16.0, 16.0]);
        assert_eq!(out, [1, 2]);
    }
}
