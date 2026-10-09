use std::{fs, path::Path};

use assets::{
    EntityAnimationInterpolation, EntityAnimationLoop, EntityAnimationProperty, EntityRigFallback,
    MolangOp, MolangSymbolKind, encode_entity_blob,
};
use pack_compiler::{compile_entity_assets, compile_entity_assets_with_report};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");

#[test]
fn entity_rotation_frames_survive_position_only_and_frame_only_bones() {
    let pack = animation_pack(false);
    let original = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(
        original
            .animation_channels
            .iter()
            .all(|channel| !channel.rotation_relative_to_entity)
    );
    let path = pack.path().join("animations/test.animation.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let root = &mut value["animations"]["animation.test.walk"]["bones"]["root"];
    root["relative_to"] = serde_json::json!({"rotation":"entity"});
    // No angle channel: the translated bone still switches to entity axes.
    let compiled = compile_entity_assets_after_write(pack.path(), &path, &value);
    let root_channels: Vec<_> = compiled
        .animation_channels
        .iter()
        .filter(|channel| channel.bone == 0)
        .collect();
    assert!(
        root_channels
            .iter()
            .any(|channel| channel.property == EntityAnimationProperty::Translation)
    );
    assert!(
        root_channels
            .iter()
            .all(|channel| channel.rotation_relative_to_entity)
    );
    assert!(
        compiled
            .animation_channels
            .iter()
            .filter(|channel| channel.bone != 0)
            .all(|channel| !channel.rotation_relative_to_entity)
    );
    value["animations"]["animation.test.walk"]["bones"]["root"] =
        serde_json::json!({"relative_to":{"rotation":"entity"}});
    let compiled = compile_entity_assets_after_write(pack.path(), &path, &value);
    let frame = compiled
        .animation_channels
        .iter()
        .find(|channel| channel.bone == 0)
        .unwrap();
    assert!(frame.rotation_relative_to_entity);
    let runtime =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    assert_eq!(
        runtime.animation_channels(),
        compiled.animation_channels.as_ref()
    );
}

fn compile_entity_assets_after_write(
    root: &Path,
    path: &Path,
    value: &serde_json::Value,
) -> assets::CompiledEntityAssets {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    compile_entity_assets(root, MANIFEST).unwrap()
}

#[test]
fn modern_player_scripts_activate_only_animate_roots_and_compile_rig_scripts() {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["format_version"] = serde_json::json!("1.26.0");
    value["minecraft:client_entity"]["description"]["identifier"] =
        serde_json::json!("minecraft:player");
    value["minecraft:client_entity"]["description"]["scripts"] = serde_json::json!({
        "scale": "0.9375",
        "initialize": ["variable.example=0;"],
        "pre_animation": ["variable.tcos0 = Math.cos(query.modified_distance_moved * 38.17);"],
        "animate": [{"walk": "query.is_moving"}]
    });
    value["minecraft:client_entity"]["description"]
        .as_object_mut()
        .unwrap()
        .remove("animation_controllers");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = compiled.rig_bindings[0];
    assert!(rig.initialize.is_some() && rig.pre_animation.is_some());
    assert_eq!(rig.scale.get(), 0.9375);
    assert_eq!(rig.fallback, assets::EntityRigFallback::Skip);
    for candidate in &compiled.rig_geometries {
        assert_eq!(candidate.animation_count, 1);
        assert_eq!(candidate.controller_count, 0);
    }
    let walk = compiled.rig_animations[0];
    assert!(
        walk.weight.is_some(),
        "a conditional root carries its weight"
    );
    assert!(
        compiled.molang_ops.contains(&MolangOp::StoreVariable(
            compiled
                .molang_symbols
                .iter()
                .position(|symbol| symbol.identifier.as_ref() == "variable.tcos0")
                .unwrap() as u32
        ))
    );
}

#[test]
fn modern_alias_lookup_alone_does_not_activate_and_explicit_roots_are_not_subtracted() {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let description = value["minecraft:client_entity"]["description"]
        .as_object_mut()
        .unwrap();
    description.remove("animation_controllers");
    description.remove("scripts");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(!compiled.animation_clips.is_empty());
    for candidate in &compiled.rig_geometries {
        assert_eq!(candidate.animation_count, 0);
        assert_eq!(candidate.controller_count, 0);
    }
    value["minecraft:client_entity"]["description"]["scripts"] =
        serde_json::json!({"animate":["walk","main"]});
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let explicit = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for candidate in &explicit.rig_geometries {
        assert_eq!(candidate.animation_count, 1);
        assert_eq!(candidate.controller_count, 1);
    }
}

fn clip_target(animation: &assets::EntityControllerAnimation) -> u32 {
    match animation.target {
        assets::EntityControllerAnimationTarget::Clip(clip) => clip,
        assets::EntityControllerAnimationTarget::Controller(_) => panic!("expected a clip"),
    }
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn selectable_geometry_pack(index: &str, members: &[&str]) -> TempDir {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/test.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.a"},"bones":[{"name":"root"},{"name":"arm"}]},{"description":{"identifier":"geometry.b"},"bones":[{"name":"arm"},{"name":"root"}]}]}"#,
    );
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.a","alternate":"geometry.b"},"animations":{"move":"animation.test.walk"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move"]}}}}"#,
    );
    let controller = serde_json::json!({
        "format_version": "1.8.0",
        "render_controllers": {
            "controller.render.test": {
                "arrays": {"geometries": {"Array.test": members}},
                "geometry": format!("Array.test[{index}]")
            }
        }
    });
    write(
        pack.path(),
        "render_controllers/test.render_controllers.json",
        &serde_json::to_vec(&controller).unwrap(),
    );
    pack
}

