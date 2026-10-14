//! Scalar reference mesher: the test oracle for [`super::mesher::Mesher`].
//!
//! It reads voxels one at a time through absolute coordinates, classes them with
//! [`voxel_class`], fills exterior air breadth-first (a padding voxel is open when its
//! same-level, coarser or any finer voxel is not solid) and merges each face plane with a
//! plain scan, so it shares no mask, gather or slab code with the mesher.

use std::collections::VecDeque;

use crate::{
    classify::{BlockClass, MaterialClasses, voxel_class},
    geometry::{Face, MAX_LEVEL, SECTION_SIDE, SectionPos, voxel_index},
    mesh::{
        Bucket, ExteriorCulling, FALLBACK_LIGHT, MeshList,
        input::MeshInput,
        overlaps_resident_columns,
        quad::{MAX_QUAD_SIDE, PAYLOAD_LIGHT_MASK, Quad, in_plane_axes},
    },
    section::Section,
    voxel::{Light, MATERIAL_FIELD, MaterialId, Voxel},
};

const SIDE: i64 = SECTION_SIDE as i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Air,
    Solid,
    Water,
    SeeThrough,
    /// A waterlogged see-through voxel.
    WetSeeThrough,
}

/// The classes the reference reads, with the water material that colours the surface of
/// waterlogged voxels.
#[derive(Clone, Copy)]
pub(super) struct Classes<'a, C> {
    pub(super) classes: &'a C,
    pub(super) water: Option<MaterialId>,
}

impl<C: MaterialClasses> Classes<'_, C> {
    /// The reference's class of `voxel`; waterlogged see-through voxels hold water only
    /// with a water material.
    fn kind(&self, voxel: Voxel) -> Kind {
        match voxel_class(self.classes, voxel) {
            BlockClass::Air => Kind::Air,
            BlockClass::Opaque | BlockClass::Leaves => Kind::Solid,
            BlockClass::Water => Kind::Water,
            BlockClass::SeeThrough if voxel.has_water() && self.water.is_some() => {
                Kind::WetSeeThrough
            }
            BlockClass::SeeThrough => Kind::SeeThrough,
        }
    }

    /// `voxel` with its class.
    fn classed(&self, voxel: Voxel) -> (Kind, Voxel) {
        (self.kind(voxel), voxel)
    }

    /// The payload word of a face of `own` in `bucket`.
    fn payload(&self, own: (Kind, Voxel), bucket: Bucket) -> u32 {
        match (own.0, bucket, self.water) {
            (Kind::WetSeeThrough, Bucket::Translucent, Some(water)) => {
                (own.1.bits() & !MATERIAL_FIELD.mask()) | (water.raw() << MATERIAL_FIELD.shift)
            }
            _ => own.1.bits(),
        }
    }
}

/// The bucket of a face of `own` against `front`, or `None` when it does not show.
fn shows(own: (Kind, Voxel), front: (Kind, Voxel)) -> Option<Bucket> {
    let opaque = |shows: bool| shows.then_some(Bucket::Opaque);
    match own.0 {
        Kind::Air => None,
        Kind::Solid => opaque(front.0 != Kind::Solid),
        Kind::Water => {
            matches!(front.0, Kind::Air | Kind::SeeThrough).then_some(Bucket::Translucent)
        }
        Kind::WetSeeThrough if front.0 == Kind::Air => Some(Bucket::Translucent),
        Kind::SeeThrough | Kind::WetSeeThrough => {
            opaque(front.0 != Kind::Solid && own.1.material() != front.1.material())
        }
    }
}

/// What unknown space reads as.
fn unknown() -> Voxel {
    Voxel::air(FALLBACK_LIGHT)
}

/// The voxel at absolute level coordinates `at` in whichever of `sections` holds it.
fn find(sections: &[&Section], level: u8, at: [i64; 3]) -> Option<Voxel> {
    let pos = SectionPos::new(
        level,
        at[0].div_euclid(SIDE) as i32,
        at[1].div_euclid(SIDE) as i32,
        at[2].div_euclid(SIDE) as i32,
    )
    .ok()?;
    let section = sections.iter().find(|section| section.pos() == pos)?;
    let local = at.map(|value| value.rem_euclid(SIDE) as usize);
    Some(
        section
            .voxels()
            .get(voxel_index(local[0], local[1], local[2])),
    )
}

