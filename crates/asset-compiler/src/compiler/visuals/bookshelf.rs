use super::super::*;
use super::state::exact_tagged_int;

pub(in crate::compiler) fn chiseled_bookshelf_quads(
    empty: u32,
    occupied: u32,
    side: u32,
    top: u32,
    books: u32,
) -> Vec<ModelQuad> {
    debug_assert!(books <= 63);
    let ordinary = cuboid_quads(
        [side, side, top, top, empty, side],
        [0, 0, 0],
        [256, 256, 256],
    );
    let mut quads = Vec::with_capacity(11);
    for index in [0, 1, 2, 3, 5] {
        let mut quad = ordinary[index];
        let face = quad.flags & MODEL_QUAD_FLAG_FACE_MASK;
        quad.flags |= face << 4;
        quads.push(quad);
    }

    const X: [i16; 4] = [0, 85, 171, 256];
    const U: [u16; 4] = [0, 1365, 2731, 4096];
    for slot in 0..6_usize {
        let column = slot % 3;
        let top_row = slot < 3;
        let (min_y, max_y, min_v, max_v) = if top_row {
            (128, 256, 0, 2048)
        } else {
            (0, 128, 2048, 4096)
        };
        let min_x = X[column];
        let max_x = X[column + 1];
        let min_u = U[column];
        let max_u = U[column + 1];
        quads.push(ModelQuad {
            positions: [
                [min_x, min_y, 0],
                [min_x, max_y, 0],
                [max_x, max_y, 0],
                [max_x, min_y, 0],
            ],
            uvs: [
                [min_u, max_v],
                [min_u, min_v],
                [max_u, min_v],
                [max_u, max_v],
            ],
            material: if books & (1 << slot) == 0 {
                empty
            } else {
                occupied
            },
            flags: 5 | (5 << 4),
        });
    }
    quads
}

pub(in crate::compiler) fn chiseled_bookshelf_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 4]> {
    let faces = pack.blocks.get_exact_faces("chiseled_bookshelf")?;
    if faces
        != [
            "chiseled_bookshelf_side",
            "chiseled_bookshelf_side",
            "chiseled_bookshelf_top",
            "chiseled_bookshelf_top",
            "chiseled_bookshelf_front",
            "chiseled_bookshelf_side",
        ]
    {
        return None;
    }
    let front = pack
        .terrain
        .get_exact_pair_no_tint("chiseled_bookshelf_front")?;
    let side = pack
        .terrain
        .get_exact_static_no_tint("chiseled_bookshelf_side")?;
    let top = pack
        .terrain
        .get_exact_static_no_tint("chiseled_bookshelf_top")?;
    let paths = [front[0], front[1], side, top];
    if paths.into_iter().collect::<BTreeSet<_>>().len() != 4 {
        return None;
    }
    Some([
        (
            Descriptor {
                path: front[0].into(),
                texture_key: "chiseled_bookshelf_front".into(),
                flags: 0,
            },
            "chiseled_bookshelf_front".into(),
        ),
        (
            Descriptor {
                path: front[1].into(),
                texture_key: "chiseled_bookshelf_front".into(),
                flags: 0,
            },
            "chiseled_bookshelf_front".into(),
        ),
        (
            Descriptor {
                path: side.into(),
                texture_key: "chiseled_bookshelf_side".into(),
                flags: 0,
            },
            "chiseled_bookshelf_side".into(),
        ),
        (
            Descriptor {
                path: top.into(),
                texture_key: "chiseled_bookshelf_top".into(),
                flags: 0,
            },
            "chiseled_bookshelf_top".into(),
        ),
    ])
}

pub(in crate::compiler) fn is_chiseled_bookshelf_name(name: &str) -> bool {
    name == "minecraft:chiseled_bookshelf"
}

pub(in crate::compiler) fn exact_chiseled_bookshelf_state(
    record: &RegistryRecord,
) -> Option<(u32, u32)> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 2 {
        return None;
    }
    let books = exact_tagged_int(state.get("books_stored")?, 63)?;
    let direction = exact_tagged_int(state.get("direction")?, 3)?;
    let mask = 1 << (ModelStateField::Connections as u8 - 1)
        | 1 << (ModelStateField::Orientation as u8 - 1);
    if record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Connections) != Some(books)
        || record.model_state.get(ModelStateField::Orientation) != Some(direction)
    {
        return None;
    }
    Some((books, direction))
}

pub(in crate::compiler) fn is_chiseled_bookshelf_record(record: &RegistryRecord) -> bool {
    if !is_chiseled_bookshelf_name(&record.name)
        || record.model_family != ModelFamily::ChiseledBookshelf
        || record.contributor_role != ContributorRole::Primary
        || record.flags != BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        || record.face_coverage != 0x3f
        || record.collision_seed.shape_id != 1
        || record.collision_seed.confidence != assets::CollisionConfidence::CollisionOnly
        || record.collision_seed.boxes.as_ref()
            != [assets::CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..assets::CollisionBox::default()
            }]
    {
        return false;
    }
    exact_chiseled_bookshelf_state(record).is_some()
}

pub(in crate::compiler) fn chiseled_bookshelf_inventory_is_exact(
    records: &[RegistryRecord],
) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_chiseled_bookshelf_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 256 {
        return false;
    }
    let mut seen = [false; 256];
    for record in selected {
        if !is_chiseled_bookshelf_record(record) {
            return false;
        }
        let Some((books, direction)) = exact_chiseled_bookshelf_state(record) else {
            return false;
        };
        let index = (books * 4 + direction) as usize;
        if record.sequential_id != 1605 + index as u32 || seen[index] {
            return false;
        }
        seen[index] = true;
    }
    seen.into_iter().all(|present| present)
}