fn evaluate_selection_expression(
    compiled: &assets::CompiledEntityAssets,
    expression: u32,
    query_value: f32,
) -> f32 {
    let expression = compiled.molang_expressions[expression as usize];
    let operations = &compiled.molang_ops[expression.first_op as usize
        ..(expression.first_op + u32::from(expression.op_count)) as usize];
    let mut stack = Vec::<f32>::new();
    for operation in operations {
        match operation {
            MolangOp::Push(value) => stack.push(value.get()),
            MolangOp::LoadQuery(_) => stack.push(query_value),
            MolangOp::Call(function) => {
                let arguments = stack.split_off(stack.len() - function.arity());
                stack.push(assets::molang_call(*function, &arguments, &mut || 0.0));
            }
            MolangOp::Equal => {
                let right = stack.pop().unwrap();
                let left = stack.pop().unwrap();
                stack.push(f32::from(left == right));
            }
            operation => panic!("unexpected constant selection operation: {operation:?}"),
        }
    }
    assert_eq!(stack.len(), 1);
    stack[0]
}

fn selected_geometry(index: f32, members: &[&str]) -> Box<str> {
    let pack = selectable_geometry_pack("query.modified_move_speed", members);
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = compiled.rig_bindings[0];
    let candidates = &compiled.rig_geometries[rig.first_geometry as usize
        ..(rig.first_geometry + u32::from(rig.geometry_count)) as usize];
    let selected = candidates[1..]
        .iter()
        .find(|candidate| {
            evaluate_selection_expression(&compiled, candidate.condition.unwrap(), index) != 0.0
        })
        .unwrap_or(&candidates[0]);
    compiled.geometries[selected.geometry as usize]
        .identifier
        .clone()
}

