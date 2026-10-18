//! Attachable rig lookup prepared once for each immutable entity catalog.

use super::*;
use std::sync::Arc;

/// Indexes admitted attachable rigs by identifier, retaining duplicates as ambiguous entries.
pub(super) fn compile(symbols: &[EntityAssetSymbol], rigs: &[EntityRigBinding]) -> Arc<[usize]> {
    let mut indices: Vec<_> = (0..rigs.len())
        .filter(|&index| rig_symbol(symbols, rigs, index).kind == EntityAssetKind::Attachable)
        .collect();
    indices.sort_unstable_by(|&left, &right| {
        rig_symbol(symbols, rigs, left)
            .identifier
            .cmp(&rig_symbol(symbols, rigs, right).identifier)
    });
    indices.into()
}

/// Reads one rig's validated symbol and counts actual catalog inspections in test builds.
fn rig_symbol<'a>(
    symbols: &'a [EntityAssetSymbol],
    rigs: &[EntityRigBinding],
    index: usize,
) -> &'a EntityAssetSymbol {
    #[cfg(any(test, feature = "test-support"))]
    RIG_INSPECTIONS.with(|work| work.set(work.get() + 1));
    &symbols[rigs[index].entity_symbol as usize]
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static RIG_INSPECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl RuntimeEntityAssets {
    /// Finds an unambiguous attachable rig, including selection-only rigs, in logarithmic work.
    #[must_use]
    pub fn attachable_rig_binding(&self, identifier: &str) -> Option<usize> {
        let symbol = |index| rig_symbol(&self.symbols, &self.rig_bindings, index);
        let start = self
            .attachable_rigs
            .partition_point(|&index| symbol(index).identifier.as_ref() < identifier);
        let &index = self.attachable_rigs.get(start)?;
        if symbol(index).identifier.as_ref() != identifier
            || self
                .attachable_rigs
                .get(start + 1)
                .is_some_and(|&next| symbol(next).identifier.as_ref() == identifier)
        {
            return None;
        }
        Some(index)
    }

    /// Returns and resets this thread's rig inspections during index preparation and lookup.
    #[cfg(any(test, feature = "test-support"))]
    pub fn take_attachable_rig_work() -> usize {
        RIG_INSPECTIONS.with(|work| work.replace(0))
    }
}
