use super::*;

#[test]
fn player_pack_attack_roots_keep_custom_bones_and_engine_swing_reads() {
    let pack = tempfile::tempdir().unwrap();
    let files = [
        (
            "entity/player.entity.json",
            serde_json::json!({
                "format_version": "1.26.0",
                "minecraft:client_entity": {"description": {
                    "identifier": "minecraft:player",
                    "textures": {"default": "textures/entity/player"},
                    "geometry": {"default": "geometry.player"},
                    "animations": {"attack": "animation.player.attack.rotations"},
                    "scripts": {"animate": [{"attack": "variable.attack_time > 0 && !variable.is_first_person"}]},
                    "render_controllers": ["controller.render.player"]
                }}
            }),
        ),
        (
            "models/entity/player.geo.json",
            serde_json::json!({
                "format_version": "1.12.0",
                "minecraft:geometry": [{
                    "description": {"identifier": "geometry.player", "texture_width": 16, "texture_height": 16},
                    "bones": [{"name": "strike_arm", "pivot": [0, 0, 0]}]
                }]
            }),
        ),
        (
            "animations/player.animation.json",
            serde_json::json!({
                "format_version": "1.8.0",
                "animations": {"animation.player.attack.rotations": {
                    "loop": true,
                    "bones": {"strike_arm": {"rotation": ["variable.attack_time * 90 + query.camera_rotation(0)", 0, 0]}}
                }}
            }),
        ),
        (
            "render_controllers/player.json",
            serde_json::json!({
                "format_version": "1.8.0",
                "render_controllers": {"controller.render.player": {
                    "geometry": "Geometry.default", "textures": ["Texture.default"]
                }}
            }),
        ),
    ];
    for (path, value) in files {
        write(pack.path(), path, &serde_json::to_vec(&value).unwrap());
    }
    write(
        pack.path(),
        "textures/entity/player.png",
        b"synthetic-raster",
    );
    // A resource pack always carries the controller family, even when this entity uses none.
    fs::create_dir_all(pack.path().join("animation_controllers")).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = compiled.rig_bindings[0];
    assert_eq!(
        compiled.symbols[rig.entity_symbol as usize]
            .identifier
            .as_ref(),
        "minecraft:player"
    );
    let geometry = compiled.rig_geometries[rig.first_geometry as usize];
    assert_eq!(geometry.animation_count, 1);
    let binding = compiled.rig_animations[geometry.first_animation as usize];
    assert!(binding.weight.is_some());
    let clip = &compiled.animation_clips[binding.clip as usize];
    assert_eq!(
        compiled.symbols[clip.symbol as usize].identifier.as_ref(),
        "animation.player.attack.rotations"
    );
    let channel = &compiled.animation_channels[clip.first_channel as usize];
    let bones = &compiled.geometries[geometry.geometry as usize].bones;
    assert_eq!(bones[channel.bone as usize].name.as_ref(), "strike_arm");
    assert_eq!(channel.property, EntityAnimationProperty::Rotation);
    assert!(compiled.molang_ops.iter().any(|op| matches!(op, MolangOp::LoadVariable(symbol) if compiled.molang_symbols[*symbol as usize].identifier.as_ref() == "variable.attack_time")));
    assets::RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap();
}