fn animation_pack(reverse: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let files: [(&str, &[u8]); 7] = [
        (
            "entity/test.entity.json",
            br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"walk":"animation.test.walk","attack":"animation.test.attack","main":"controller.animation.test"},"animation_controllers":[{"main":"controller.animation.test"}],"scripts":{"animate":["main"]},"render_controllers":["controller.render.test"]}}}"#,
        ),
        (
            "models/entity/test.geo.json",
            br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":16,"texture_height":16},"bones":[{"name":"root","pivot":[0,0,0]},{"name":"arm","parent":"root","pivot":[1,2,3]}]}]}"#,
        ),
        (
            "animations/test.animation.json",
            br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"loop":true,"animation_length":1.0,"bones":{"root":{"position":{"0.0":[0,0,0],"1.0":[1,2,3]}}}},"animation.test.attack":{"loop":false,"animation_length":0.5,"bones":{"arm":{"rotation":{"0.0":{"pre":[0,0,0],"post":[0,10,0],"lerp_mode":"catmullrom"},"0.5":[0,30,0]}}}}}}"#,
        ),
        (
            "animation_controllers/test.animation_controllers.json",
            br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"initial_state":"default","states":{"default":{"animations":["animation.test.walk"],"transitions":[{"moving":"query.is_moving && variable.enabled"}]},"moving":{"animations":[{"animation.test.attack":"math.clamp(query.ground_speed, 0, 1)"}],"transitions":[{"default":"!query.is_moving"}]}}}}}"#,
        ),
        (
            "render_controllers/test.render_controllers.json",
            br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"arrays":{"geometries":{"Array.test":["Geometry.default","Geometry.default"]}},"geometry":"Array.test[math.floor(query.modified_move_speed)]","textures":["Texture.default"]}}}"#,
        ),
        ("textures/entity/test.png", b"synthetic-raster"),
        (
            "textures/entity/test.texture_set.json",
            br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"test"}}"#,
        ),
    ];
    let iterator: Box<dyn Iterator<Item = &(&str, &[u8])>> = if reverse {
        Box::new(files.iter().rev())
    } else {
        Box::new(files.iter())
    };
    for (path, bytes) in iterator {
        write(temporary.path(), path, bytes);
    }
    temporary
}

fn set_walk_time_update(pack: &Path, clock: serde_json::Value) {
    let path = pack.join("animations/test.animation.json");
    let mut animations: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    animations["animations"]["animation.test.walk"]["anim_time_update"] = clock;
    fs::write(path, serde_json::to_vec(&animations).unwrap()).unwrap();
}

