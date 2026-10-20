use super::*;
use crate::entity::compile_entity_pack;
use image::{Rgba, RgbaImage};

/// Builds two valid rasters and an unusable one, drawn in a different order from source indices.
fn pack() -> crate::entity::EntityPackCompilation {
    let definition = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"test:staging","materials":{"default":"entity_alphatest"},"textures":{"first":"textures/entity/a","second":"textures/entity/b","bad":"textures/entity/c"},"geometry":{"default":"geometry.staging"},"render_controllers":["controller.render.staging"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.staging","texture_width":4,"texture_height":4},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[4,4,0],"uv":[0,0]}]}]}]}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.staging":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.second","Texture.first","Texture.bad"]}}}"#;
    let mut files = vec![
        ("entity/staging.json".into(), definition.to_vec()),
        ("models/entity/staging.geo.json".into(), geometry.to_vec()),
        (
            "render_controllers/staging.json".into(),
            controller.to_vec(),
        ),
    ];
    for (name, color) in [("a", [17, 23, 19, 255]), ("b", [29, 47, 5, 255])] {
        let mut bytes = Cursor::new(Vec::new());
        RgbaImage::from_pixel(4, 4, Rgba(color))
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        files.push((
            format!("textures/entity/{name}.png").into(),
            bytes.into_inner(),
        ));
    }
    files.push(("textures/entity/c.png".into(), b"unusable".to_vec()));
    compile_entity_pack(files).unwrap().unwrap()
}

#[test]
fn rig_order_and_one_read_per_source_survive_prepared_pixel_sharing() {
    let pack = pack();
    let runtime = assets::RuntimeEntityAssets::from_compiled(pack.assets.clone()).unwrap();
    let calls = std::sync::Mutex::new(BTreeMap::<u32, usize>::new());
    let build = build_artwork(
        &pack.assets,
        &runtime,
        &|source| {
            *calls.lock().unwrap().entry(source).or_default() += 1;
            Ok(pack.payloads[pack.assets.sources[source as usize].path.as_ref()].to_vec())
        },
        true,
    )
    .unwrap();
    assert_eq!(build.bindings.len(), 1);
    assert!(build.fallbacks.is_empty(), "{:?}", build.fallbacks);
    let paths = build
        .textures
        .iter()
        .map(|texture| pack.assets.sources[texture.source as usize].path.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["textures/entity/b.png", "textures/entity/a.png"]);
    assert_eq!(build.bindings[0].texture, 0);
    assert_eq!(build.pixel_bytes, 2 * 4 * 4 * 4);
    let calls = calls.into_inner().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.values().all(|&count| count == 1));
}

#[test]
fn a_failed_prepass_read_is_retried_at_the_owning_rig_and_still_reports_failure() {
    let pack = pack();
    let runtime = assets::RuntimeEntityAssets::from_compiled(pack.assets.clone()).unwrap();
    let retry = pack
        .assets
        .sources
        .iter()
        .position(|source| source.path.as_ref() == "textures/entity/b.png")
        .unwrap() as u32;
    for recover in [true, false] {
        let calls = std::sync::Mutex::new(BTreeMap::<u32, usize>::new());
        let result = build_artwork(
            &pack.assets,
            &runtime,
            &|source| {
                let mut calls = calls.lock().unwrap();
                let count = calls.entry(source).or_default();
                *count += 1;
                if source == retry && (*count == 1 || !recover) {
                    return Err(invalid("fixture read failure"));
                }
                Ok(pack.payloads[pack.assets.sources[source as usize].path.as_ref()].to_vec())
            },
            true,
        );
        assert_eq!(calls.lock().unwrap()[&retry], 2);
        if recover {
            let build = result.unwrap();
            assert_eq!(build.textures[0].source, retry);
            assert_eq!(build.bindings[0].texture, 0);
            assert!(build.fallbacks.is_empty());
        } else {
            assert!(
                matches!(result, Err(AssetError::InvalidCompiledAssets { detail }) if detail.contains("fixture read failure"))
            );
        }
    }
}

#[test]
fn unusable_sources_keep_the_owning_rig_fallback_and_are_not_read_again() {
    let pack = pack();
    let runtime = assets::RuntimeEntityAssets::from_compiled(pack.assets.clone()).unwrap();
    let calls = std::sync::Mutex::new(BTreeMap::<u32, usize>::new());
    let build = build_artwork(
        &pack.assets,
        &runtime,
        &|source| {
            *calls.lock().unwrap().entry(source).or_default() += 1;
            Ok(b"unusable image".to_vec())
        },
        true,
    )
    .unwrap();
    assert!(build.textures.is_empty());
    assert!(build.bindings.is_empty());
    assert_eq!(build.pixel_bytes, 0);
    assert_eq!(
        build.fallbacks,
        [ActorFallback {
            rig: 0,
            reason: "missing_or_ambiguous_texture".into(),
        }]
    );
    let calls = calls.into_inner().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.values().all(|&count| count == 1));
}

#[test]
fn existing_actor_fixture_is_byte_identical_on_one_and_four_threads() {
    let pack = pack();
    let runtime = assets::RuntimeEntityAssets::from_compiled(pack.assets.clone()).unwrap();
    let entity_bytes = encode_entity_blob(&pack.assets).unwrap();
    let compile = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let artwork = build_artwork(
                    &pack.assets,
                    &runtime,
                    &|source| {
                        Ok(
                            pack.payloads[pack.assets.sources[source as usize].path.as_ref()]
                                .as_ref(),
                        )
                    },
                    true,
                )
                .unwrap();
                assert_eq!(artwork.bindings.len(), 1);
                (
                    encode_actor_catalog(&entity_bytes, &artwork.textures, &artwork.bindings)
                        .unwrap(),
                    artwork.fallbacks,
                )
            })
    };
    assert_eq!(compile(1), compile(4));
}
