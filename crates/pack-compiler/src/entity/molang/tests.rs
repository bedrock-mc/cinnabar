use super::*;

fn compiles(source: &str) -> bool {
    MolangCompiler::default().compile(source).is_ok()
}

#[test]
fn long_authored_scripts_compile_as_one_program_with_ordered_assignments() {
    let mut compiler = MolangCompiler::default();
    let assignment = "variable.long_authored_script_value = variable.long_authored_script_value + query.wing_flap_position + query.life_time;";
    let entries = vec![assignment; 250];
    let (expression, dropped) = compiler.compile_script(&entries).unwrap();
    assert_eq!(dropped, 0);
    let payload = compiler.finish().unwrap();
    let expression = &payload.expressions[expression.expect("complete script retained") as usize];
    let program = &payload.ops[expression.first_op as usize..][..usize::from(expression.op_count)];
    assert!(program.len() <= MAX_MOLANG_OPS_PER_EXPRESSION);
    assert_eq!(
        program
            .iter()
            .filter(|op| matches!(op, MolangOp::StoreVariable(_)))
            .count(),
        entries.len()
    );
}

#[test]
fn server_pre_animation_retains_visibility_after_optional_item_and_property_branches() {
    let mut compiler = MolangCompiler::default();
    let entries = [
        "v.near = q.camera_distance_range_lerp(2, 6);",
        "v.spear ? { t.rate = 1 / q.base_swing_duration; };",
        "v.wardrobe ? { v.hat = q.has_property('custom:hat') ? q.property('custom:hat') : 0; v.hat = v.hat && !q.has_armor_slot(0); };",
        "v.visible = q.mark_variant != 255;",
    ];
    let (expression, dropped) = compiler.compile_script(&entries).unwrap();
    assert_eq!(dropped, 0);
    let payload = compiler.finish().unwrap();
    let expression = &payload.expressions[expression.expect("complete script retained") as usize];
    let visible = payload
        .symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "variable.visible")
        .unwrap() as u32;
    let program = &payload.ops[expression.first_op as usize..][..usize::from(expression.op_count)];
    assert!(program.contains(&MolangOp::StoreVariable(visible)));
}

fn constant(source: &str) -> f32 {
    MolangCompiler::evaluate_default(source).unwrap_or_else(|| panic!("{source} folds"))
}

fn ops(source: &str) -> Vec<MolangOp> {
    let mut compiler = MolangCompiler::default();
    compiler.compile(source).unwrap();
    compiler.finish().unwrap().ops.into_vec()
}

#[test]
fn precedence_matches_the_vanilla_table() {
    // `/` binds tighter than `*`, relational tighter than equality, `&&` tighter than `||`.
    let divided_first = 10.0_f32 * (1.0_f32 / 3.0);
    assert_ne!(
        divided_first,
        10.0_f32 / 3.0,
        "the fixture discriminates grouping"
    );
    assert_eq!(constant("10 * 1 / 3"), divided_first);
    assert_eq!(constant("1 || 0 && 0"), 1.0);
    assert_eq!(constant("3 == 3 > 0"), 0.0);
    assert_eq!(constant("2 * 6 / 3"), 4.0);
    assert_eq!(constant("1 + 2 * 3"), 7.0);
    assert_eq!(constant("1 < 2 == 1"), 1.0);
    assert_eq!(constant("0 || 1 && 0"), 0.0);
    assert_eq!(constant("1 ? 2 : 3 ? 4 : 5"), 2.0);
    assert_eq!(constant("0 ? 2 : 0 ? 4 : 5"), 5.0);
    assert_eq!(constant("0 ? 7"), 0.0);
    assert_eq!(constant("-2 * -3"), 6.0);
    assert_eq!(constant("!0 + !5"), 1.0);
    assert_eq!(constant("math.pi > 3.14"), 1.0);
    assert_eq!(constant("1 / 0"), 0.0);
    assert_eq!(constant("2.5f * 2"), 5.0);
}