#[test]
fn compiles_authored_distance_clock_and_preserves_it_in_entity_carrier() {
    let pack = animation_pack(false);
    set_walk_time_update(
        pack.path(),
        serde_json::json!("query.modified_distance_moved"),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let walk = compiled
        .animation_clips
        .iter()
        .find(|clip| {
            compiled.symbols[clip.symbol as usize].identifier.as_ref() == "animation.test.walk"
        })
        .unwrap();
    let clock = compiled.molang_expressions[walk.anim_time_update.unwrap() as usize];
    let distance_query = compiled
        .molang_symbols
        .iter()
        .position(|symbol| {
            symbol.kind == MolangSymbolKind::Query
                && symbol.identifier.as_ref() == "query.modified_distance_moved"
        })
        .unwrap() as u32;
    assert_eq!(
        &compiled.molang_ops
            [clock.first_op as usize..(clock.first_op + u32::from(clock.op_count)) as usize],
        &[MolangOp::LoadQuery(distance_query)]
    );
    assert!(
        compiled
            .animation_clips
            .iter()
            .any(|clip| clip.anim_time_update.is_none())
    );
    let blob = encode_entity_blob(&compiled).unwrap();
    let runtime = assets::RuntimeEntityAssets::decode(&blob).unwrap();
    assert_eq!(runtime.animation_clips(), compiled.animation_clips.as_ref());
    assert_eq!(runtime.encode().unwrap(), blob);
}

#[test]
fn animation_time_update_accepts_general_molang_and_numeric_constants() {
    for (authored, expected_ops) in [
        (serde_json::json!(0.75), 1),
        (serde_json::json!("query.anim_time + query.delta_time"), 3),
    ] {
        let pack = animation_pack(false);
        set_walk_time_update(pack.path(), authored.clone());
        let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
        let clock = compiled
            .animation_clips
            .iter()
            .find_map(|clip| clip.anim_time_update)
            .unwrap();
        let clock = compiled.molang_expressions[clock as usize];
        assert_eq!(clock.op_count, expected_ops, "authored clock {authored}");
        if authored.is_number() {
            assert_eq!(
                compiled.molang_ops[clock.first_op as usize],
                MolangOp::Push(assets::EntityGeometryScalar::new(0.75).unwrap())
            );
        }
    }
}

#[test]
fn rejects_malformed_or_unsupported_animation_time_updates() {
    for authored in [
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!("query.modified_distance_moved +"),
        serde_json::json!("query.unreviewed_time"),
    ] {
        let pack = animation_pack(false);
        set_walk_time_update(pack.path(), authored.clone());
        let error = compile_entity_assets(pack.path(), MANIFEST).unwrap_err();
        assert!(
            error.to_string().contains("anim_time_update"),
            "{authored}: {error}"
        );
    }
}

#[test]
fn compiles_clips_controllers_molang_and_collection_selection_deterministically() {
    let first = compile_entity_assets(animation_pack(false).path(), MANIFEST).unwrap();
    let second = compile_entity_assets(animation_pack(true).path(), MANIFEST).unwrap();
    assert_eq!(
        encode_entity_blob(&first).unwrap(),
        encode_entity_blob(&second).unwrap()
    );

    assert_eq!(first.animation_clips.len(), 2);
    assert_eq!(
        first.animation_clips[0].loop_mode,
        EntityAnimationLoop::Once
    );
    assert_eq!(
        first.animation_clips[1].loop_mode,
        EntityAnimationLoop::Loop
    );
    assert!(first.animation_channels.iter().any(|channel| {
        channel.property == EntityAnimationProperty::Rotation && channel.keyframe_count == 3
    }));
    assert!(first.animation_keyframes.iter().any(|frame| {
        frame.interpolation == EntityAnimationInterpolation::CatmullRom
            && frame.value[1].get() == 10.0
    }));
    assert_eq!(first.controllers.len(), 1);
    assert_eq!(first.controller_states.len(), 2);
    assert_eq!(
        first.controller_transitions.len(),
        2,
        "state cycles are bounded data"
    );
    assert_eq!(first.rig_bindings.len(), 1);
    assert_eq!(first.rig_bindings[0].fallback, EntityRigFallback::Skip);
    let rig = first.rig_bindings[0];
    assert_eq!(rig.geometry_count, 3);
    let candidates = &first.rig_geometries[rig.first_geometry as usize
        ..(rig.first_geometry + u32::from(rig.geometry_count)) as usize];
    assert!(candidates[0].condition.is_none());
    assert!(
        candidates[1..]
            .iter()
            .all(|candidate| candidate.condition.is_some())
    );
    assert!(first.molang_symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::Query && symbol.identifier.as_ref() == "query.is_moving"
    }));
    assert!(first.molang_symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::Variable
            && symbol.identifier.as_ref() == "variable.enabled"
    }));
    assert!(
        first
            .molang_ops
            .iter()
            .any(|op| matches!(op, MolangOp::JumpIfFalse(_)))
    );
    assert!(
        first
            .molang_ops
            .contains(&MolangOp::Call(assets::MolangFunction::Clamp))
    );
    assert!(first.molang_ops.contains(&MolangOp::Equal));
}

#[test]
fn geometry_selector_retains_all_forty_one_authored_choices() {
    let members = (0..41)
        .map(|index| {
            if index % 2 == 0 {
                "Geometry.alternate"
            } else {
                "Geometry.default"
            }
        })
        .collect::<Vec<_>>();
    let pack = selectable_geometry_pack("query.modified_move_speed", &members);
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = compiled.rig_bindings[0];
    let candidates = &compiled.rig_geometries[rig.first_geometry as usize
        ..(rig.first_geometry + u32::from(rig.geometry_count)) as usize];
    assert_eq!(candidates.len(), members.len() + 1);
    assert_ne!(rig.fallback, assets::EntityRigFallback::GeometryOnly);
    for (index, alias) in members.iter().enumerate() {
        let selected = candidates[1..]
            .iter()
            .filter(|candidate| {
                evaluate_selection_expression(&compiled, candidate.condition.unwrap(), index as f32)
                    != 0.0
            })
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 1, "selector index {index}");
        assert_eq!(
            compiled.geometries[selected[0].geometry as usize]
                .identifier
                .as_ref(),
            if *alias == "Geometry.alternate" {
                "geometry.b"
            } else {
                "geometry.a"
            }
        );
    }
}

#[test]
fn geometry_collection_clamps_negative_indices_before_selection() {
    assert_eq!(
        selected_geometry(-0.25, &["Geometry.alternate", "Geometry.default"]).as_ref(),
        "geometry.b"
    );
}

