//! Bounded pack reads for the shared geometry compiler.
use pack_compiler::block_geometry::{
    BlockGeometryCatalog, Geometry, MAX_GEOMETRY_FILE_BYTES, MAX_GEOMETRY_FILES,
};
use resource_pack::{LayeredPackView, normalize_jsonc};
use std::collections::{HashMap, HashSet};

/// Admits winning models and parent definitions before compiling requested block models.
pub(super) fn geometry_catalog(
    view: &LayeredPackView,
    wanted: &HashSet<&str>,
) -> HashMap<String, Geometry> {
    geometry_catalog_with_budget(view, wanted, pack_compiler::MAX_PACK_ENTITY_BYTES)
}

/// Compiles requested models within the caller's total read budget.
fn geometry_catalog_with_budget(
    view: &LayeredPackView,
    wanted: &HashSet<&str>,
    mut remaining: usize,
) -> HashMap<String, Geometry> {
    if wanted.is_empty() {
        return HashMap::new();
    }
    let files = view
        .list_with_suffixes("models/", &[".json"])
        .into_iter()
        .take(MAX_GEOMETRY_FILES)
        .collect::<Vec<_>>();
    let mut catalog = BlockGeometryCatalog::default();
    // Later layers and file names win, so read them before spending the budget.
    for layer in view.layers().rev() {
        for path in files.iter().rev() {
            let Ok(Some(bytes)) = layer.read_file_with_limit(path, MAX_GEOMETRY_FILE_BYTES as u64)
            else {
                continue;
            };
            let Some(next) = remaining.checked_sub(bytes.len()) else {
                return catalog.compile(wanted);
            };
            remaining = next;
            let Some(bytes) = normalize_jsonc(&bytes) else {
                continue;
            };
            let Ok(root) = serde_json::from_slice(&bytes) else {
                continue;
            };
            catalog.insert_fallback(&root);
        }
    }
    catalog.compile(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Admits authored model documents as a low-to-high stack of resource packs.
    fn model_stack(layers: &[&[(&str, &[u8])]]) -> LayeredPackView {
        let archives = layers.iter().enumerate().map(|(index, files)| {
            let id = format!("00000000-0000-0000-0000-{:012}", index + 1);
            let manifest = format!(r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#);
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (path, bytes) in std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied()) {
                writer.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
                writer.write_all(bytes).unwrap();
            }
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(), "1.0.0".into(), String::new(), writer.finish().unwrap().into_inner(),
            )
        }).collect();
        LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(archives),
        ))
    }

    /// Creates a legacy model with one observable cube height and an optional parent.
    fn model(identifier: &str, y: i32) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({identifier: {
            "bones":[{"name":"root","cubes":[{"origin":[-8,y,-8],"size":[16,1,16],"uv":[0,0]}]}]
        }}))
        .unwrap()
    }

    #[test]
    fn budget_exhaustion_preserves_the_upper_layer_geometry() {
        let lower = model("geometry.model", 0);
        let upper = model("geometry.model", 8);
        let unrelated = vec![b' '; lower.len()];
        let view = model_stack(&[
            &[("models/a.json", &lower), ("models/b.json", &unrelated)],
            &[("models/a.json", &upper)],
        ]);
        let compiled =
            geometry_catalog_with_budget(&view, &HashSet::from(["geometry.model"]), upper.len());
        assert_eq!(compiled["geometry.model"].cubes[0].min[1], 8.0);
    }

    #[test]
    fn budget_exhaustion_preserves_an_override_at_a_different_path() {
        let lower = model("geometry.model", 0);
        let upper = model("geometry.model", 8);
        let view = model_stack(&[&[("models/a.json", &lower)], &[("models/z.json", &upper)]]);
        let wanted = HashSet::from(["geometry.model"]);
        let compiled = geometry_catalog_with_budget(&view, &wanted, upper.len());
        assert_eq!(compiled["geometry.model"].cubes[0].min[1], 8.0);
        let compiled = geometry_catalog(&view, &wanted);
        assert_eq!(compiled["geometry.model"].cubes[0].min[1], 8.0);
    }

    #[test]
    fn later_files_in_one_layer_keep_their_precedence() {
        let earlier = model("geometry.model", 0);
        let later = model("geometry.model", 8);
        let view = model_stack(&[&[("models/a.json", &earlier), ("models/z.json", &later)]]);
        for budget in [later.len(), earlier.len() + later.len()] {
            let compiled =
                geometry_catalog_with_budget(&view, &HashSet::from(["geometry.model"]), budget);
            assert_eq!(compiled["geometry.model"].cubes[0].min[1], 8.0);
        }
    }

    #[test]
    fn an_unread_upper_layer_model_does_not_reveal_the_lower_definition() {
        let lower = model("geometry.model", 0);
        let mut upper = model("geometry.model", 8);
        upper.extend_from_slice(b"  ");
        let view = model_stack(&[&[("models/a.json", &lower)], &[("models/z.json", &upper)]]);
        assert!(
            geometry_catalog_with_budget(&view, &HashSet::from(["geometry.model"]), lower.len())
                .is_empty()
        );
    }

    #[test]
    fn upper_layer_child_can_resolve_a_lower_layer_parent() {
        let parent = model("geometry.base", 0);
        let child = model("geometry.child:geometry.base", 8);
        let view = model_stack(&[&[("models/a.json", &parent)], &[("models/z.json", &child)]]);
        let compiled = geometry_catalog_with_budget(
            &view,
            &HashSet::from(["geometry.child"]),
            parent.len() + child.len(),
        );
        assert_eq!(
            compiled["geometry.child"]
                .cubes
                .iter()
                .map(|cube| cube.min[1])
                .collect::<Vec<_>>(),
            [0.0, 8.0]
        );
    }
}
