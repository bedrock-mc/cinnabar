use super::*;

const TEXTURE: &str = "textures/entity/iron_golem";

/// The samples' iron golem: its body controller is referenced but not defined anywhere.
fn golem_pack(define_body: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(root, "entity/iron_golem.entity.json", format!(r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:iron_golem","materials":{{"default":"iron_golem"}},"textures":{{"default":"{TEXTURE}"}},"geometry":{{"default":"geometry.irongolem"}},"render_controllers":["controller.render.iron_golem","controller.render.iron_golem_cracks"]}}}}}}"#).as_bytes());
    write(root, "models/entity/iron_golem.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.irongolem","texture_width":16,"texture_height":16},"bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[4,4,4],"uv":[0,0]}]}]}]}"#);
    write(
        root,
        "animations/empty.json",
        br#"{"format_version":"1.8.0","animations":{}}"#,
    );
    write(
        root,
        "animation_controllers/empty.json",
        br#"{"format_version":"1.10.0","animation_controllers":{}}"#,
    );
    write(root, "render_controllers/default.render_controllers.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.default":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#);
    let body = if define_body {
        r#""controller.render.iron_golem":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]},"#
    } else {
        ""
    };
    write(root, "render_controllers/iron_golem.render_controllers.json", format!(r#"{{"format_version":"1.8.0","render_controllers":{{{body}"controller.render.iron_golem_cracks":{{"geometry":"Geometry.default","materials":[{{"*":"Material.default"}}],"textures":["Texture.default"]}}}}}}"#).as_bytes());
    fs::create_dir_all(root.join("textures/entity")).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([180, 170, 160, 255]))
        .save(root.join(format!("{TEXTURE}.png")))
        .unwrap();
    temporary
}

fn synthetic_controllers(pack: &TempDir) -> usize {
    compile_entity_assets(pack.path(), MANIFEST)
        .unwrap()
        .sources
        .iter()
        .filter(|source| {
            source
                .path
                .starts_with("render_controllers/_cinnabar/omitted/")
        })
        .count()
}

#[test]
fn an_omitted_body_controller_is_completed_from_the_pack_default_and_draws() {
    let pack = golem_pack(false);
    assert_eq!(synthetic_controllers(&pack), 1);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
}

#[test]
fn a_pack_that_defines_the_body_controller_keeps_its_own() {
    let pack = golem_pack(true);
    assert_eq!(synthetic_controllers(&pack), 0);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 1);
}

#[test]
fn pinned_samples_still_omit_the_policy_controllers_and_every_mob_rig_draws() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT") else {
        eprintln!("missing fixture: CINNABAR_VANILLA_ACTOR_ROOT pinned vanilla pack");
        return;
    };
    let root = Path::new(&root);
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let completed = entities
        .sources
        .iter()
        .filter(|source| {
            source
                .path
                .starts_with("render_controllers/_cinnabar/omitted/")
        })
        .count();
    assert_eq!(
        completed,
        assets::facts::pack::rules()
            .omitted_render_controllers()
            .count(),
        "a policy-listed controller is now defined by the pack; drop its rule"
    );
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let texture_fallbacks: Vec<_> = compiled
        .report
        .fallbacks
        .iter()
        .filter(|fallback| fallback.reason.as_ref() != "no_render_layer")
        .map(|fallback| {
            let rig = &entities.rig_bindings[fallback.rig as usize];
            (
                entities.symbols[rig.entity_symbol as usize]
                    .identifier
                    .clone(),
                fallback.reason.clone(),
            )
        })
        .collect();
    assert!(texture_fallbacks.is_empty(), "{texture_fallbacks:?}");
    // Every texture a controller can select is built, so no variant silently draws another.
    let runtime_entities =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entities).unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(&compiled.bytes, &runtime_entities).unwrap();
    let built: std::collections::BTreeSet<u32> = catalog
        .textures()
        .iter()
        .map(|texture| texture.source)
        .collect();
    let unbuilt: std::collections::BTreeSet<_> = entities
        .render
        .candidates
        .iter()
        .filter(|candidate| !built.contains(&candidate.source))
        .map(|candidate| &entities.sources[candidate.source as usize].path)
        .filter(|path| path.starts_with("textures/"))
        .collect();
    assert!(unbuilt.is_empty(), "{unbuilt:?}");
    for mob in [
        "minecraft:iron_golem",
        "minecraft:spider",
        "minecraft:slime",
    ] {
        let rig = entities
            .rig_bindings
            .iter()
            .position(|rig| {
                entities.symbols[rig.entity_symbol as usize]
                    .identifier
                    .as_ref()
                    == mob
            })
            .unwrap_or_else(|| panic!("{mob} has no rig"));
        assert!(
            compiled.report.bindings > 0
                && !compiled
                    .report
                    .fallbacks
                    .iter()
                    .any(|f| f.rig as usize == rig),
            "{mob}"
        );
    }
}
