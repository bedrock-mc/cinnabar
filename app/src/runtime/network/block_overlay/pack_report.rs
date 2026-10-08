//! Env-gated report over locally cached server packs: block texture and geometry outcomes.

use std::collections::BTreeMap;

use super::{
    super::{
        local_pack::local_pack_view_at,
        resource_packs::{decode_pack_texture, texture_key_paths},
    },
    geometry::parse_geometry_file,
};

/// `CINNABAR_PACKCACHE_DIR` names a directory of cached `<uuid>_<version>.zip` packs.
#[test]
fn report_local_pack_blocks() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping report_local_pack_blocks: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let mut zips = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .collect::<Vec<_>>();
    zips.sort();
    for path in zips {
        let Some(view) = local_pack_view_at(&path) else {
            continue;
        };
        let keys = texture_key_paths(&view, "textures/terrain_texture.json");
        let mut geometries = 0;
        let (mut cubes, mut rotated, mut empty) = (0, 0, 0);
        for file in view.list("models/") {
            if !file.ends_with(".json") {
                continue;
            }
            let Some(bytes) = view.read(file) else {
                continue;
            };
            for (_, geometry) in parse_geometry_file(&bytes) {
                geometries += 1;
                cubes += geometry.cubes.len();
                rotated += geometry
                    .cubes
                    .iter()
                    .filter(|cube| !cube.rotations.is_empty())
                    .count();
                empty += usize::from(geometry.cubes.is_empty());
            }
        }
        if keys.is_empty() && geometries == 0 {
            continue;
        }
        let mut failed = BTreeMap::<&str, Vec<&str>>::new();
        let mut decoded = 0;
        for (key, texture_path) in &keys {
            let reason = if decode_pack_texture(&view, texture_path).is_some() {
                decoded += 1;
                continue;
            } else if ["png", "tga", "jpg"]
                .iter()
                .any(|ext| view.read(&format!("{texture_path}.{ext}")).is_some())
            {
                "present_but_undecodable"
            } else if view
                .read(&format!("{texture_path}.texture_set.json"))
                .is_some()
            {
                "texture_set_unresolved"
            } else {
                "file_missing"
            };
            failed.entry(reason).or_default().push(key);
        }
        eprintln!(
            "BLOCKS {}: terrain_keys={} decoded={decoded} geometries={geometries} cubes={cubes} rotated_cubes={rotated} empty_geometries={empty}",
            path.file_name().unwrap().to_string_lossy(),
            keys.len(),
        );
        for (reason, keys) in failed {
            eprintln!(
                "  {reason}: {} e.g. {:?}",
                keys.len(),
                &keys[..keys.len().min(3)]
            );
        }
    }
}