#[test]
fn geometry_collection_clamps_oversized_indices_before_selection() {
    assert_eq!(
        selected_geometry(99.0, &["Geometry.default", "Geometry.alternate"]).as_ref(),
        "geometry.b"
    );
}

#[test]
fn geometry_collection_floors_fractional_indices_before_selection() {
    assert_eq!(
        selected_geometry(0.75, &["Geometry.alternate", "Geometry.default"]).as_ref(),
        "geometry.b"
    );
}

#[test]
fn absent_named_geometry_collection_is_an_attributed_static_fallback() {
    let pack = selectable_geometry_pack("0", &["Geometry.default"]);
    write(
        pack.path(),
        "render_controllers/test.render_controllers.json",
        br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Array.absent[0]"}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.assets.rig_bindings.len(), 1);
    assert_eq!(
        compiled.assets.rig_bindings[0].fallback,
        EntityRigFallback::GeometryOnly
    );
    let entity_symbol = compiled
        .assets
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "minecraft:test")
        .unwrap() as u32;
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
            if *symbol == entity_symbol
    )));
}

#[test]
fn undefined_animation_reference_keeps_the_rig_as_a_static_fallback() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/rejected.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:rejected","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"required":"animation.missing"},"render_controllers":[{"controller.render.test":"query.unlisted"}],"scripts":{"animate":["required"]}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.rig_bindings.len(), 2);
}

#[test]
fn malformed_keyframes_non_finite_literals_and_unsupported_grammar_fail_closed() {
    for animation in [
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"bones":{"root":{"position":{"bad":[0,0,0]}}}}}}"#.as_slice(),
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"animation_length":"NaN"}}}"#.as_slice(),
    ] {
        let pack = animation_pack(false);
        write(pack.path(), "animations/test.animation.json", animation);
        assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
    }

    let pack = animation_pack(false);
    write(
        pack.path(),
        "animation_controllers/test.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"states":{"default":{"transitions":[{"default":"variable.x = 1"}]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_only_transition_dropped(&compiled);
    assert!(
        !compiled
            .assets
            .molang_ops
            .iter()
            .any(|operation| { matches!(operation, MolangOp::LoadVariable(_)) })
    );
}

fn assert_only_transition_dropped(compiled: &pack_compiler::EntityAssetCompilation) {
    assert!(!compiled.assets.controllers.is_empty());
    assert!(compiled.assets.controller_transitions.is_empty());
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            reason: pack_compiler::FallbackReason::UnsupportedOptionalExpression,
            ..
        }
    )));
}

