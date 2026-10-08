use super::*;

fn assert_not_baby_gate(compiled: &assets::CompiledEntityAssets, weight: Option<u32>) {
    let expression = &compiled.molang_expressions[weight.expect("native baby gate") as usize];
    let ops = &compiled.molang_ops[expression.first_op as usize
        ..expression.first_op as usize + usize::from(expression.op_count)];
    assert!(ops.windows(2).any(|pair| match pair {
        [assets::MolangOp::LoadQuery(symbol), assets::MolangOp::Not] => {
            compiled.molang_symbols[*symbol as usize]
                .identifier
                .as_ref()
                == "query.is_baby"
        }
        _ => false,
    }));
}

fn collision_pack(format_version: &str, explicit_move: bool) -> TempDir {
    let pack = synthetic_pack();
    let mut description = serde_json::json!({
        "identifier":"minecraft:allay",
        "textures":{"default":"textures/entity/allay/allay"},
        "geometry":{"default":"geometry.allay"},
        "animations":{"move":"animation.allay.move"},
        "animation_controllers":[{"move":"controller.animation.allay.general"}],
        "render_controllers":["controller.render.allay"]
    });
    if explicit_move {
        description["scripts"] = serde_json::json!({"animate":[{"move":"query.is_baby"}]});
    }
    write(
        pack.path(),
        "entity/allay.entity.json",
        serde_json::json!({
            "format_version":format_version,
            "minecraft:client_entity":{"description":description}
        })
        .to_string()
        .as_bytes(),
    );
    write(
        pack.path(),
        "animations/allay.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.allay.move":{
            "loop":true,"bones":{"root":{"position":[0,"-9-this",0]}}
        }}}"#,
    );
    write(
        pack.path(),
        "animation_controllers/allay.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{
            "controller.animation.allay.general":{"states":{"default":{
                "animations":[{"move":"!query.is_baby"}]
            }}}
        }}"#,
    );
    pack
}

#[test]
fn legacy_controller_same_named_clip_cannot_bypass_its_baby_gate() {
    let pack = collision_pack("1.8.0", false);
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = &compiled.rig_bindings[0];
    assert_eq!(rig.fallback, assets::EntityRigFallback::Skip);
    let geometry = &compiled.rig_geometries[rig.first_geometry as usize];
    assert_eq!(geometry.animation_count, 0, "no unconditional body clip");
    assert_eq!(geometry.controller_count, 1);
    let binding = &compiled.rig_controllers[geometry.first_controller as usize];
    let controller = &compiled.controllers[binding.controller as usize];
    let state = &compiled.controller_states[controller.first_state as usize];
    let animation = &compiled.controller_animations[state.first_animation as usize];
    assert_not_baby_gate(&compiled, animation.weight);
    let assets::EntityControllerAnimationTarget::Clip(clip) = animation.target else {
        panic!("controller plays its same-named clip")
    };
    assert_eq!(
        compiled.animation_clips[clip as usize].geometry,
        Some(geometry.geometry)
    );
    assert_eq!(
        compiled.symbols[compiled.animation_clips[clip as usize].symbol as usize]
            .identifier
            .as_ref(),
        "animation.allay.move"
    );
}

#[test]
fn explicitly_animated_clip_and_same_named_legacy_controller_keep_separate_bindings() {
    let pack = collision_pack("1.10.0", true);
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let geometry = &compiled.rig_geometries[compiled.rig_bindings[0].first_geometry as usize];
    assert_eq!(geometry.animation_count, 1);
    assert_eq!(geometry.controller_count, 1);
    let direct = &compiled.rig_animations[geometry.first_animation as usize];
    let controller = &compiled.rig_controllers[geometry.first_controller as usize];
    assert!(direct.weight.is_some());
    assert_ne!(direct.name, controller.name);
}

#[test]
#[ignore = "requires CINNABAR_VANILLA_ROOT pointing to the pinned downloaded resource pack"]
fn pinned_polar_bear_geometries_only_activate_the_native_controller_not_the_move_clip() {
    let root = std::path::PathBuf::from(std::env::var_os("CINNABAR_VANILLA_ROOT").unwrap());
    let compiled = compile_entity_assets(&root, MANIFEST).unwrap();
    let rig = compiled
        .rig_bindings
        .iter()
        .find(|rig| {
            compiled.symbols[rig.entity_symbol as usize]
                .identifier
                .as_ref()
                == "minecraft:polar_bear"
        })
        .unwrap();
    for geometry in compiled
        .rig_geometries
        .iter()
        .skip(rig.first_geometry as usize)
        .take(usize::from(rig.geometry_count))
    {
        assert_eq!(
            geometry.animation_count, 0,
            "only controller-gated clips may run"
        );
        assert_eq!(geometry.controller_count, 1);
        let binding = &compiled.rig_controllers[geometry.first_controller as usize];
        let controller = &compiled.controllers[binding.controller as usize];
        let state = &compiled.controller_states[controller.first_state as usize];
        let move_clip = compiled
            .controller_animations
            .iter()
            .skip(state.first_animation as usize)
            .take(usize::from(state.animation_count))
            .find(|animation| match animation.target {
                assets::EntityControllerAnimationTarget::Clip(index) => {
                    compiled.symbols[compiled.animation_clips[index as usize].symbol as usize]
                        .identifier
                        .as_ref()
                        == "animation.polarbear.move"
                }
                _ => false,
            })
            .unwrap();
        assert_not_baby_gate(&compiled, move_clip.weight);
    }
}
