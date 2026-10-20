//! Guest-scoped publication identities shared by gameplay and panel card rendering.

use super::ModWidgets;
use crate::{ModUi, PanelUiOwner};
use std::{collections::BTreeMap, sync::Arc};
use ui::mod_hud::Hud;

#[derive(Default)]
pub(crate) struct HudSources {
    entries: BTreeMap<PanelUiOwner, Source>,
    pub(super) order: Vec<PanelUiOwner>,
}

struct Source {
    revision: u64,
    live: bool,
    widgets: ModWidgets,
}

impl ModUi {
    /// Publishes validated guest HUDs with process-unique revisions, including native updates.
    /// Repeating a revision preserves all cached output; absent owners are revoked immediately.
    pub fn set_mod_hud_sources<'a>(
        &mut self,
        sources: impl Iterator<Item = (PanelUiOwner, u64, &'a Hud)> + Clone,
    ) -> Result<(), String> {
        if !self
            .hud_sources
            .order
            .iter()
            .copied()
            .eq(sources.clone().map(|(owner, _, _)| owner))
        {
            self.hud_sources.order.clear();
            self.hud_sources
                .order
                .extend(sources.clone().map(|(owner, _, _)| owner));
        }
        let mut changed = false;
        for source in self.hud_sources.entries.values_mut() {
            source.live = false;
        }
        for (owner, revision, hud) in sources {
            if let Some(source) = self.hud_sources.entries.get_mut(&owner) {
                source.live = true;
                if source.revision != revision {
                    changed = true;
                    hud.validate()?;
                    source.widgets.publish(Arc::new(hud.clone()))?;
                    source.revision = revision;
                }
            } else {
                changed = true;
                hud.validate()?;
                self.hud_sources.entries.insert(
                    owner,
                    Source {
                        revision,
                        live: true,
                        widgets: ModWidgets::new(Arc::new(hud.clone()))?,
                    },
                );
            }
        }
        changed |= self.hud_sources.entries.values().any(|source| !source.live);
        self.hud_sources.entries.retain(|_, source| source.live);
        if changed {
            self.refresh_hud_card_sources();
        }
        Ok(())
    }
    /// Resolves merged precedence only when publication changes, never on the paint path.
    pub(super) fn refresh_hud_card_sources(&mut self) {
        if let Some(widgets) = &mut self.mod_widgets {
            for (entry, card) in widgets.cards.iter_mut().zip(&widgets.published.cards) {
                entry.source = self.hud_sources.card_owner(card);
            }
        }
    }
}

impl HudSources {
    /// Borrows only the named guest's currently published cards.
    pub(crate) fn get_mut(&mut self, owner: PanelUiOwner) -> Option<&mut ModWidgets> {
        self.entries
            .get_mut(&owner)
            .map(|source| &mut source.widgets)
    }

    /// Finds the admitted merged card's original owner, respecting source precedence.
    pub(super) fn card_owner(&self, card: &ui::mod_hud::Card) -> Option<(PanelUiOwner, usize)> {
        self.order.iter().find_map(|owner| {
            self.entries.get(owner).and_then(|source| {
                source
                    .widgets
                    .published
                    .cards
                    .iter()
                    .position(|candidate| candidate == card)
                    .map(|index| (*owner, index))
            })
        })
    }
}
