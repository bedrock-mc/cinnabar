//! Optional compatibility check against the private catalog, without vendored production assets.
use super::*;
use cosmetics::Catalog;

#[test]
fn api_catalog_assets_admit_through_client_validation() {
    let Some(root) = std::env::var_os("CINNABAR_COSMETICS_CHECKOUT")
        .or_else(|| std::env::var_os("CINNABAR_API_CHECKOUT"))
        .map(std::path::PathBuf::from)
    else {
        eprintln!(
            "missing-fixture: private cosmetics checkout; set CINNABAR_COSMETICS_CHECKOUT to run catalog compatibility"
        );
        return;
    };
    let catalog: Catalog =
        serde_json::from_slice(&std::fs::read(root.join("src/catalog.json")).unwrap()).unwrap();
    assert!(!catalog.items.is_empty(), "API catalog must contain items");
    for descriptor in catalog.items {
        let assets = &descriptor.assets;
        let read = |asset: &cosmetics::Asset| {
            assert!(
                asset.sha256.len() == 64
                    && asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid asset digest for {}",
                descriptor.id
            );
            std::fs::read(root.join("assets/cosmetics").join(&asset.sha256)).unwrap()
        };
        let geometry = read(&assets.geometry);
        let texture = read(&assets.texture);
        let animation = assets.animation.as_ref().map(read).unwrap_or_default();
        let id = descriptor.id.clone();
        let emissive = descriptor.render_flags.emissive;
        let item = admit(descriptor, &geometry, &texture, &animation)
            .unwrap_or_else(|| panic!("{id} must admit with the API catalog digests"));
        let model = rig(item);
        assert_eq!(model.emissive(), emissive, "{id}");
        assert!(!model.rest_vertices.is_empty(), "{id}");
        let root = RenderBoneTransform {
            rotation: glam::Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: crate::UNIT_AXIS_SCALE,
        };
        let pose = model.pose(root, Motion::default());
        assert_eq!(pose.len(), model.geometry.bone_pivots.len(), "{id}");
        assert!(pose.iter().all(|bone| bone.is_finite()), "{id}");

        assert!(
            thumbnail::render(item, 64, 64).is_some_and(|image| image
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|texel| texel[3] > 0)),
            "{id} needs visible gallery artwork"
        );
    }
}
