use super::*;

fn base(seed: u8) -> Box<[u8]> {
    let mut bytes = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let offset = ((y * TILE_SIZE + x) * 4) as usize;
            bytes[offset..offset + 4].copy_from_slice(&[
                seed,
                seed,
                seed,
                if x < TILE_SIZE / 2 { 255 } else { 0 },
            ]);
        }
    }
    bytes.into_boxed_slice()
}

fn page(bases: &[Box<[u8]>]) -> TexturePage {
    let chains = bases
        .iter()
        .map(|base| assets::build_texture_mip_chain(base.clone(), TILE_SIZE).unwrap())
        .collect::<Vec<_>>();
    TexturePage::new(TextureArray {
        layers: bases.len() as u32,
        mips: (0..MIP_COUNT as usize)
            .map(|level| TextureMip {
                size: TILE_SIZE >> level,
                rgba8: chains
                    .iter()
                    .flat_map(|chain| chain[level].rgba8.iter().copied())
                    .collect(),
            })
            .collect(),
    })
}

fn material(texture: TextureRef, world: bool) -> Material {
    Material {
        texture,
        flags: if world {
            MATERIAL_FLAG_NATIVE_LEAF_COLOUR
        } else {
            0
        },
        ..Material::unvaried()
    }
}

fn layer(page: &TexturePage, texture: TextureRef, level: usize) -> &[u8] {
    let mip = &page.texture.mips[level];
    let length = (mip.size * mip.size * 4) as usize;
    let offset = texture.layer() as usize * length;
    &mip.rgba8[offset..offset + length]
}

#[test]
fn world_leaf_mip_copies_dedupe_without_mutating_carried_or_other_materials() {
    let old = TextureRef::DIAGNOSTIC;
    let original = page(&[base(60), base(60)]);
    let mut materials = [
        material(old, false),
        material(old, true),
        material(old, true),
        material(TextureRef::new(0, 1).unwrap(), true),
    ];
    let result = install(
        &mut materials,
        vec![original.clone()].into(),
        Box::default(),
        Box::default(),
    )
    .unwrap();
    assert_eq!(materials[0].texture, old);
    assert_eq!(materials[1].texture, materials[2].texture);
    assert_eq!(materials[1].texture, materials[3].texture);
    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.pages[0].texture.layers, original.texture.layers + 1);
    let expected = build_legacy_terrain_mip_chain(&base(60), TILE_SIZE).unwrap();
    for (level, old_mip) in original.texture.mips.iter().enumerate() {
        assert_eq!(
            &result.pages[0].texture.mips[level].rgba8[..old_mip.rgba8.len()],
            old_mip.rgba8.as_ref()
        );
        assert_eq!(
            layer(&result.pages[0], materials[1].texture, level),
            expected[level].rgba8.as_ref()
        );
    }
}

#[test]
fn world_leaf_animation_copies_keep_original_timeline_and_dedupe_repeated_frames() {
    let original = page(&[base(60), base(90)]);
    let refs = [
        TextureRef::new(0, 0).unwrap(),
        TextureRef::new(0, 1).unwrap(),
    ];
    let timeline = vec![refs[0], refs[1], refs[0]];
    let animation = Animation {
        frame_start: 0,
        frame_count: timeline.len() as u32,
        ticks_per_frame: 3,
        atlas_index: 5,
        atlas_tile_variant: 1,
        replicate: 1,
        flags: 1,
    };
    let mut materials = [
        material(refs[0], false),
        material(refs[0], true),
        material(refs[0], true),
    ];
    for material in &mut materials {
        material.animation = 0;
    }
    let result = install(
        &mut materials,
        vec![original].into(),
        vec![animation].into(),
        timeline.clone().into(),
    )
    .unwrap();
    assert_eq!(materials[0].animation, 0);
    assert_eq!(materials[1].animation, materials[2].animation);
    assert_ne!(materials[1].animation, 0);
    assert_eq!(result.animations.len(), 2);
    assert_eq!(result.animations[0], animation);
    assert_eq!(&result.frames[..timeline.len()], timeline.as_slice());
    let copy = result.animations[materials[1].animation as usize];
    assert_eq!(copy.frame_start, timeline.len() as u32);
    assert_eq!(copy.frame_count, animation.frame_count);
    assert_eq!(copy.ticks_per_frame, animation.ticks_per_frame);
    assert_eq!(copy.flags, animation.flags);
    let copied = &result.frames[copy.frame_start as usize..];
    assert_eq!(copied[0], materials[1].texture);
    assert_eq!(copied[0], copied[2]);
    assert_ne!(copied[0], copied[1]);
    assert_eq!(result.pages[0].texture.layers, 4);
}

#[test]
fn world_leaf_mip_copies_roll_over_to_the_second_page_and_fail_at_total_capacity() {
    let mut full = page(&[base(60)]);
    full.texture.layers = MAX_TEXTURE_LAYERS as u32;
    for mip in &mut full.texture.mips {
        let one = mip.rgba8.clone();
        mip.rgba8 = (0..MAX_TEXTURE_LAYERS)
            .flat_map(|_| one.iter().copied())
            .collect();
    }
    let mut materials = [material(TextureRef::DIAGNOSTIC, true)];
    let result = install(
        &mut materials,
        vec![full.clone()].into(),
        Box::default(),
        Box::default(),
    )
    .unwrap();
    assert_eq!(result.pages.len(), MAX_TEXTURE_PAGES);
    assert_eq!(materials[0].texture, TextureRef::new(1, 0).unwrap());
    assert_eq!(result.pages[0], full);
    let mut materials = [material(TextureRef::DIAGNOSTIC, true)];
    assert!(
        install(
            &mut materials,
            vec![full; MAX_TEXTURE_PAGES].into(),
            Box::default(),
            Box::default()
        )
        .is_err()
    );
}

#[test]
fn world_leaf_mip_copies_reject_invalid_references_without_panicking() {
    let pages = vec![page(&[base(60)])].into_boxed_slice();
    let mut materials = [material(TextureRef::new(1, 0).unwrap(), true)];
    assert!(
        install(
            &mut materials,
            pages.clone(),
            Box::default(),
            Box::default()
        )
        .is_err()
    );
    let mut materials = [material(TextureRef::DIAGNOSTIC, true)];
    materials[0].animation = 1;
    assert!(install(&mut materials, pages, Box::default(), Box::default()).is_err());
}