// Molang literals are all floats, unlike JSON-UI's integer-prefix typing.
#[test]
fn leading_zero_decimals_and_division_are_float() {
    assert_eq!(constant("0.01 * 300"), 0.01_f32 * 300.0);
    assert_eq!(constant("0.5 + .25"), 0.75);
    assert_eq!(constant("-0.5 * 2"), -1.0);
    assert_eq!(constant("7 / 2"), 3.5);
}

#[test]
fn identifiers_are_case_insensitive_and_short_namespaces_expand() {
    assert!(compiles(
        "Math.Sin(Q.Anim_Time) + V.Speed * T.Scratch + C.Item_Slot"
    ));
    let lowered = ops("q.is_sneaking ? v.a : t.b");
    let mut compiler = MolangCompiler::default();
    compiler
        .compile("query.is_sneaking ? variable.a : temp.b")
        .unwrap();
    assert_eq!(compiler.finish().unwrap().ops.into_vec(), lowered);
}

#[test]
fn string_literals_keep_their_case_and_compare_by_value() {
    let mut compiler = MolangCompiler::default();
    compiler
        .compile("query.get_equipped_item_name('off_hand') == 'Filled_Map'")
        .unwrap();
    let payload = compiler.finish().unwrap();
    assert!(payload.symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::String && symbol.identifier.as_ref() == "Filled_Map"
    }));
    assert!(payload.ops.contains(&MolangOp::Equal));
}

#[test]
fn complex_expressions_need_terminators_and_end_at_return() {
    assert!(compiles("v.x = 1;"));
    assert!(compiles("v.x = 1; return v.x;"));
    assert!(compiles("{ v.x = 1; v.y = 2; };"));
    assert!(!compiles("v.x = 1"));
    assert!(!compiles("v.x = 1; v.y = 2"));
    assert!(!compiles("return 1; v.x = 2;"));
    assert!(!compiles("q.anim_time = 1;"));
    let program = ops("v.x = 1;");
    assert_eq!(
        program.last(),
        Some(&MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap())),
        "a complex expression without return yields zero"
    );
}

#[test]
fn loops_break_continue_and_for_each_compile_to_bounded_frames() {
    assert!(compiles("loop(4, { v.i = v.i + 1; (v.i > 2) ? break; });"));
    assert!(compiles("loop(4, { continue; });"));
    assert!(compiles(
        "for_each(t.actor, q.anim_time, { v.n = v.n + 1; });"
    ));
    assert!(!compiles("break;"));
    assert!(!compiles("loop(2, 1);"));
    assert!(!compiles("loop(2, { v.x = 1; }) + 1"));
    let program = ops("loop(3, { v.i = v.i + 1; });");
    assert!(
        program
            .iter()
            .any(|op| matches!(op, MolangOp::LoopStart(_)))
    );
    assert!(program.iter().any(|op| matches!(op, MolangOp::LoopNext(_))));
}

#[test]
fn unknown_names_wrong_arity_and_vanilla_rejected_forms_fail_to_compile() {
    for source in [
        "query.not_a_vanilla_query",
        "math.nope(1)",
        "math.sin(1, 2)",
        "math.clamp(1, 2)",
        "1 % 2",
        "q.anim_time ?? 1",
        "v.a->v.b->v.c",
        "'unterminated",
        "array.skins[0]",
        "",
    ] {
        assert!(!compiles(source), "{source}");
    }
}

#[test]
fn coalesce_arrow_and_resource_references_compile() {
    let program = ops("v.offset ?? 2");
    assert!(program.iter().any(|op| matches!(op, MolangOp::Coalesce(_))));
    assert!(compiles("c.owning_entity->v.attack_time"));
    assert!(compiles("geometry.default == 'geometry.default'"));
}

#[test]
fn script_entries_form_one_program_that_compiles_or_is_dropped_whole() {
    let mut compiler = MolangCompiler::default();
    let split = ["(v.flag) ? {", "  t.a = 1;", "  v.x = t.a;", "};"];
    assert_eq!(compiler.compile_script(&split).unwrap().1, 0);
    let (script, dropped) = compiler
        .compile_script(&["v.x = 1;", "v.y = q.not_a_vanilla_query;"])
        .unwrap();
    assert!(script.is_none());
    assert_eq!(dropped, 2);
}

