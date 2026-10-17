//! Original predicates for cold and cached equipment-selection regressions.

/// Builds a false predicate that fills one expression budget with repeated variable reads.
pub(crate) fn rejected_predicate(index: usize) -> String {
    let variable = format!("v.fixture_rejected_{index}");
    // Each sum statement takes eight operations; the final statement and result take eight.
    let statement = format!("{variable}+{variable}+{variable}+{variable};");
    format!(
        "{}!({variable}+{variable}+{variable});",
        statement.repeat((assets::MAX_MOLANG_OPS_PER_EXPRESSION - 8) / 8),
    )
}
