use std::{fs, path::Path};

use asset_compiler::compile_entity_assets;
use assets::{
    EntityAnimationInterpolation, EntityAnimationLoop, EntityAnimationProperty, EntityRigFallback,
    MolangOp, MolangSymbolKind, encode_entity_blob,
};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../assets/vanilla-source.json");

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn animation_pack(reverse: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let files: [(&str, &[u8]); 7] = [
        (
            "entity/test.entity.json",
            br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"walk":"animation.test.walk","attack":"animation.test.attack","main":"controller.animation.test"},"animation_controllers":[{"main":"controller.animation.test"}],"render_controllers":["controller.render.test"]}}}"#,
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
    assert!(first.molang_symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::Query && symbol.identifier.as_ref() == "query.is_moving"
    }));
    assert!(first.molang_symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::Variable
            && symbol.identifier.as_ref() == "variable.enabled"
    }));
    assert!(first.molang_ops.contains(&MolangOp::And));
    assert!(first.molang_ops.contains(&MolangOp::Clamp));
    assert!(
        first
            .molang_ops
            .iter()
            .any(|op| matches!(op, MolangOp::SelectCollection(_)))
    );
}

#[test]
fn required_missing_animation_rejects_only_that_rig_and_optional_expression_falls_back() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/rejected.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:rejected","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"required":"animation.missing"},"render_controllers":[{"controller.render.test":"query.unlisted"}]}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.rig_bindings.len(),
        1,
        "the valid rig remains resolved"
    );
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
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(compiled.controllers.is_empty());
    assert!(
        !compiled
            .molang_ops
            .iter()
            .any(|operation| { matches!(operation, MolangOp::LoadVariable(_)) })
    );
}

#[test]
fn unlisted_query_in_optional_controller_is_attributed_as_fallback_not_bytecode() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "animation_controllers/test.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"states":{"default":{"transitions":[{"default":"query.unlisted"}]}}}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(
        !compiled
            .molang_symbols
            .iter()
            .any(|symbol| symbol.identifier.as_ref() == "query.unlisted")
    );
    assert!(compiled.controllers.is_empty());
}

#[test]
fn accepted_molang_surface_compiles_every_query_operator_and_fixed_arity_function() {
    let pack = animation_pack(false);
    let expressions = [
        "query.is_on_ground ? query.anim_time : query.life_time",
        "-query.modified_move_speed + query.ground_speed",
        "query.is_on_ground && query.is_moving || query.is_sprinting",
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
        "query.anim_time % query.life_time",
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
        "1 / 0 + 1 % 0",
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
    for operation in [
        MolangOp::Add,
        MolangOp::Subtract,
        MolangOp::Multiply,
        MolangOp::Divide,
        MolangOp::Modulo,
        MolangOp::Negate,
        MolangOp::Not,
        MolangOp::Abs,
        MolangOp::Ceil,
        MolangOp::Floor,
        MolangOp::Round,
        MolangOp::Sqrt,
        MolangOp::Sin,
        MolangOp::Cos,
        MolangOp::And,
        MolangOp::Or,
        MolangOp::Equal,
        MolangOp::NotEqual,
        MolangOp::Less,
        MolangOp::LessEqual,
        MolangOp::Greater,
        MolangOp::GreaterEqual,
        MolangOp::Min,
        MolangOp::Max,
        MolangOp::Select,
        MolangOp::Clamp,
        MolangOp::Lerp,
    ] {
        assert!(
            compiled.molang_ops.contains(&operation),
            "missing {operation:?}"
        );
    }
    assert!(
        compiled
            .molang_ops
            .iter()
            .any(|operation| matches!(operation, MolangOp::Push(value) if value.get() == 0.0))
    );
}

#[test]
fn assignment_loops_return_strings_dynamic_properties_and_arbitrary_functions_are_unsupported() {
    for expression in [
        "variable.x = 1",
        "loop(2, 1)",
        "return 1",
        "'runtime string'",
        "variable['dynamic']",
        "math.random(0, 1)",
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
            compiled.controllers.is_empty(),
            "unexpected support for {expression}"
        );
    }
}