/// Every quad of `input`'s mesh with its list and bucket, sorted.
pub(super) fn mesh(
    input: &MeshInput<'_>,
    classes: Classes<'_, impl MaterialClasses>,
    culling: ExteriorCulling,
) -> Vec<(MeshList, Bucket, Quad)> {
    let section = input.section();
    let pos = section.pos();
    let level = pos.level();
    let origin = [pos.x(), pos.y(), pos.z()].map(|value| i64::from(value) * SIDE);
    let same: Vec<&Section> = Face::ALL
        .iter()
        .filter_map(|&face| input.face(face).same)
        .chain([section])
        .collect();
    // Section-local coordinates, one voxel of padding included.
    let voxel = |local: [i64; 3]| {
        let at = [0, 1, 2].map(|axis| origin[axis] + local[axis]);
        find(&same, level, at).unwrap_or_else(unknown)
    };
    let inside = |local: [i64; 3]| local.iter().all(|&value| (0..SIDE).contains(&value));
    let coarser_applies = |face: Face| {
        level < MAX_LEVEL && pos.neighbour(face).and_then(SectionPos::parent) != pos.parent()
    };
    // The coarser voxel across `face` at absolute level coordinates `at`, if it applies.
    let coarse = |face: Face, at: [i64; 3]| {
        coarser_applies(face).then(|| {
            let coarse_at = at.map(|value| value.div_euclid(2));
            let found = input
                .face(face)
                .coarser
                .and_then(|c| find(&[c], level + 1, coarse_at))
                .unwrap_or_else(unknown);
            classes.classed(found)
        })
    };
    // The four finer children across `face` touching the padding voxel at `at`.
    let fine = |face: Face, at: [i64; 3]| -> Vec<(Kind, Voxel)> {
        if level == 0 {
            return Vec::new();
        }
        let [u_axis, v_axis] = in_plane_axes(face);
        let finer: Vec<&Section> = input.face(face).finer.iter().flatten().copied().collect();
        (0..4)
            .map(|child: i64| {
                let mut fine_at = at.map(|value| 2 * value);
                fine_at[face.axis()] += i64::from(!face.is_positive());
                fine_at[u_axis] += child & 1;
                fine_at[v_axis] += child >> 1;
                let found = find(&finer, level - 1, fine_at).unwrap_or_else(unknown);
                classes.classed(found)
            })
            .collect()
    };
    // A padding voxel is also open where the coarser voxel or a finer child is.
    let open_across = |local: [i64; 3]| {
        let outside: Vec<usize> = (0..3)
            .filter(|&axis| !(0..SIDE).contains(&local[axis]))
            .collect();
        let [axis] = outside[..] else {
            return false;
        };
        let face = Face::of_axis(axis, local[axis] >= SIDE);
        if input.face(face).same.is_none() {
            return false;
        }
        let at = [0, 1, 2].map(|a| origin[a] + local[a]);
        coarse(face, at).is_some_and(|(kind, _)| kind != Kind::Solid)
            || fine(face, at).iter().any(|(kind, _)| *kind != Kind::Solid)
    };
    let reached = exterior(level, culling, |local| {
        classes.kind(voxel(local)) != Kind::Solid || open_across(local)
    });
    let mut out = Vec::new();
    for face in Face::ALL {
        let axis = face.axis();
        let [u_axis, v_axis] = in_plane_axes(face);
        let step = if face.is_positive() { 1 } else { -1 };
        let given = input.face(face);
        let lists_apply = given.same.is_some();
        for layer in 0..SIDE {
            let mut planes = [[[None; SECTION_SIDE * SECTION_SIDE]; 2]; 3];
            for v in 0..SIDE {
                for u in 0..SIDE {
                    let mut local = [0; 3];
                    local[axis] = layer;
                    local[u_axis] = u;
                    local[v_axis] = v;
                    let own = voxel(local);
                    let own = classes.classed(own);
                    let mut ahead = local;
                    ahead[axis] += step;
                    let front = classes.classed(voxel(ahead));
                    let cell = (v * SIDE + u) as usize;
                    let with_light = |bucket: Bucket, light: u32| {
                        Some((classes.payload(own, bucket) & !PAYLOAD_LIGHT_MASK) | light)
                    };
                    if let Some(bucket) = shows(own, front) {
                        if !inside(ahead) || reached.as_ref().is_none_or(|r| r[index(ahead)]) {
                            planes[0][bucket.index()][cell] =
                                with_light(bucket, front.1.bits() & PAYLOAD_LIGHT_MASK);
                        }
                        continue;
                    }
                    if inside(ahead) || own.0 == Kind::Air || !lists_apply {
                        continue;
                    }
                    let at = [0, 1, 2].map(|a| origin[a] + ahead[a]);
                    let to_coarser = coarse(face, at).and_then(|coarse| shows(own, coarse));
                    // Against several children, one opaque face covers the voxel's face.
                    let finer: Vec<Bucket> = fine(face, at)
                        .into_iter()
                        .filter_map(|child| shows(own, child))
                        .collect();
                    let to_finer = if finer.contains(&Bucket::Opaque) {
                        Some(Bucket::Opaque)
                    } else {
                        finer.first().copied()
                    };
                    if to_coarser.is_none() && to_finer.is_none() {
                        continue;
                    }
                    let light = if front.0 == Kind::Solid {
                        brightest(local, ahead, |at| classes.classed(voxel(at)))
                    } else {
                        front.1.bits() & PAYLOAD_LIGHT_MASK
                    };
                    if let Some(bucket) = to_coarser {
                        planes[1][bucket.index()][cell] = with_light(bucket, light);
                    }
                    if let Some(bucket) = to_finer {
                        planes[2][bucket.index()][cell] = with_light(bucket, light);
                    }
                }
            }
            for (list, buckets) in MeshList::ALL.into_iter().zip(&mut planes) {
                for (bucket, plane) in Bucket::ALL.into_iter().zip(buckets.iter_mut()) {
                    merge(plane, |u, v, width, height, payload| {
                        let mut min = [0; 3];
                        min[axis] = layer as usize;
                        min[u_axis] = u;
                        min[v_axis] = v;
                        out.push((list, bucket, Quad::new(face, min, [width, height], payload)));
                    });
                }
            }
        }
    }
    out.sort();
    out
}