#[test]
fn unlisted_query_in_optional_controller_is_attributed_as_fallback_not_bytecode() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animation_controllers/test.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"states":{"default":{"transitions":[{"default":"query.unlisted"}]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert!(
        !compiled
            .assets
            .molang_symbols
            .iter()
            .any(|symbol| symbol.identifier.as_ref() == "query.unlisted")
    );
    assert_only_transition_dropped(&compiled);
}

#[test]
fn accepted_molang_surface_compiles_every_query_operator_and_fixed_arity_function() {
    let pack = animation_pack(false);
    let expressions = [
        "query.is_on_ground ? query.anim_time : query.life_time",
        "-query.modified_move_speed + query.ground_speed",
        "query.is_on_ground && query.is_moving || query.is_sprinting",
        "query.is_riding",
        "query.is_sneaking == query.is_sleeping",
        "query.body_y_rotation != query.head_y_rotation",
        "query.target_x_rotation < 1",
        "query.anim_time <= 1",
        "query.life_time > 0",
        "query.ground_speed >= 0",
        "variable.speed + temp.scratch",
        "query.anim_time - query.life_time",
        "query.anim_time * query.life_time",
        "query.anim_time / query.life_time",
        "!query.is_moving",
        "math.abs(query.body_y_rotation)",
        "math.ceil(query.anim_time)",
        "math.floor(query.anim_time)",
        "math.round(query.anim_time)",
        "math.sqrt(query.anim_time)",
        "math.sin(query.body_y_rotation)",
        "math.cos(query.body_y_rotation)",
        "math.min(query.anim_time, query.life_time)",
        "math.max(query.anim_time, query.life_time)",
        "math.clamp(query.anim_time, 0, 1)",
        "math.lerp(query.anim_time, query.life_time, 0.5)",
        "1 / 0 + math.mod(1, 0)",
        "variable.speed ?? 1",
        "query.get_equipped_item_name == 'bow'",
        "query.is_moving ? 1",
        "math.ease_in_out_back(0, 1, query.anim_time)",
    ];
    let transitions = expressions
        .iter()
        .map(|expression| serde_json::json!({"default": expression}))
        .collect::<Vec<_>>();
    let controller = serde_json::json!({
        "format_version": "1.10.0",
        "animation_controllers": {
            "controller.animation.test": {
                "states": {"default": {"transitions": transitions}}
            }
        }
    });
    write(
        pack.path(),
        "animation_controllers/test.animation_controllers.json",
        &serde_json::to_vec(&controller).unwrap(),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.controller_transitions.len(), expressions.len());
    use assets::MolangFunction as F;
    for operation in [
        MolangOp::Add,
        MolangOp::Subtract,
        MolangOp::Multiply,
        MolangOp::Divide,
        MolangOp::Negate,
        MolangOp::Not,
        MolangOp::Truthy,
        MolangOp::Equal,
        MolangOp::NotEqual,
        MolangOp::Less,
        MolangOp::LessEqual,
        MolangOp::Greater,
        MolangOp::GreaterEqual,
        MolangOp::Call(F::Abs),
        MolangOp::Call(F::Ceil),
        MolangOp::Call(F::Floor),
        MolangOp::Call(F::Round),
        MolangOp::Call(F::Sqrt),
        MolangOp::Call(F::Sin),
        MolangOp::Call(F::Cos),
        MolangOp::Call(F::Min),
        MolangOp::Call(F::Max),
        MolangOp::Call(F::Clamp),
        MolangOp::Call(F::Lerp),
    ] {
        assert!(
            compiled.molang_ops.contains(&operation),
            "missing {operation:?}"
        );
    }
    for present in [
        |op: &MolangOp| matches!(op, MolangOp::Coalesce(_)),
        |op: &MolangOp| matches!(op, MolangOp::PushString(_)),
        |op: &MolangOp| matches!(op, MolangOp::JumpIfTrue(_)),
        |op: &MolangOp| matches!(op, MolangOp::Call(F::Ease(..))),
    ] {
        assert!(compiled.molang_ops.iter().any(present));
    }
    assert!(
        compiled
            .molang_ops
            .iter()
            .any(|operation| matches!(operation, MolangOp::Push(value) if value.get() == 0.0))
    );
}

#[test]
fn forms_vanilla_rejects_leave_only_that_transition_out() {
    for expression in [
        "variable.x = 1",
        "loop(2, 1)",
        "return 1",
        "variable['dynamic']",
        "query.not_a_vanilla_query",
        "math.not_a_function(1)",
        "math.sin(1, 2)",
        "break;",
        "return 1; return 2;",
        "v.a->v.b->v.c",
        "1 % 2",
        "'unterminated",
    ] {
        let pack = animation_pack(false);
        let controller = serde_json::json!({
            "format_version": "1.10.0",
            "animation_controllers": {
                "controller.animation.test": {
                    "states": {"default": {"transitions": [{"default": expression}]}}
                }
            }
        });
        write(
            pack.path(),
            "animation_controllers/test.animation_controllers.json",
            &serde_json::to_vec(&controller).unwrap(),
        );
        let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
        assert!(
            compiled.controller_transitions.is_empty(),
            "unexpected support for {expression}"
        );
        assert!(!compiled.controllers.is_empty());
    }
}

#[test]
fn conflicting_animation_aliases_are_resolved_inside_each_entity_environment() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.walk","main":"controller.animation.test"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move","main"]}}}}"#,
    );
    write(
        pack.path(),
        "entity/second.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:second","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.attack","main":"controller.animation.second"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move","main"]}}}}"#,
    );
    write(
        pack.path(),
        "animation_controllers/test.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"states":{"default":{"animations":["move"]}}},"controller.animation.second":{"states":{"default":{"animations":["move"]}}}}}"#,
    );

    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.controllers.len(), 2);
    assert_eq!(compiled.rig_bindings.len(), 2);
    let clip_symbols = compiled
        .controller_animations
        .iter()
        .map(|binding| compiled.animation_clips[clip_target(binding) as usize].symbol)
        .map(|symbol| compiled.symbols[symbol as usize].identifier.as_ref())
        .collect::<Vec<_>>();
    assert!(clip_symbols.contains(&"animation.test.walk"));
    assert!(clip_symbols.contains(&"animation.test.attack"));
}

