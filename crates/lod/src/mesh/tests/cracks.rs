//! Watertightness at one-level transitions: around a boundary between a section and a
//! neighbour region one level coarser, every face of the rendered terrain (each side at its
//! own level) must be drawn by the main list plus the border list matching the other side's
//! level, with coarse quads split into fine units, so the transition closes without skirts.

use std::collections::{HashSet, VecDeque};

use super::*;
use crate::{
    geometry::MAX_LEVEL,
    mesh::class::{SOLID, face_bucket},
};

/// Unit faces `(face, section-local voxel)` of a mesh's main list plus `list`.
pub(super) fn drawn(mesh: &SectionMesh, list: MeshList) -> HashSet<(Face, [usize; 3])> {
    let mut faces = HashSet::new();
    for drawn_list in [MeshList::Main, list] {
        for bucket in Bucket::ALL {
            for face in Face::ALL {
                for quad in mesh.group(drawn_list, bucket, face) {
                    let (min, max) = quad.voxel_box();
                    for x in min[0]..max[0] {
                        for y in min[1]..max[1] {
                            for z in min[2]..max[2] {
                                faces.insert((face, [x, y, z]));
                            }
                        }
                    }
                }
            }
        }
    }
    faces
}

/// Random terrain of every class, independent per level, half air.
pub(super) fn mixed(seed: u64) -> impl Fn(u8, [i64; 3]) -> Voxel {
    move |level, at| {
        let roll = hash(level, at.map(|value| value >> 1), seed);
        let light = Light::new((roll >> 8) as u8 % 16, (roll >> 12) as u8 % 16);
        match roll % 8 {
            0..=3 => Voxel::air(light),
            4 => block(blocks::WATER, light),
            5 => block(blocks::GLASS, light),
            6 => Voxel::HIDDEN,
            _ => block(blocks::STONE, light),
        }
    }
}

/// A fine section on its parent's `face` and the coarser section across that face, which
/// belongs to another parent.
pub(super) fn transition(level: u8, face: Face) -> (SectionPos, SectionPos) {
    let mut at = [2, 0, -2];
    at[face.axis()] = if face.is_positive() { 1 } else { 0 };
    let fine = pos(level, at[0], at[1], at[2]);
    let coarse = fine
        .neighbour(face)
        .and_then(SectionPos::parent)
        .expect("coarser neighbour");
    assert_ne!(Some(coarse), fine.parent());
    (fine, coarse)
}

/// Which side of a transition a fine-level cell is rendered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Fine,
    Coarse,
}

/// The terrain around one transition as rendered: cells in fine-level units, the fine
/// section's at its own level and the coarse section's at the coarser one.
struct Rendered<'w> {
    fine: &'w Section,
    coarse: &'w Section,
    fine_origin: [i64; 3],
    coarse_origin: [i64; 3],
    classes: MeshClasses,
}

