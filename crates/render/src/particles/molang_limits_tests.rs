use super::*;

#[test]
fn rejects_unary_and_binary_chains_before_recursive_evaluation() {
    let mut names = Interner::with_builtins();
    for source in [
        format!("{}1", "!".repeat(256)),
        std::iter::repeat_n("v.x", 256)
            .collect::<Vec<_>>()
            .join("+"),
    ] {
        assert!(
            Program::parse(&source, &mut names).is_none(),
            "oversized expression was admitted"
        );
    }
}

#[test]
fn hostile_sized_chains_are_rejected_without_recursive_work() {
    let mut names = Interner::with_builtins();
    assert!(Program::parse(&format!("{}1", "!".repeat(100_000)), &mut names).is_none());
    let chain = std::iter::repeat_n("v.x", 100_000)
        .collect::<Vec<_>>()
        .join("+");
    assert!(Program::parse(&chain, &mut names).is_none());
}
