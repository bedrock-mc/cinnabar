//! Per-player furnace preferences outlive individual container windows.

use crate::InventorySession;
use protocol::{FurnaceKind, FurnaceOptions};

/// Independently retained preferences for each supported furnace kind.
#[derive(Clone, Debug, Default)]
pub(crate) struct FurnacePreferences {
    furnace: Option<FurnaceOptions>,
    blast_furnace: Option<FurnaceOptions>,
    smoker: Option<FurnaceOptions>,
}

impl FurnacePreferences {
    /// Borrows the preference slot belonging to one furnace screen.
    fn slot(&mut self, kind: FurnaceKind) -> &mut Option<FurnaceOptions> {
        match kind {
            FurnaceKind::Furnace => &mut self.furnace,
            FurnaceKind::BlastFurnace => &mut self.blast_furnace,
            FurnaceKind::Smoker => &mut self.smoker,
        }
    }

    /// Reads the last server or locally saved preferences for one furnace kind.
    fn get(&self, kind: FurnaceKind) -> Option<FurnaceOptions> {
        match kind {
            FurnaceKind::Furnace => self.furnace,
            FurnaceKind::BlastFurnace => self.blast_furnace,
            FurnaceKind::Smoker => self.smoker,
        }
    }
}

impl InventorySession {
    /// Reads preferences retained for the player, even without an open furnace.
    pub fn furnace_options(&self, kind: FurnaceKind) -> FurnaceOptions {
        self.furnace_preferences
            .get(kind)
            .unwrap_or(FurnaceOptions {
                left_tab: protocol::FurnaceTab::Search,
                filtering: false,
                layout: protocol::FurnaceLayout::None,
            })
    }

    /// Retains one furnace kind's preferences without modifying other screens or stacks.
    pub fn set_furnace_options(&mut self, kind: FurnaceKind, options: FurnaceOptions) {
        *self.furnace_preferences.slot(kind) = Some(options);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{FurnaceLayout, FurnaceOptionsEvent, FurnaceTab, InventoryEvent};

    #[test]
    fn furnace_preferences_are_fifo_owned_separate_and_retired_with_the_session() {
        let mut session = InventorySession::new(1);
        let options = FurnaceOptions {
            left_tab: FurnaceTab::Food,
            filtering: false,
            layout: FurnaceLayout::RecipeBook,
        };
        let initial = session.furnace_options(FurnaceKind::Furnace);
        session
            .enqueue_inventory_event(
                1,
                1,
                InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
                    kind: FurnaceKind::Furnace,
                    options,
                }),
            )
            .unwrap();
        assert_eq!(session.furnace_options(FurnaceKind::Furnace), initial);
        session.apply_next().unwrap();
        assert_eq!(session.furnace_options(FurnaceKind::Furnace), options);
        assert_eq!(session.furnace_options(FurnaceKind::BlastFurnace), initial);
        assert_eq!(session.furnace_options(FurnaceKind::Smoker), initial);
        let snapshot = session.with_ledger(session.ledger_snapshot());
        assert_eq!(snapshot.furnace_options(FurnaceKind::Furnace), options);
        session.begin_session(2);
        assert_eq!(session.furnace_options(FurnaceKind::Furnace), initial);
    }
}