impl Rendered<'_> {
    /// The side rendering absolute fine-level `cell`, its voxel word and class; `None`
    /// outside both sections.
    fn at(&self, cell: [i64; 3]) -> Option<(Side, u32, u8)> {
        let side = SECTION_SIDE as i64;
        let fine_local = [0, 1, 2].map(|axis| cell[axis] - self.fine_origin[axis]);
        let coarse_local =
            [0, 1, 2].map(|axis| cell[axis].div_euclid(2) - self.coarse_origin[axis]);
        let (side_of, section, local) = if fine_local.iter().all(|value| (0..side).contains(value))
        {
            (Side::Fine, self.fine, fine_local)
        } else if coarse_local.iter().all(|value| (0..side).contains(value)) {
            (Side::Coarse, self.coarse, coarse_local)
        } else {
            return None;
        };
        let [x, y, z] = local.map(|value| value as usize);
        let word = section
            .voxels()
            .get(crate::geometry::voxel_index(x, y, z))
            .bits();
        Some((side_of, word, self.classes.of_word(word)))
    }

    /// The section-local voxel of `cell` on its side.
    fn local(&self, side: Side, cell: [i64; 3]) -> [usize; 3] {
        [0, 1, 2].map(|axis| match side {
            Side::Fine => (cell[axis] - self.fine_origin[axis]) as usize,
            Side::Coarse => (cell[axis].div_euclid(2) - self.coarse_origin[axis]) as usize,
        })
    }

    /// Open cells of `side` reached from the other side's open cells beside the transition
    /// plane through open cells of `side`: air either section's exterior culling must
    /// treat as outside, since a camera in the other side's open cells sees it.
    fn reached_across(&self, side: Side, other_layer: &[[i64; 3]]) -> HashSet<[i64; 3]> {
        let open = |cell: [i64; 3]| {
            self.at(cell)
                .is_some_and(|(at_side, _, class)| at_side == side && class != SOLID)
        };
        let mut reached = HashSet::new();
        let mut queue = VecDeque::new();
        for &cell in other_layer {
            if self.at(cell).is_some_and(|(_, _, class)| class != SOLID) {
                for axis in 0..3 {
                    for step in [-1, 1] {
                        let mut next = cell;
                        next[axis] += step;
                        if open(next) && reached.insert(next) {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
        while let Some(cell) = queue.pop_front() {
            for axis in 0..3 {
                for step in [-1, 1] {
                    let mut next = cell;
                    next[axis] += step;
                    if open(next) && reached.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }
        reached
    }
}

/// Checks that every face of the rendered terrain within two fine cells of the transition
/// plane, over the fine section's extent, is drawn; with culling, faces whose front cell
/// only sealed pockets reach may go. Returns the faces checked.
fn check_transition(
    world: &World<'_>,
    (fine, coarse): (SectionPos, SectionPos),
    face: Face,
    mesher: &mut Mesher,
    culling: ExteriorCulling,
) -> usize {
    let classes = mesh_classes();
    let side = SECTION_SIDE as i64;
    let fine_mesh = mesher.mesh(&world.input(fine), &classes, culling);
    let coarse_mesh = mesher.mesh(&world.input(coarse), &classes, culling);
    let drawn_fine = drawn(&fine_mesh, MeshList::ToCoarser);
    let drawn_coarse = drawn(&coarse_mesh, MeshList::ToFiner);
    let origin = |p: SectionPos| [p.x(), p.y(), p.z()].map(|value| i64::from(value) * side);
    let rendered = Rendered {
        fine: &world.sections[&fine],
        coarse: &world.sections[&coarse],
        fine_origin: origin(fine),
        coarse_origin: origin(coarse),
        classes,
    };
    let axis = face.axis();
    // Fine cells along the face axis: two layers in the fine section, then two across.
    let plane = rendered.fine_origin[axis] + if face.is_positive() { side } else { 0 };
    let layer = |depth: i64| {
        if face.is_positive() {
            plane + depth
        } else {
            plane - 1 - depth
        }
    };
    let [u_axis, v_axis] = crate::mesh::quad::in_plane_axes(face);
    let cells_of = |depth: i64| -> Vec<[i64; 3]> {
        let mut cells = Vec::new();
        for v in 0..side {
            for u in 0..side {
                let mut cell = [0; 3];
                cell[axis] = layer(depth);
                cell[u_axis] = rendered.fine_origin[u_axis] + u;
                cell[v_axis] = rendered.fine_origin[v_axis] + v;
                cells.push(cell);
            }
        }
        cells
    };
    let reached = (culling == ExteriorCulling::SectionLocal).then(|| {
        let mut reached = rendered.reached_across(Side::Fine, &cells_of(0));
        reached.extend(rendered.reached_across(Side::Coarse, &cells_of(-1)));
        reached
    });
    let in_footprint = |cell: [i64; 3]| {
        [u_axis, v_axis]
            .iter()
            .all(|&a| (rendered.fine_origin[a]..rendered.fine_origin[a] + side).contains(&cell[a]))
    };
    let mut checked = 0;
    for depth in -2..2 {
        for cell in cells_of(depth) {
            let Some((own_side, own, own_class)) = rendered.at(cell) else {
                continue;
            };
            for direction in Face::ALL {
                let mut ahead = cell;
                ahead[direction.axis()] += if direction.is_positive() { 1 } else { -1 };
                if !in_footprint(ahead) {
                    continue;
                }
                let Some((front_side, front, front_class)) = rendered.at(ahead) else {
                    continue;
                };
                if face_bucket(own_class, own, front_class, front).is_none() {
                    continue;
                }
                // Faces across the plane face the other side's open cells and always show;
                // others may go when only a sealed pocket lies in front.
                let required = front_side != own_side
                    || reached
                        .as_ref()
                        .is_none_or(|reached| reached.contains(&ahead));
                if !required {
                    continue;
                }
                checked += 1;
                let local = rendered.local(own_side, cell);
                let shown = match own_side {
                    Side::Fine => drawn_fine.contains(&(direction, local)),
                    Side::Coarse => drawn_coarse.contains(&(direction, local)),
                };
                assert!(
                    shown,
                    "level {} {face:?} {culling:?}: {own_side:?} face {direction:?} at \
                     {local:?} (cell {cell:?}) missing",
                    fine.level()
                );
            }
        }
    }
    checked
}

#[test]
fn one_level_transitions_are_watertight_at_every_level() {
    let mut mesher = Mesher::new();
    for level in 0..MAX_LEVEL {
        for face in Face::ALL {
            let generate = mixed(0xc4ac_0000 + u64::from(level) * 16 + face.index() as u64);
            let mut world = World::new(&generate);
            let (fine, coarse) = transition(level, face);
            world.build_around(fine, |_| true);
            world.build_around(coarse, |_| true);
            for culling in [ExteriorCulling::Off, ExteriorCulling::SectionLocal] {
                let checked = check_transition(&world, (fine, coarse), face, &mut mesher, culling);
                assert!(
                    checked > 1000,
                    "level {level} {face:?}: the fixture exercises the boundary ({checked})"
                );
            }
        }
    }
}

#[test]
fn border_lists_hold_only_faces_the_same_level_neighbour_hides() {
    let generate = |level: u8, [_, y, _]: [i64; 3]| match level {
        // Solid below y 8 at the fine level; the coarse level has air from y 2 (blocks 4).
        0 if y < 8 => block(blocks::STONE, Light::DARK),
        1 if y < 2 => block(blocks::STONE, Light::DARK),
        _ => Voxel::air(Light::new(12, 2)),
    };
    let mut world = World::new(&generate);
    let fine = pos(0, 1, 0, 0);
    world.build_around(fine, |_| true);
    let mesh = mesh_checked(
        &world,
        fine,
        &mut Mesher::new(),
        ExteriorCulling::SectionLocal,
    );
    let to_coarser = mesh.group(MeshList::ToCoarser, Bucket::Opaque, Face::PosX);
    let area: usize = to_coarser.iter().map(|q| q.size()[0] * q.size()[1]).sum();
    assert_eq!(
        area,
        4 * SECTION_SIDE,
        "rows 4..8 face coarse air across +X"
    );
    for quad in to_coarser {
        let expected = if quad.origin()[1] == 7 {
            Light::new(12, 2)
        } else {
            crate::mesh::FALLBACK_LIGHT
        };
        assert_eq!(
            quad.light(),
            expected,
            "solid in front: the brightest see-through neighbour, if any, lights {quad:?}"
        );
    }
    assert!(
        mesh.group(MeshList::Main, Bucket::Opaque, Face::PosX)
            .is_empty()
    );
    assert!(
        mesh.group(MeshList::ToCoarser, Bucket::Opaque, Face::NegX)
            .is_empty(),
        "the -X neighbour shares this section's parent"
    );
}
