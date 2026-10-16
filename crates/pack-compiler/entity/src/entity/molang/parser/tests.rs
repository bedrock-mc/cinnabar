use super::*;

/// Wraps a statement in nested control-flow bodies without exceeding the source-size policy.
fn nested(open: &str, close: &str, depth: usize) -> String {
    format!("{}v.x = 1;{}", open.repeat(depth), close.repeat(depth))
}

#[test]
fn blocks_keep_their_parent_depth() {
    let blocks = |depth| nested("{", "};", depth);
    assert!(parse(&blocks(MAX_PARSE_DEPTH - 1)).is_ok());
    assert!(parse(&blocks(MAX_PARSE_DEPTH)).is_err());
    assert!(parse(&blocks(4_000)).is_err());
}

#[test]
fn loop_foreach_and_branch_bodies_keep_structural_depth() {
    for (open, close) in [
        ("loop(1, {", "});"),
        ("for_each(t.actor, q.anim_time, {", "});"),
        ("q.health ? {", "};"),
    ] {
        assert!(parse(&nested(open, close, 4)).is_ok(), "{open}");
        assert!(
            parse(&nested(open, close, MAX_PARSE_DEPTH + 1)).is_err(),
            "{open}"
        );
    }
    let nested_no = |depth| nested("q.health ? { v.y = 1; } : {", "};", depth);
    assert!(parse(&nested_no(4)).is_ok());
    assert!(parse(&nested_no(MAX_PARSE_DEPTH + 1)).is_err());
}

#[test]
fn flat_binary_chains_bound_the_constructed_tree() {
    for operator in ["+", "-", "*", "/", "&&", "||", "==", "<"] {
        let chain = |terms: usize| vec!["1"; terms].join(operator);
        assert!(parse(&chain(MAX_PARSE_DEPTH + 1)).is_ok(), "{operator}");
        assert!(parse(&chain(MAX_PARSE_DEPTH + 2)).is_err(), "{operator}");
        assert!(parse(&chain(8_000)).is_err(), "{operator}");
    }
}

#[test]
fn binary_and_nested_call_depth_share_the_owned_tree_limit() {
    let child = vec!["1"; MAX_PARSE_DEPTH + 1].join("+");
    assert!(parse(&child).is_ok());
    assert!(parse(&format!("math.abs({child})")).is_err());
    assert!(parse(&format!("({child}) * 1")).is_err());
    assert!(parse(&format!("q.health ? ({child}) : 0")).is_err());
}
