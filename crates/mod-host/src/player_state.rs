//! Bounded, read-only local player facts with callback-scoped reads and exact content tokens.

use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{PlayerStateItem, PlayerStateSlot, PlayerStateSnapshot};
use anyhow::{Result, bail, ensure};
use mod_api::{
    MAX_ITEM_IDENTIFIER_BYTES, MAX_PLAYER_STATE_EFFECTS, PLAYER_STATE_ARMOR_SLOTS,
    PLAYER_STATE_INVENTORY_SLOTS,
};

#[derive(Default)]
pub(super) struct PlayerState {
    pub snapshot: Option<PlayerStateSnapshot>,
    comparison_snapshot: Option<PlayerStateSnapshot>,
    comparison_revision: u64,
    current_revision: Option<u64>,
    revision: u64,
    reads: u32,
    pub observations: crate::module_observations::ModuleObservations,
    pub world: Option<crate::WorldObservation>,
    pub fps: Option<f32>,
    pub sidebar: Option<crate::SidebarSnapshot>,
    pub chat: Vec<crate::ObservedChatMessage>,
}

impl PlayerState {
    /// Clears current reads without discarding the exact observation used for change detection.
    pub fn begin_frame(&mut self) {
        self.snapshot = None;
        self.current_revision = None;
        self.reads = 0;
    }

    /// Compares validated callback facts with the last snapshot imported by the guest.
    pub fn set_snapshot(&mut self, snapshot: Option<PlayerStateSnapshot>) -> Result<()> {
        let Some(snapshot) = snapshot else {
            self.snapshot = None;
            self.current_revision = None;
            return Ok(());
        };
        let revision = if self.comparison_snapshot.as_ref() == Some(&snapshot) {
            self.comparison_revision
        } else {
            self.advance_revision()?;
            self.revision
        };
        self.current_revision = Some(revision);
        self.snapshot = Some(snapshot);
        Ok(())
    }

    /// Releases current and comparison facts after failure or loss of the instance.
    pub fn revoke(&mut self) {
        self.begin_frame();
        self.comparison_snapshot = None;
        self.clear_presentation();
    }

    /// Removes callback-only presentation data after successful dispatch or revocation.
    pub fn clear_presentation(&mut self) {
        self.observations = Default::default();
        self.world = None;
        self.fps = None;
        self.sidebar = None;
        self.chat.clear();
    }

    /// Advances an instance-scoped content revision without wrapping the counter.
    fn advance_revision(&mut self) -> Result<()> {
        let Some(revision) = self.revision.checked_add(1) else {
            self.revoke();
            bail!("player-state revision exhausted");
        };
        self.revision = revision;
        Ok(())
    }

    /// Full snapshot and revision imports consume the same callback read allowance.
    fn read_budget(&mut self) -> Result<()> {
        self.reads += 1;
        if self.reads > MAX_IMPORT_WRITES {
            bail!("player-state read budget exhausted");
        }
        Ok(())
    }
}

fn valid_item(item: &PlayerStateItem) -> bool {
    item.network_id != 0
        && item.count > 0
        && item.identifier.as_ref().is_none_or(|identifier| {
            !identifier.is_empty()
                && identifier.len() <= MAX_ITEM_IDENTIFIER_BYTES
                && !identifier.chars().any(char::is_control)
        })
        && item.max_durability != Some(0)
}

fn valid_slot(slot: &PlayerStateSlot) -> bool {
    slot.item
        .as_ref()
        .is_none_or(|item| slot.known && valid_item(item))
}

/// Reject an invalid app payload before any facts cross the guest boundary.
pub(super) fn validate(snapshot: Option<&PlayerStateSnapshot>) -> Result<()> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    ensure!(
        snapshot.inventory.len() == PLAYER_STATE_INVENTORY_SLOTS
            && snapshot.armor.len() == PLAYER_STATE_ARMOR_SLOTS
            && snapshot.effects.len() <= MAX_PLAYER_STATE_EFFECTS,
        "invalid player-state snapshot lengths"
    );
    ensure!(
        snapshot.selected_slot.is_none_or(|slot| slot < 9)
            && snapshot.inventory.iter().all(valid_slot)
            && snapshot.armor.iter().all(valid_slot)
            && valid_slot(&snapshot.offhand)
            && snapshot
                .effects
                .iter()
                .all(|effect| { effect.effect_id > 0 && effect.remaining_ticks != Some(0) })
            && snapshot
                .effects
                .windows(2)
                .all(|pair| pair[0].effect_id < pair[1].effect_id),
        "invalid player-state item, selection or effect"
    );
    Ok(())
}

impl cinnabar::extension::player_state::Host for State {
    fn read_motion(&mut self) -> Result<Result<Option<crate::MotionObservation>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.observations.motion))
    }
    fn read_vitals(&mut self) -> Result<Result<Option<crate::VitalsObservation>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.observations.vitals))
    }
    fn read_ping(&mut self) -> Result<Result<Option<u32>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.observations.ping_ms))
    }
    fn read_actors(&mut self) -> Result<Result<Vec<crate::ActorObservation>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.players && !self.grants.entities {
            return Ok(Err("actor observation capability denied".into()));
        }
        Ok(Ok(self
            .player_state
            .observations
            .actors
            .iter()
            .filter(|actor| {
                if actor.player {
                    self.grants.players
                } else {
                    self.grants.entities
                }
            })
            .cloned()
            .collect()))
    }
    fn read_combat(
        &mut self,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<crate::CombatObservation>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        if limit == 0 || limit as usize > mod_api::MAX_COMBAT_READ_EVENTS {
            return Ok(Err("combat import limit exceeds bounds".into()));
        }
        Ok(Ok(self
            .player_state
            .observations
            .combat
            .iter()
            .filter(|event| {
                event.sequence > after
                    && (event.local
                        || event.kind == crate::CombatKind::AttackAdmitted
                        || if event.player {
                            self.grants.players
                        } else {
                            self.grants.entities
                        })
            })
            .take(limit as usize)
            .cloned()
            .collect()))
    }

    fn read_chat(
        &mut self,
        after: u64,
        limit: u32,
    ) -> Result<Result<Vec<crate::ObservedChatMessage>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        if limit == 0 || limit as usize > ui::mod_hud::MAX_CHAT_READ_MESSAGES {
            return Ok(Err("chat import limit exceeds bounds".into()));
        }
        Ok(Ok(self
            .player_state
            .chat
            .iter()
            .filter(|m| m.sequence > after)
            .take(limit as usize)
            .cloned()
            .collect()))
    }
    fn read_world(&mut self) -> Result<Result<Option<crate::WorldObservation>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.world.clone()))
    }
    fn read_fps(&mut self) -> Result<Result<Option<f32>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.fps))
    }
    fn read_sidebar(&mut self) -> Result<Result<Option<crate::SidebarSnapshot>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.sidebar.clone()))
    }
    fn read_snapshot(&mut self) -> Result<Result<Option<PlayerStateSnapshot>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        if let Some(snapshot) = self.player_state.snapshot.as_ref() {
            if self.player_state.comparison_snapshot.as_ref() != Some(snapshot) {
                self.player_state.comparison_snapshot = Some(snapshot.clone());
            }
            self.player_state.comparison_revision = self
                .player_state
                .current_revision
                .expect("a current player-state snapshot has a revision");
        }
        Ok(Ok(self.player_state.snapshot.clone()))
    }

    fn read_revision(&mut self) -> Result<Result<Option<u64>, String>> {
        self.player_state.read_budget()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.player_state.current_revision))
    }
}

#[cfg(test)]
#[path = "player_state_tests.rs"]
mod tests;