/// Voxel index of in-section coordinates.
fn index(local: [i64; 3]) -> usize {
    voxel_index(local[0] as usize, local[1] as usize, local[2] as usize)
}

/// Light bits of the brightest non-solid neighbour of `local` other than `ahead`.
fn brightest(local: [i64; 3], ahead: [i64; 3], at: impl Fn([i64; 3]) -> (Kind, Voxel)) -> u32 {
    let mut best: Option<Light> = None;
    for axis in 0..3 {
        for step in [-1, 1] {
            let mut neighbour = local;
            neighbour[axis] += step;
            let (kind, voxel) = at(neighbour);
            if neighbour == ahead || kind == Kind::Solid {
                continue;
            }
            let light = voxel.light();
            best = Some(best.map_or(light, |best| {
                Light::new(best.sky().max(light.sky()), best.block().max(light.block()))
            }));
        }
    }
    Voxel::air(best.unwrap_or(FALLBACK_LIGHT)).bits()
}

/// Section voxels reached through open voxels from the padding and, at levels drawn over
/// resident columns, from the chunk-column planes; `None` when culling is off.
fn exterior(
    level: u8,
    culling: ExteriorCulling,
    open: impl Fn([i64; 3]) -> bool,
) -> Option<Vec<bool>> {
    if culling == ExteriorCulling::Off {
        return None;
    }
    // Blocks per chunk column side over blocks per voxel.
    let spacing = (world::SUB_CHUNK_SIDE as i64) >> level;
    let by_plane = |value: i64| {
        overlaps_resident_columns(level)
            && (1..SIDE / spacing).any(|k| value == k * spacing - 1 || value == k * spacing)
    };
    let mut reached = vec![false; SECTION_SIDE * SECTION_SIDE * SECTION_SIDE];
    let mut queue = VecDeque::new();
    let visit = |at: [i64; 3], reached: &mut Vec<bool>, queue: &mut VecDeque<[i64; 3]>| {
        if at.iter().all(|&value| (0..SIDE).contains(&value)) && open(at) && !reached[index(at)] {
            reached[index(at)] = true;
            queue.push_back(at);
        }
    };
    for x in -1..=SIDE {
        for y in -1..=SIDE {
            for z in -1..=SIDE {
                let padding = [x, y, z].iter().any(|value| !(0..SIDE).contains(value));
                if padding && open([x, y, z]) {
                    for axis in 0..3 {
                        for step in [-1, 1] {
                            let mut next = [x, y, z];
                            next[axis] += step;
                            visit(next, &mut reached, &mut queue);
                        }
                    }
                } else if !padding && (by_plane(x) || by_plane(z)) {
                    visit([x, y, z], &mut reached, &mut queue);
                }
            }
        }
    }
    while let Some(at) = queue.pop_front() {
        for axis in 0..3 {
            for step in [-1, 1] {
                let mut next = at;
                next[axis] += step;
                visit(next, &mut reached, &mut queue);
            }
        }
    }
    Some(reached)
}

/// Plain greedy merge of one key plane, `[v][u]`, in the mesher's scan and growth order.
fn merge(
    plane: &mut [Option<u32>; SECTION_SIDE * SECTION_SIDE],
    mut emit: impl FnMut(usize, usize, usize, usize, u32),
) {
    for v in 0..SECTION_SIDE {
        let mut u = 0;
        while u < SECTION_SIDE {
            let Some(key) = plane[v * SECTION_SIDE + u] else {
                u += 1;
                continue;
            };
            let mut width = 1;
            while u + width < SECTION_SIDE
                && width < MAX_QUAD_SIDE
                && plane[v * SECTION_SIDE + u + width] == Some(key)
            {
                width += 1;
            }
            let mut height = 1;
            while v + height < SECTION_SIDE
                && height < MAX_QUAD_SIDE
                && (0..width).all(|i| plane[(v + height) * SECTION_SIDE + u + i] == Some(key))
            {
                height += 1;
            }
            for row in v..v + height {
                plane[row * SECTION_SIDE + u..row * SECTION_SIDE + u + width].fill(None);
            }
            emit(u, v, width, height, key);
            u += width;
        }
    }
}
