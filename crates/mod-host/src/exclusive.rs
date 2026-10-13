//! Host-side refusal of exclusive outputs claimed by higher-priority instances.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

/// HUD slot owners are shared across instances in Active-list order.
#[derive(Clone, Default)]
pub struct HudClaims(Arc<Mutex<BTreeMap<String, usize>>>);
impl HudClaims {
    /// Rebuilds slot ownership from retained outputs at the beginning of each frame.
    pub fn reset(&self) {
        self.0.lock().expect("mod HUD claims").clear();
    }
    /// Refuses the whole publication when any slot belongs to a higher mod.
    pub(crate) fn claim(&self, owner: usize, ids: impl Iterator<Item = String>) -> bool {
        let ids: Vec<_> = ids.collect();
        let mut claims = self.0.lock().expect("mod HUD claims");
        if ids
            .iter()
            .any(|id| claims.get(id).is_some_and(|higher| *higher < owner))
        {
            return false;
        }
        for id in ids {
            claims.insert(id, owner);
        }
        true
    }
    /// Releases a quarantined instance's slots so later callbacks can claim them immediately.
    pub(crate) fn release(&self, owner: usize) {
        self.0
            .lock()
            .expect("mod HUD claims")
            .retain(|_, claimed| *claimed != owner);
    }

    /// Seeds a retained publication in precedence order.
    pub fn retain(&self, owner: usize, ids: impl Iterator<Item = String>) {
        self.claim(owner, ids);
    }
}

#[derive(Clone, Default)]
pub(crate) struct Exclusive {
    pub camera: bool,
    /// A consumed one-shot camera publication keeps precedence until the next native frame.
    pub camera_claimed: bool,
    pub environment: bool,
    pub owner: usize,
    pub hud: Option<HudClaims>,
    pub refused: u64,
    pub suppressed_hud: BTreeSet<String>,
}
impl Exclusive {
    /// Records each refused import without trapping the guest.
    pub fn refuse(&mut self) -> String {
        self.refused = self.refused.saturating_add(1);
        "exclusive output belongs to a higher active mod".into()
    }
}
