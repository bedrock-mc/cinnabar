use std::{fs, path::Path};

use asset_compiler::{compile_entity_assets, compile_entity_assets_with_report};
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
    assert!(first.molang_ops.contains(&MolangOp::And));
    assert!(first.molang_ops.contains(&MolangOp::Clamp));
    assert!(first.molang_ops.contains(&MolangOp::Equal));
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

#[test]
fn conflicting_animation_aliases_are_resolved_inside_each_entity_environment() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.walk","main":"controller.animation.test"},"render_controllers":["controller.render.test"]}}}"#,
    );
    write(
        pack.path(),
        "entity/second.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:second","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.attack","main":"controller.animation.second"},"render_controllers":["controller.render.test"]}}}"#,
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
        .map(|binding| compiled.animation_clips[binding.clip as usize].symbol)
        .map(|symbol| compiled.symbols[symbol as usize].identifier.as_ref())
        .collect::<Vec<_>>();
    assert!(clip_symbols.contains(&"animation.test.walk"));
    assert!(clip_symbols.contains(&"animation.test.attack"));
}

#[test]
fn real_layout_animation_controllers_bind_separately_from_animation_aliases() {
    let pack = animation_pack(false);
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:allay","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"move":"animation.test.walk"},"animation_controllers":[{"main":"controller.animation.test"}],"render_controllers":["controller.render.test"]}}}"#,
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
        asset_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
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
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"aaa_optional":"geometry.player","default":"geometry.test"},"animations":{"walk":"animation.test.walk"},"render_controllers":["controller.render.test"]}}}"#,
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
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.child"},"animations":{"move":"animation.test.walk"},"render_controllers":["controller.render.test"]}}}"#,
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
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.a"},"animations":{"move":"animation.test.walk"},"render_controllers":["controller.render.test"]}}}"#,
    );
    write(
        pack.path(),
        "entity/second.entity.json",
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:second","geometry":{"default":"geometry.b"},"animations":{"move":"animation.test.second"},"render_controllers":["controller.render.test"]}}}"#,
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
        br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.a","alternate":"geometry.b"},"animations":{"move":"animation.test.walk","attack":"animation.test.attack"},"animation_controllers":[{"main":"controller.animation.test"}],"render_controllers":["controller.render.test"]}}}"#,
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
    let controller_clip = compiled.controller_animations[state.first_animation as usize].clip;
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
        asset_compiler::CompileReferenceOutcome::RequiredRigRejected {
            reason: asset_compiler::RejectReason::AmbiguousGeometryReference,
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
        asset_compiler::CompileReferenceOutcome::RequiredRigRejected {
            reason: asset_compiler::RejectReason::AmbiguousAnimationReference,
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
        asset_compiler::CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
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
        asset_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            symbol,
            reason: asset_compiler::FallbackReason::UnreferencedDefinition,
            ..
        } if *symbol == controller_symbol
    )));
}
