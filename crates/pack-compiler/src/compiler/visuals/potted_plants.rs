use super::super::*;
use super::context::{ModelStorage, RuleInputs, push_model_template};
use super::geometry::vanilla_cuboid_quads;
use assets::BlockFace;

/// Whether compilation needs all face selectors for a dedicated pot-content model.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    (record.model_family == ModelFamily::Cross
        && !matches!(
            record.name.as_ref(),
            "minecraft:fire" | "minecraft:soul_fire"
        ))
        || matches!(
            record.name.as_ref(),
            "minecraft:cactus"
                | "minecraft:bamboo"
                | "minecraft:azalea"
                | "minecraft:flowering_azalea"
        )
}

/// Adds a pot-content template without changing the ordinary terrain model's geometry.
pub(in crate::compiler) fn apply(
    record: &RegistryRecord,
    visual: &mut BlockVisual,
    inputs: &RuleInputs<'_>,
    cache: &mut BTreeMap<(u32, [u32; 6]), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<(), AssetError> {
    if !is_record(record) || visual.model_template == assets::NO_MODEL_TEMPLATE {
        return Ok(());
    }
    let materials = if record.name.as_ref() == "minecraft:cactus" {
        visual.faces
    } else {
        let faces: &[BlockFace] = match record.name.as_ref() {
            "minecraft:bamboo" => &[BlockFace::North, BlockFace::East],
            "minecraft:azalea" | "minecraft:flowering_azalea" => {
                &[BlockFace::Down, BlockFace::South, BlockFace::West]
            }
            "minecraft:crimson_roots" | "minecraft:warped_roots" => &[BlockFace::South],
            _ => &[cross_texture_face(record)],
        };
        let mut materials = [0; 6];
        for &face in faces {
            let Some(material) = inputs.material(record, face) else {
                return Ok(());
            };
            materials[face as usize] = material;
        }
        materials
    };
    let key = (visual.model_template, materials);
    if let Some(&template) = cache.get(&key) {
        visual.model_template = template;
        return Ok(());
    }
    let quads = if record.name.as_ref() == "minecraft:bamboo" {
        bamboo_quads(materials)
    } else if matches!(
        record.name.as_ref(),
        "minecraft:azalea" | "minecraft:flowering_azalea"
    ) {
        azalea_quads(materials)
    } else if record.name.as_ref() == "minecraft:cactus" {
        let mut quads = vanilla_cuboid_quads(materials, [96, 0, 96], [160, 192, 160]);
        for quad in &mut quads {
            for position in &mut quad.positions {
                position[1] += 64;
            }
        }
        quads.to_vec()
    } else {
        let face = if matches!(
            record.name.as_ref(),
            "minecraft:crimson_roots" | "minecraft:warped_roots"
        ) {
            BlockFace::South
        } else {
            cross_texture_face(record)
        };
        potted_cross(materials[face as usize], 192)
    };
    let world = storage.templates[visual.model_template as usize];
    // Keep the paired world template's existing span: its quads remain byte-identical.
    let world_quads =
        storage.quads[world.quad_start as usize..][..world.quad_count as usize].to_vec();
    push_model_template(
        quads,
        assets::MODEL_TEMPLATE_FLAG_POTTED_PLANT,
        storage.templates,
        storage.quads,
    )?;
    let template = push_model_template(world_quads, world.flags, storage.templates, storage.quads)?;
    cache.insert(key, template);
    visual.model_template = template;
    Ok(())
}

/// Two unextruded, double-sided diagonal planes rooted at the pot's soil surface.
fn potted_cross(material: u32, height: i16) -> Vec<ModelQuad> {
    [
        [
            [36, 64, 36],
            [220, 64, 220],
            [220, 64 + height, 220],
            [36, 64 + height, 36],
        ],
        [
            [220, 64, 36],
            [36, 64, 220],
            [36, 64 + height, 220],
            [220, 64 + height, 36],
        ],
    ]
    .map(|positions| ModelQuad {
        positions,
        uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    })
    .to_vec()
}

/// Potted bamboo uses a thin stem and one side leaf with independent sprite regions.
fn bamboo_quads(materials: [u32; 6]) -> Vec<ModelQuad> {
    let stem = materials[BlockFace::North as usize];
    let mut quads = vanilla_cuboid_quads([stem; 6], [112, 0, 112], [144, 192, 144]);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        quad.uvs = quad.positions.map(|[x, y, z]| match face {
            BlockFace::Up => [3328 + (x - 112) as u16 * 16, (z - 112) as u16 * 16],
            BlockFace::Down => [3328 + (x - 112) as u16 * 16, 1024 + (144 - z) as u16 * 16],
            _ => {
                let u = match face {
                    BlockFace::East => 144 - z,
                    BlockFace::North => 144 - x,
                    BlockFace::West => z - 112,
                    _ => x - 112,
                };
                [1536 + u as u16 * 16, 3072 - y as u16 * 16]
            }
        });
        for position in &mut quad.positions {
            position[1] += 64;
        }
    }
    let mut quads = quads.to_vec();
    quads.push(ModelQuad {
        positions: [
            [144, 288, 128],
            [240, 288, 128],
            [240, 32, 128],
            [144, 32, 128],
        ],
        uvs: [[1792, 0], [256, 0], [256, 4096], [1792, 4096]],
        material: materials[BlockFace::East as usize],
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    });
    quads
}

/// Potted azalea selects its dedicated stem, canopy sides and top; the canopy has no bottom.
fn azalea_quads(materials: [u32; 6]) -> Vec<ModelQuad> {
    let mut quads = potted_cross(materials[BlockFace::West as usize], 256);
    let mut faces = [materials[BlockFace::South as usize]; 6];
    faces[BlockFace::Up as usize] = materials[BlockFace::Down as usize];
    for (face, mut quad) in
        BlockFace::ALL
            .into_iter()
            .zip(vanilla_cuboid_quads(faces, [64, 0, 64], [192, 192, 192]))
    {
        if face == BlockFace::Down {
            continue;
        }
        for position in &mut quad.positions {
            position[1] += 64;
        }
        quads.push(quad);
    }
    quads
}
