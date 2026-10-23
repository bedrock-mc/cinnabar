//! Shares the first-menu boundary with world loaders and pipeline owners.

use bevy::prelude::{Res, Resource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Direct world launches are admitted immediately; menu launches wait for completed content.
#[derive(Clone, Resource)]
pub struct WorldWorkAdmission(Arc<AtomicBool>);

impl Default for WorldWorkAdmission {
    /// Keeps direct joins, tests and isolated renderers independent of a menu.
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl WorldWorkAdmission {
    /// Creates a closed boundary for a launch that must present its menu first.
    pub fn after_menu() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Opens the boundary once the menu's submitted GPU work completes.
    pub fn admit(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Reads admission across the main and render threads.
    pub fn is_admitted(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Gates world-only systems while allowing renderers without a startup menu to run normally.
pub fn world_work_allowed(admission: Option<Res<WorldWorkAdmission>>) -> bool {
    admission.is_none_or(|admission| admission.is_admitted())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_completion_opens_the_shared_boundary_once() {
        let gate = WorldWorkAdmission::after_menu();
        let worker = gate.clone();
        assert!(!worker.is_admitted());
        gate.admit();
        assert!(worker.is_admitted());
        gate.admit();
        assert!(worker.is_admitted());
        assert!(WorldWorkAdmission::default().is_admitted());
    }
}
