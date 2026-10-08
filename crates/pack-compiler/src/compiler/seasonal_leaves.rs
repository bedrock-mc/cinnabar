//! World-only seasonal leaf materials. The ordinary face table remains usable
//! by carried block icons and viewmodels without a terrain exposure context.

use super::*;
use assets::{
    BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF, BLOCK_VISUAL_VARIANT_SEASONAL_LEAF,
    MATERIAL_FLAG_EXPOSED_FOLIAGE, MATERIAL_FLAG_NATIVE_LEAF_COLOUR,
    MATERIAL_FLAG_SEASONAL_FOLIAGE, MATERIAL_FLAG_TWO_SIDED, SEASONAL_LEAF_MATERIAL_COUNT,
};

pub(super) fn install(
    records: &[RegistryRecord],
    blocks: &crate::pack::BlockTextureMap,
    visuals: &mut [BlockVisual],
    materials: &mut Vec<Material>,
) -> Result<Vec<(u32, u32)>, AssetError> {
    let mut groups = BTreeMap::<
        (
            [u32; BlockFace::ALL.len()],
            bool,
            [u32; BlockFace::ALL.len()],
        ),
        u32,
    >::new();
    let mut copies = Vec::new();
    // Selector and texture-copy identities must not depend on input order.
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|record| record.sequential_id);
    for record in ordered {
        let visual = &mut visuals[record.sequential_id as usize];
        if visual.kind != VisualKind::Diagnostic && native_seasonal_replaceable(&record.name) {
            visual.flags.insert(BlockFlags::SEASONAL_REPLACEABLE);
        }
        if visual.kind != VisualKind::Cube || !visual.flags.contains(BlockFlags::LEAF_MODEL) {
            continue;
        }
        let seasonal = leaf_tint_flags(&record.name) != 0;
        let metadata = blocks.leaf_world_face_flags(record)?;
        let key = (visual.faces, seasonal, metadata);
        let base = if let Some(&base) = groups.get(&key) {
            base
        } else {
            let material_count = SEASONAL_LEAF_MATERIAL_COUNT as usize;
            let mut selected = Vec::with_capacity(material_count);
            let mut selected_sources = Vec::with_capacity(material_count);
            // Vanilla deep leaves take an opaque material without changing the
            // fancy texture. Season-agnostic leaves also choose layer5/7 by depth, but never seasonal layer9/10.
            // Its exposure halves share one layout, without palette flags.
            let layers = [(false, false), (false, true), (true, false), (true, true)];
            for (deep, exposed) in layers {
                let flags = if seasonal {
                    MATERIAL_FLAG_SEASONAL_FOLIAGE
                } else {
                    0
                } | if seasonal && exposed {
                    MATERIAL_FLAG_EXPOSED_FOLIAGE
                } else {
                    0
                };
                for (&id, &metadata) in visual.faces.iter().zip(&metadata) {
                    let mut material = materials[id as usize];
                    material.flags = world_flags(material.flags, flags | metadata, deep);
                    if material.variation_count != 0 {
                        let start = material.variation_start as usize;
                        let end = start + material.variation_count as usize;
                        let mut variants = materials[start..end].to_vec();
                        if materials.len() + variants.len() + material_count > MAX_MATERIALS {
                            return Err(AssetError::InvalidCompiledAssets {
                                detail: "seasonal leaf variations exceed the material table bound"
                                    .into(),
                            });
                        }
                        for variant in &mut variants {
                            variant.flags = world_flags(variant.flags, flags | metadata, deep);
                        }
                        material.variation_start = materials.len() as u32;
                        copies.extend((start..end).enumerate().map(|(offset, original)| {
                            (material.variation_start + offset as u32, original as u32)
                        }));
                        materials.extend(variants);
                    }
                    selected.push(material);
                    selected_sources.push(id);
                }
            }
            if materials.len() + selected.len() > MAX_MATERIALS {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: "seasonal leaf materials exceed the material table bound".into(),
                });
            }
            let base = materials.len() as u32;
            copies.extend(
                selected_sources
                    .into_iter()
                    .enumerate()
                    .map(|(offset, original)| (base + offset as u32, original)),
            );
            materials.extend(selected);
            groups.insert(key, base);
            base
        };
        visual.variant = if seasonal {
            BLOCK_VISUAL_VARIANT_SEASONAL_LEAF
        } else {
            BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF
        } | base;
    }
    Ok(copies)
}

