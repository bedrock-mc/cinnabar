//! Env-gated report over locally cached server packs: entity compile outcomes.

/// Env-gated: `CINNABAR_PACKCACHE_DIR` names a directory of cached `<uuid>_<version>.zip`
/// packs; prints which pack entities compile to artwork and why the rest fall back.
#[test]
fn report_local_pack_entities() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping report_local_pack_entities: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let mut zips = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .collect::<Vec<_>>();
    zips.sort();
    let refs = std::fs::read("../.local/assets/compiled/vanilla-v1.vanillarefs.json")
        .ok()
        .and_then(|bytes| assets::VanillaEntityRefs::from_json(&bytes));
    eprintln!("vanilla refs loaded: {}", refs.is_some());
    for path in zips {
        let Some(view) = super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let files = super::collect::collect_files(&view, refs.as_ref());
        let entity_files = files
            .iter()
            .filter(|(p, _)| p.starts_with("entity/"))
            .count();
        if entity_files == 0 {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        diagnose_references(&name, &files);
        match asset_compiler::compile_actor_pack(files) {
            Ok(Some(c)) => {
                let mut reasons = std::collections::BTreeMap::<String, Vec<String>>::new();
                for fallback in &c.fallbacks {
                    let rig = &c.entities.rig_bindings[fallback.rig as usize];
                    let id = &c.entities.symbols[rig.entity_symbol as usize].identifier;
                    reasons
                        .entry(fallback.reason.to_string())
                        .or_default()
                        .push(id.to_string());
                }
                let rigs = c.entities.rig_bindings.len();
                eprintln!(
                    "PACK {name}: entity_files={entity_files} rigs={rigs} artwork={} skipped={:?}",
                    c.bindings.len(),
                    c.skipped
                );
                for (reason, ids) in reasons {
                    eprintln!(
                        "  {reason}: {} e.g. {:?}",
                        ids.len(),
                        &ids[..ids.len().min(3)]
                    );
                }
            }
            Ok(None) => eprintln!("PACK {name}: entity_files={entity_files} compiled to nothing"),
            Err(error) => eprintln!("PACK {name}: entity_files={entity_files} ERROR {error}"),
        }
    }
}

/// Counts entity references the pack's own files cannot satisfy (vanilla-defined or missing).
fn diagnose_references(name: &str, files: &[(Box<str>, Vec<u8>)]) {
    use std::collections::{BTreeMap, BTreeSet};
    let json = |bytes: &[u8]| super::super::resource_packs::parse_pack_json(bytes);
    let mut defined = BTreeSet::new();
    let paths = files
        .iter()
        .map(|(p, _)| p.as_ref())
        .collect::<BTreeSet<_>>();
    for (path, bytes) in files {
        if path.starts_with("render_controllers/")
            && let Some(root) = json(bytes)
            && let Some(map) = root["render_controllers"].as_object()
        {
            defined.extend(map.keys().cloned());
        }
    }
    let mut missing_controllers = BTreeMap::<String, u32>::new();
    let mut missing_textures = BTreeMap::<String, u32>::new();
    for (path, bytes) in files {
        if !path.starts_with("entity/") {
            continue;
        }
        let Some(root) = json(bytes) else { continue };
        let description = &root["minecraft:client_entity"]["description"];
        for entry in description["render_controllers"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let key = entry
                .as_str()
                .map(str::to_owned)
                .or_else(|| entry.as_object()?.keys().next().cloned());
            if let Some(key) = key
                && !defined.contains(&key)
            {
                *missing_controllers.entry(key).or_default() += 1;
            }
        }
        for texture in description["textures"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(_, v)| v.as_str())
        {
            let present = [".png", ".tga"]
                .iter()
                .any(|ext| paths.contains(format!("{texture}{ext}").as_str()));
            if !present {
                let dir = texture.rsplit_once('/').map_or("", |(d, _)| d);
                *missing_textures.entry(dir.to_owned()).or_default() += 1;
            }
        }
    }
    eprintln!(
        "  refs {name}: undefined_controllers={missing_controllers:?} textures_not_collected_by_dir={missing_textures:?}"
    );
}