#[test]
fn random_calls_are_never_folded() {
    let program = ops("math.random(1, 1)");
    assert!(program.contains(&MolangOp::Call(assets::MolangFunction::Random)));
    assert_eq!(ops("math.clamp(5, 0, 1) + math.abs(-2)").len(), 1);
}

#[test]
fn constants_beyond_the_carrier_bound_leave_only_their_expression_uncompiled() {
    let mut compiler = MolangCompiler::default();
    assert!(compiler.compile("1e30 + q.anim_time").is_err());
    compiler.compile("q.anim_time").unwrap();
    compiler.finish().unwrap();
}

#[test]
fn malformed_script_shapes_drop_the_whole_script_for_entities_and_controllers_alike() {
    let mut compiler = MolangCompiler::default();
    let value = serde_json::json!(["v.x = 1;", 3]);
    assert_eq!(
        compiler.compile_script_value(Some(&value)).unwrap(),
        (None, 2)
    );
    assert_eq!(
        compiler
            .compile_script_value(Some(&serde_json::json!({"v.x": 1})))
            .unwrap(),
        (None, 1)
    );
    let (script, dropped) = compiler
        .compile_script_value(Some(&serde_json::json!("v.x = 1;")))
        .unwrap();
    assert!(script.is_some() && dropped == 0);
}

// Block contexts compare strings and read integer/boolean states as numbers.
#[test]
fn block_expressions_read_block_states() {
    let state = |name: &str| match name {
        "minecraft:cardinal_direction" => Some(BlockStateValue::String("north")),
        "df:s" => Some(BlockStateValue::Number(1.0)),
        _ => None,
    };
    let evaluate = |source| BlockMolang::parse(source)?.evaluate(&state);
    assert_eq!(
        evaluate("q.block_state('minecraft:cardinal_direction') == 'north'"),
        Some(1.0)
    );
    assert_eq!(
        evaluate("query.block_state('minecraft:cardinal_direction') != 'north'"),
        Some(0.0)
    );
    assert_eq!(evaluate("q.block_state('df:s')"), Some(1.0));
    assert_eq!(evaluate("!q.block_state('df:s') || 0"), Some(0.0));
    assert_eq!(evaluate("1.000000"), Some(1.0));
    assert_eq!(evaluate("0.000000"), Some(0.0));
    assert_eq!(evaluate("q.block_state('df:missing') == 1"), None);
    assert_eq!(
        evaluate("q.block_state('df:s') == 'true'"),
        None,
        "mixed types"
    );
    assert_eq!(evaluate("q.is_baby"), None, "other queries do not parse");
    assert_eq!(evaluate("math.random(0, 1) > 0.5"), None);
}

#[test]
fn block_property_alias_reads_the_same_named_state() {
    let state = |name: &str| match name {
        "custom:facing_direction" => Some(BlockStateValue::Number(2.0)),
        "minecraft:cardinal_direction" => Some(BlockStateValue::String("west")),
        _ => None,
    };
    for query in [
        "q.block_property",
        "query.block_property",
        "q.block_state",
        "query.block_state",
    ] {
        for (suffix, expected) in [
            ("('custom:facing_direction') == 2", Some(1.0)),
            ("('custom:facing_direction') == 1", Some(0.0)),
            ("('minecraft:cardinal_direction') == 'west'", Some(1.0)),
            ("('custom:missing')", None),
        ] {
            let source = format!("{query}{suffix}");
            let expression = BlockMolang::parse(&source).expect("block-state query alias");
            assert_eq!(expression.evaluate(&state), expected, "{source}");
        }
    }
}

// A server-sent flat operator chain cannot exhaust the stack that walks its tree.
#[test]
fn block_expressions_bound_their_tree_depth() {
    let chain = |terms: usize| vec!["1"; terms].join("+");
    let state = |_: &str| None;
    assert_eq!(
        BlockMolang::parse(&chain(1024)).and_then(|expression| expression.evaluate(&state)),
        Some(1024.0)
    );
    assert!(BlockMolang::parse(&chain(16_000)).is_none());
}
