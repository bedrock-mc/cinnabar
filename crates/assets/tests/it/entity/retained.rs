use std::sync::Arc;

use assets::compiled::{RetainedBuffers, retained_bytes};
use assets::{RuntimeEntityAssets, SessionEntityPack};

#[test]
fn retained_entity_catalogs_include_animation_programs_and_geometry() {
    let animation = RuntimeEntityAssets::from_compiled(super::suite::carrier_v4_fixture()).unwrap();
    let geometry =
        RuntimeEntityAssets::from_compiled(super::suite::inherited_geometry_fixture()).unwrap();
    for assets in [animation, geometry] {
        let pack = SessionEntityPack {
            assets: Arc::new(assets),
            textures: Arc::from([]),
            bindings: Arc::from([]),
            equipment: None,
        };
        let mut buffers = Vec::new();
        pack.visit_buffers(&mut |address, bytes| buffers.push((address, bytes)));
        let included = |address: *const u8, bytes| {
            assert!(bytes > 0);
            assert!(
                buffers.contains(&(address, bytes)),
                "catalog buffer was omitted"
            );
        };
        let assets = &pack.assets;
        for (address, bytes) in [
            (
                assets.animation_keyframes().as_ptr().cast(),
                size_of_val(assets.animation_keyframes()),
            ),
            (
                assets.molang_ops().as_ptr().cast(),
                size_of_val(assets.molang_ops()),
            ),
        ] {
            if bytes > 0 {
                included(address, bytes);
            }
        }
        for geometry in assets.geometries() {
            for bone in &geometry.bones {
                if !bone.cubes.is_empty() {
                    included(bone.cubes.as_ptr().cast(), size_of_val(bone.cubes.as_ref()));
                }
            }
        }
        let bytes = retained_bytes(&[&pack]);
        assert!(bytes > 0, "a catalog with no textures still retains memory");
        let shared = pack.clone();
        assert_eq!(retained_bytes(&[&pack, &shared]), bytes);
    }
}