#[test]
fn legacy_animation_controllers_bind_separately_from_animation_aliases() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.8.0","minecraft:client_entity":{"description":{"identifier":"minecraft:allay","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.walk"},"animation_controllers":[{"main":"controller.animation.test"}],"render_controllers":["controller.render.test"]}}}"#,
    );

    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.assets.controllers.len(), 1);
    assert_eq!(compiled.assets.rig_bindings.len(), 1);
    let rig = compiled.assets.rig_bindings[0];
    assert_eq!(
        compiled.assets.rig_geometries[rig.first_geometry as usize].controller_count,
        1
    );
    let controller_symbol = compiled
        .assets
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "controller.animation.test")
        .unwrap() as u32;
    assert!(!compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
            if *symbol == controller_symbol
    )));
}

#[test]
fn explicit_default_geometry_wins_over_alphabetically_earlier_optional_alias() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/test.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.player"},"bones":[{"name":"player_root"}]},{"description":{"identifier":"geometry.test"},"bones":[{"name":"root"},{"name":"arm","parent":"root"}]}]}"#,
    );
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"aaa_optional":"geometry.player","default":"geometry.test"},"animations":{"walk":"animation.test.walk"},"render_controllers":["controller.render.test"],"scripts":{"animate":["walk"]}}}}"#,
    );

    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = &compiled.rig_bindings[0];
    let candidate = &compiled.rig_geometries[rig.first_geometry as usize];
    assert_eq!(
        compiled.geometries[candidate.geometry as usize]
            .identifier
            .as_ref(),
        "geometry.test"
    );
}

#[test]
fn inherited_geometry_clips_use_parent_order_and_child_overlays() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/test.geo.json",
        br#"{"format_version":"1.8.0","geometry.base":{"bones":[{"name":"root"},{"name":"arm","parent":"root"}]},"geometry.child:geometry.base":{"bones":[{"name":"arm","parent":"root"},{"name":"wing","parent":"arm"}]}}"#,
    );
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.child"},"animations":{"move":"animation.test.walk"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move"]}}}}"#,
    );
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"bones":{"root":{"position":[1,0,0]},"arm":{"position":[2,0,0]},"wing":{"position":[3,0,0]}}}}}"#,
    );

    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let candidate = compiled.rig_geometries[compiled.rig_bindings[0].first_geometry as usize];
    let clip = &compiled.animation_clips
        [compiled.rig_animations[candidate.first_animation as usize].clip as usize];
    let channels = &compiled.animation_channels
        [clip.first_channel as usize..(clip.first_channel + clip.channel_count) as usize];
    assert_eq!(
        channels
            .iter()
            .map(|channel| channel.bone)
            .collect::<Vec<_>>(),
        vec![1, 0, 2]
    );
}

#[test]
fn animation_bones_are_numbered_in_the_selected_geometry_not_global_order() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/test.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.a"},"bones":[{"name":"root"},{"name":"arm"}]},{"description":{"identifier":"geometry.b"},"bones":[{"name":"arm"},{"name":"root"}]}]}"#,
    );
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.a"},"animations":{"move":"animation.test.walk"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move"]}}}}"#,
    );
    write(
        pack.path(),
        "entity/second.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:second","geometry":{"default":"geometry.b"},"animations":{"move":"animation.test.second"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move"]}}}}"#,
    );
    write(
        pack.path(),
        "animations/test.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"bones":{"root":{"position":[1,0,0]}}},"animation.test.second":{"bones":{"root":{"position":[2,0,0]}}}}}"#,
    );

    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let root_bones = compiled
        .rig_bindings
        .iter()
        .map(|rig| {
            let candidate = &compiled.rig_geometries[rig.first_geometry as usize];
            let binding = &compiled.rig_animations[candidate.first_animation as usize];
            compiled.animation_channels
                [compiled.animation_clips[binding.clip as usize].first_channel as usize]
                .bone
        })
        .collect::<Vec<_>>();
    assert_eq!(
        root_bones,
        vec![1, 0],
        "rig order is symbol-sorted; each clip must use its rig geometry's local root index"
    );
}