fn world_flags(original: u32, seasonal: u32, deep: bool) -> u32 {
    (original
        & !(MATERIAL_FLAG_ALPHA_BLEND
            | MATERIAL_FLAG_ALPHA_CUTOUT
            | MATERIAL_FLAG_TWO_SIDED
            | assets::MATERIAL_FLAG_ISOTROPIC
            | assets::MATERIAL_LEAF_METADATA_MASK))
        | seasonal
        | MATERIAL_FLAG_NATIVE_LEAF_COLOUR
        | if deep {
            0
        } else {
            MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_TWO_SIDED
        }
}

fn native_seasonal_replaceable(name: &str) -> bool {
    // Vanilla blocks registered as replaceable:
    // short_grass, fern, water,
    // flowing_water. Flowers/mushrooms are deliberately not
    // included: their crossed shape does not establish this component.
    matches!(
        name,
        "minecraft:short_grass" | "minecraft:fern" | "minecraft:water" | "minecraft:flowing_water"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::{SEASONAL_LEAF_DEEP_OFFSET, SEASONAL_LEAF_EXPOSED_OFFSET};

    fn empty_blocks() -> crate::pack::BlockTextureMap {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("textures")).unwrap();
        for (path, json) in [
            ("blocks.json", "{}"),
            ("textures/terrain_texture.json", r#"{"texture_data":{}}"#),
            ("textures/flipbook_textures.json", "[]"),
        ] {
            std::fs::write(directory.path().join(path), json).unwrap();
        }
        crate::pack::read_pack(directory.path()).unwrap().blocks
    }

    fn leaf(name: &str, id: u32) -> RegistryRecord {
        RegistryRecord {
            sequential_id: id,
            network_hash: id,
            name: name.into(),
            canonical_state: "{}".into(),
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
            contributor_role: ContributorRole::Primary,
            model_family: ModelFamily::Leaves,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: assets::RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn only_world_leaf_selector_uses_seasonal_materials_and_states_share_one_group() {
        let records = [
            leaf("minecraft:spruce_leaves", 0),
            leaf("minecraft:spruce_leaves", 1),
            leaf("minecraft:cherry_leaves", 2),
        ];
        let mut visuals = records
            .iter()
            .map(|record| {
                let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
                visual.kind = VisualKind::Cube;
                visual.faces = [1; BlockFace::ALL.len()];
                visual
            })
            .collect::<Vec<_>>();
        let mut materials = vec![Material::unvaried(); 2];
        materials[1].flags =
            MATERIAL_FLAG_ALPHA_CUTOUT | leaf_tint_flags("minecraft:spruce_leaves");
        install(&records, &empty_blocks(), &mut visuals, &mut materials).unwrap();
        assert_eq!(
            materials.len(),
            2 + SEASONAL_LEAF_MATERIAL_COUNT as usize * 2
        );
        assert_eq!(visuals[0].variant, visuals[1].variant);
        assert_eq!(visuals[0].variant, BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | 2);
        assert_eq!(
            visuals[2].variant,
            BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF | (2 + SEASONAL_LEAF_MATERIAL_COUNT)
        );
        assert_eq!(visuals[0].faces, [1; BlockFace::ALL.len()]);
        assert_eq!(materials[1].flags & MATERIAL_FLAG_SEASONAL_FOLIAGE, 0);
        assert_eq!(materials[1].flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
        for (index, material) in materials[2..2 + SEASONAL_LEAF_MATERIAL_COUNT as usize]
            .iter()
            .enumerate()
        {
            assert_ne!(material.flags & MATERIAL_FLAG_SEASONAL_FOLIAGE, 0);
            assert_ne!(material.flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
            assert_eq!(
                material.flags & MATERIAL_FLAG_EXPOSED_FOLIAGE != 0,
                index as u32 % SEASONAL_LEAF_DEEP_OFFSET >= SEASONAL_LEAF_EXPOSED_OFFSET
            );
            assert_eq!(material.texture, materials[1].texture);
            let deep = index as u32 >= SEASONAL_LEAF_DEEP_OFFSET;
            assert_eq!(material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0, !deep);
            assert_eq!(material.flags & MATERIAL_FLAG_TWO_SIDED != 0, !deep);
        }
        for (index, material) in materials[2 + SEASONAL_LEAF_MATERIAL_COUNT as usize..]
            .iter()
            .enumerate()
        {
            assert_eq!(material.flags & MATERIAL_FLAG_SEASONAL_FOLIAGE, 0);
            assert_ne!(material.flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
            assert_eq!(material.flags & MATERIAL_FLAG_EXPOSED_FOLIAGE, 0);
            let deep = index as u32 >= SEASONAL_LEAF_DEEP_OFFSET;
            assert_eq!(material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0, !deep);
            assert_eq!(material.flags & MATERIAL_FLAG_TWO_SIDED != 0, !deep);
            assert_eq!(material.texture, materials[1].texture);
        }
    }

    #[test]
    fn seasonal_replacement_is_exact_native_admission_without_variant_changes() {
        let cases = [
            ("minecraft:short_grass", VisualKind::Cross, true),
            ("minecraft:fern", VisualKind::Cross, true),
            ("minecraft:water", VisualKind::Liquid, true),
            ("minecraft:flowing_water", VisualKind::Liquid, true),
            ("minecraft:dandelion", VisualKind::Cross, false),
            ("minecraft:poppy", VisualKind::Cross, false),
            ("minecraft:brown_mushroom", VisualKind::Cross, false),
            ("example:fern", VisualKind::Cross, false),
            ("minecraft:fern", VisualKind::Diagnostic, false),
        ];
        let records = cases
            .iter()
            .enumerate()
            .map(|(id, &(name, _, _))| {
                let mut record = leaf(name, id as u32);
                record.flags = BlockFlags::empty();
                record
            })
            .collect::<Vec<_>>();
        let mut visuals = cases
            .iter()
            .map(|&(_, kind, _)| {
                let mut visual =
                    BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
                visual.kind = kind;
                // Liquid depth must remain a plain valid value, not a marker bit.
                visual.variant = 7;
                visual
            })
            .collect::<Vec<_>>();
        let mut materials = vec![Material::unvaried()];
        assert!(
            install(&records, &empty_blocks(), &mut visuals, &mut materials)
                .unwrap()
                .is_empty()
        );
        for (visual, &(name, _, expected)) in visuals.iter().zip(&cases) {
            assert_eq!(
                visual.flags.contains(BlockFlags::SEASONAL_REPLACEABLE),
                expected,
                "{name}"
            );
            assert_eq!(visual.variant, 7);
        }
        assert_eq!(materials.len(), 1);
    }

    #[test]
    fn texture_authored_agnostic_leaf_outer_and_deep_keep_no_palette_and_carried_faces() {
        let record = leaf("minecraft:cherry_leaves", 0);
        let mut visual = BlockVisual::diagnostic(record.flags, record.contributor_role);
        visual.kind = VisualKind::Cube;
        visual.faces = [1; BlockFace::ALL.len()];
        let mut visuals = [visual];
        let mut materials = vec![Material::unvaried(); 2];
        materials[1].flags = MATERIAL_FLAG_ALPHA_CUTOUT;
        let carried = materials[1];
        install(&[record], &empty_blocks(), &mut visuals, &mut materials).unwrap();
        assert_eq!(materials[1], carried);
        assert_eq!(visuals[0].faces, visual.faces);
        for (index, material) in materials[2..].iter().enumerate() {
            let deep = index as u32 >= SEASONAL_LEAF_DEEP_OFFSET;
            assert_ne!(material.flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
            assert_eq!(material.flags & assets::MATERIAL_FLAG_TINT_MASK, 0);
            assert_eq!(material.flags & MATERIAL_FLAG_SEASONAL_FOLIAGE, 0);
            assert_eq!(material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0, !deep);
            assert_eq!(material.flags & MATERIAL_FLAG_TWO_SIDED != 0, !deep);
        }
    }
}
