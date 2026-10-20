//! Small synthetic fabric for client and presentation regression tests.
use super::Item;

/// Admits a small synthetic fabric through the public asset contract.
pub(super) fn admit(columns: usize, rows: usize) -> Item {
    use sha2::{Digest, Sha256};
    let texture = include_bytes!("../bunny_ears.png");
    let image = image::load_from_memory(texture).unwrap();
    let points: Vec<_> = (0..=rows)
        .flat_map(|y| {
            (0..=columns).map(move |x| {
                [
                    x as f32 * 10.0 / columns as f32 - 5.0,
                    23.8 - y as f32 * 17.0 / rows as f32,
                    3.2,
                ]
            })
        })
        .collect();
    let mut polygons = Vec::new();
    for y in 0..rows {
        for x in 0..columns {
            let a = y * (columns + 1) + x;
            polygons.push([a, a + columns + 1, a + columns + 2, a + 1].map(|i| [i, 0, i]));
        }
    }
    let geometry=serde_json::to_vec(&serde_json::json!({"format_version":"1.12.0", "minecraft:geometry":[{
        "description":{"identifier":"geometry.fixture.cloth","texture_width":image.width(),"texture_height":image.height()},
        "bones":[{"name":"body","pivot":[0,24,0]}, {"name":"fabric","parent":"body","pivot":[0,23.8,3.2],
        "poly_mesh":{"normalized_uvs":true,"positions":points,"normals":[[0,0,1]],"uvs":vec![[0.0,0.0];(columns+1)*(rows+1)],"polys":polygons}}]
    }]})).unwrap();
    let animation = serde_json::to_vec(
        &serde_json::json!({"cloth":{"bone":"fabric","columns":columns,"rows":rows}}),
    )
    .unwrap();
    let asset = |bytes: &[u8]| cosmetics::Asset {
        url: "/fixture".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len(),
    };
    let mut descriptor = crate::hats::fixture::bunny().descriptor().clone();
    descriptor.id = format!("fixture:continuous_cloth_{columns}_{rows}");
    descriptor.slot = "back".into();
    descriptor.attachment_bone = "body".into();
    descriptor.geometry_id = "geometry.fixture.cloth".into();
    descriptor.assets = cosmetics::Assets {
        geometry: asset(&geometry),
        texture: asset(texture),
        animation: Some(asset(&animation)),
    };
    crate::hats::admit(descriptor, &geometry, texture, &animation).unwrap()
}