#[test]
fn selectable_geometries_own_specialized_clips_and_controllers() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/test.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.a"},"bones":[{"name":"root"},{"name":"arm"}]},{"description":{"identifier":"geometry.b"},"bones":[{"name":"arm"},{"name":"root"}]}]}"#,
    );
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.a","alternate":"geometry.b"},"animations":{"move":"animation.test.walk","attack":"animation.test.attack","main":"controller.animation.test"},"render_controllers":["controller.render.test"],"scripts":{"animate":["move","main"]}}}}"#,
    );
    write(
        pack.path(),
        "render_controllers/test.render_controllers.json",
        br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"arrays":{"geometries":{"Array.test":["Geometry.default","Geometry.alternate"]}},"geometry":"Array.test[math.floor(query.modified_move_speed)]"}}}"#,
    );

    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let rig = compiled.rig_bindings[0];
    let candidates = &compiled.rig_geometries[rig.first_geometry as usize
        ..(rig.first_geometry + u32::from(rig.geometry_count)) as usize];
    let alternate = candidates
        .iter()
        .find(|candidate| {
            compiled.geometries[candidate.geometry as usize]
                .identifier
                .as_ref()
                == "geometry.b"
        })
        .unwrap();
    let direct_clip = compiled.rig_animations[alternate.first_animation as usize
        ..alternate.first_animation as usize + alternate.animation_count as usize]
        .iter()
        .find(|binding| {
            compiled.molang_symbols[binding.name as usize]
                .identifier
                .as_ref()
                == "move"
        })
        .unwrap()
        .clip;
    assert_eq!(
        compiled.animation_channels
            [compiled.animation_clips[direct_clip as usize].first_channel as usize]
            .bone,
        1
    );
    let rig_controller = compiled.rig_controllers[alternate.first_controller as usize].controller;
    let controller = compiled.controllers[rig_controller as usize];
    let state = compiled.controller_states[controller.first_state as usize];
    let controller_clip =
        clip_target(&compiled.controller_animations[state.first_animation as usize]);
    assert_eq!(
        compiled.animation_channels
            [compiled.animation_clips[controller_clip as usize].first_channel as usize]
            .bone,
        1
    );
}

#[test]
fn duplicate_geometry_identifiers_reject_the_ambiguous_rig_without_collapsing_sources() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "models/entity/duplicate.geo.json",
        br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.test"},"bones":[{"name":"other"}]}]}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert!(compiled.assets.rig_bindings.is_empty());
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::RequiredRigRejected {
            reason: pack_compiler::RejectReason::AmbiguousGeometryReference,
            ..
        }
    )));
}

#[test]
fn duplicate_animation_identifiers_reject_the_ambiguous_rig_without_collapsing_sources() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/duplicate.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"bones":{"root":{"position":[9,0,0]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::RequiredRigRejected {
            reason: pack_compiler::RejectReason::AmbiguousAnimationReference,
            ..
        }
    )));
}

#[test]
fn unsupported_optional_assets_are_present_in_the_attribution_ledger() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animations/unsupported.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.unsupported":{"bones":{"root":{"rotation":["query.anim_time",0,0]}}}}}"#,
    );
    write(
        pack.path(),
        "animation_controllers/unreferenced.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.unreferenced":{"states":{"default":{}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    let unsupported_symbol = compiled
        .assets
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "animation.test.unsupported")
        .unwrap() as u32;
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
            if *symbol == unsupported_symbol
    )));
    let controller_symbol = compiled
        .assets
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "controller.animation.unreferenced")
        .unwrap() as u32;
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            symbol,
            reason: pack_compiler::FallbackReason::UnreferencedDefinition,
            ..
        } if *symbol == controller_symbol
    )));
}
