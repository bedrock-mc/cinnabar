use assets::{MATERIAL_FLAG_ALPHA_BLEND, TextureArray, TextureMip, TextureRef};

use super::*;

const SIDE: u32 = 16;

/// One stair step: the lower tread's top face and the riser above it, in 1/256 block units.
fn stair_quads(material: u32) -> Vec<ModelQuad> {
    let uvs = [[0, 0], [0, 4096], [4096, 4096], [4096, 0]];
    let quad = |positions| ModelQuad {
        positions,
        uvs,
        material,
        flags: 0,
    };
    vec![
        quad([[0, 128, 0], [0, 128, 256], [256, 128, 256], [256, 128, 0]]),
        quad([
            [0, 128, 128],
            [256, 128, 128],
            [256, 256, 128],
            [0, 256, 128],
        ]),
    ]
}

fn material(flags: u32) -> Material {
    Material {
        texture: TextureRef::new(0, 0).unwrap(),
        flags,
        animation: 0,
        variation_start: 0,
        variation_count: 0,
        variation_weight: 0,
    }
}

fn page() -> TexturePage {
    TexturePage::new(TextureArray {
        layers: 1,
        mips: vec![TextureMip {
            size: SIDE,
            rgba8: vec![200; (SIDE * SIDE * 4) as usize].into(),
        }]
        .into(),
    })
}

fn stair<'a>(
    quads: &'a [ModelQuad],
    templates: &'a [ModelTemplate],
    materials: &'a [Material],
    pages: &'a [TexturePage],
) -> Shape<'a> {
    Shape {
        kind: VisualKind::Model,
        faces: [0; 6],
        template: Some(0),
        templates,
        quads,
        materials,
        pages,
    }
}

#[test]
fn a_stair_item_draws_its_template_quads_instead_of_the_thumbnail() {
    let quads = stair_quads(1);
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: quads.len() as u32,
        flags: 0,
    }];
    let materials = [material(0), material(0)];
    let pages = [page()];
    let mut atlas = Atlas::new(7, 1);
    let mesh = shaped(&stair(&quads, &templates, &materials, &pages), &mut atlas)
        .expect("a stair item becomes GUI geometry");
    assert_eq!(mesh.vertices().len(), quads.len() * 4);
    assert_eq!(mesh.indices().len(), quads.len() * 6);
    assert!(
        mesh.batches()
            .iter()
            .all(|batch| batch.texture_page == 7 && batch.depth_test)
    );
    // The tile is the material's full-resolution texels, not projected thumbnail pixels.
    let (pages, _) = atlas.finish().unwrap();
    let texel = pages[0].pixels();
    assert!(texel.chunks_exact(4).any(|pixel| pixel == [200; 4]));
}

#[test]
fn blended_or_missing_materials_keep_the_thumbnail() {
    let templates = [ModelTemplate {
        quad_start: 0,
        quad_count: 2,
        flags: 0,
    }];
    let materials = [
        material(0),
        material(0),
        material(MATERIAL_FLAG_ALPHA_BLEND),
    ];
    let pages = [page()];
    for material in [2, 9] {
        let quads = stair_quads(material);
        let mut atlas = Atlas::new(7, 1);
        assert!(shaped(&stair(&quads, &templates, &materials, &pages), &mut atlas).is_none());
    }
    // A template index past the table is no shape at all.
    let quads = stair_quads(1);
    let mut shape = stair(&quads, &templates, &materials, &pages);
    shape.template = Some(4);
    assert!(shaped(&shape, &mut Atlas::new(7, 1)).is_none());
}
