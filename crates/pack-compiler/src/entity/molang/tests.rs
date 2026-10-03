use super::*;

fn compiles(source: &str) -> bool {
    MolangCompiler::default().compile(source).is_ok()
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
