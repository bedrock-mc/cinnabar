//! Classic inventory controllers snapshot session preferences without changing their storage.

use protocol::{InventoryLayout, InventoryOptions, InventoryRightTab, InventoryTab};

/// Which retained layout a Classic inventory controller edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryContext {
    Personal,
    Workbench,
}

/// One open controller's complete tuple and layout owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryController {
    pub options: InventoryOptions,
    pub context: InventoryContext,
}

impl InventoryController {
    /// Captures stored options, resolving only the active Classic layout and guarded tabs.
    pub fn new(mut options: InventoryOptions, context: InventoryContext, creative: bool) -> Self {
        if options.left_tab == InventoryTab::Survival {
            options.left_tab = InventoryTab::Search;
        }
        if !creative && options.right_tab == InventoryRightTab::Fullscreen {
            options.right_tab = InventoryRightTab::Crafting;
        }
        let mut controller = Self { options, context };
        let layout = controller.layout();
        let default = if controller.allows_recipe_book_only(creative) {
            InventoryLayout::RecipeBookOnly
        } else {
            InventoryLayout::RecipeBook
        };
        if layout == InventoryLayout::None
            || layout.index() > InventoryLayout::RecipeBookOnly.index()
        {
            controller.set_layout(default);
        } else if layout == InventoryLayout::RecipeBookOnly
            && !controller.allows_recipe_book_only(creative)
        {
            controller.set_layout(InventoryLayout::RecipeBook);
        }
        controller
    }

    /// Reads the active layout while retaining the other context's value verbatim.
    pub fn layout(self) -> InventoryLayout {
        match self.context {
            InventoryContext::Personal => self.options.inventory_layout,
            InventoryContext::Workbench => self.options.crafting_layout,
        }
    }

    /// Changes only this controller's active layout.
    pub fn set_layout(&mut self, layout: InventoryLayout) {
        match self.context {
            InventoryContext::Personal => self.options.inventory_layout = layout,
            InventoryContext::Workbench => self.options.crafting_layout = layout,
        }
    }

    /// Only creative personal inventories offer the recipe-book-only layout.
    pub fn allows_recipe_book_only(self, creative: bool) -> bool {
        creative && self.context == InventoryContext::Personal
    }
}

impl crate::InventorySession {
    /// Reads the last complete server or locally saved preference tuple.
    pub fn inventory_options(&self) -> InventoryOptions {
        self.inventory_options
    }

    /// Retains a tuple without refreshing an open controller or altering stacks.
    pub fn set_inventory_options(&mut self, options: InventoryOptions) {
        self.inventory_options = options;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_options_defaults_resolve_only_the_active_classic_layout() {
        for (context, creative, expected) in [
            (
                InventoryContext::Personal,
                false,
                InventoryLayout::RecipeBook,
            ),
            (
                InventoryContext::Personal,
                true,
                InventoryLayout::RecipeBookOnly,
            ),
            (
                InventoryContext::Workbench,
                false,
                InventoryLayout::RecipeBook,
            ),
            (
                InventoryContext::Workbench,
                true,
                InventoryLayout::RecipeBook,
            ),
        ] {
            let stored = InventoryOptions::default();
            let controller = InventoryController::new(stored, context, creative);
            assert_eq!(controller.layout(), expected);
            assert!(!controller.options.filtering);
            assert_eq!(controller.options.left_tab, InventoryTab::Search);
            assert_eq!(controller.options.right_tab, InventoryRightTab::Crafting);
            match context {
                InventoryContext::Personal => {
                    assert_eq!(controller.options.crafting_layout, stored.crafting_layout)
                }
                InventoryContext::Workbench => {
                    assert_eq!(controller.options.inventory_layout, stored.inventory_layout)
                }
            }
        }
    }

    #[test]
    fn inventory_options_keep_inactive_unknown_values_and_apply_classic_guards() {
        let stored = InventoryOptions {
            left_tab: InventoryTab::Survival,
            right_tab: InventoryRightTab::Fullscreen,
            inventory_layout: InventoryLayout::RecipeBookOnly,
            crafting_layout: InventoryLayout::Unknown(83),
            filtering: true,
        };
        let mut personal = InventoryController::new(stored, InventoryContext::Personal, false);
        assert_eq!(personal.layout(), InventoryLayout::RecipeBook);
        assert_eq!(personal.options.crafting_layout, stored.crafting_layout);
        assert_eq!(personal.options.left_tab, InventoryTab::Search);
        assert_eq!(personal.options.right_tab, InventoryRightTab::Crafting);
        personal.set_layout(InventoryLayout::InventoryOnly);
        assert_eq!(personal.options.crafting_layout, stored.crafting_layout);
        let workbench = InventoryController::new(stored, InventoryContext::Workbench, true);
        assert_eq!(workbench.layout(), InventoryLayout::RecipeBook);
        assert_eq!(workbench.options.inventory_layout, stored.inventory_layout);
        assert_eq!(workbench.options.right_tab, stored.right_tab);
        let negative = InventoryOptions {
            inventory_layout: InventoryLayout::Unknown(-1),
            ..stored
        };
        assert_eq!(
            InventoryController::new(negative, InventoryContext::Personal, true).layout(),
            InventoryLayout::Unknown(-1)
        );
    }

    #[test]
    fn inventory_options_apply_in_fifo_and_retire_with_the_session() {
        let mut session = crate::InventorySession::new(1);
        let options = InventoryOptions {
            left_tab: InventoryTab::Nature,
            ..Default::default()
        };
        session
            .enqueue_inventory_event(1, 2, protocol::InventoryEvent::Options(options))
            .unwrap();
        assert_eq!(session.inventory_options(), InventoryOptions::default());
        session.apply_next().unwrap();
        assert_eq!(session.inventory_options(), options);
        assert_eq!(
            session
                .with_ledger(session.ledger_snapshot())
                .inventory_options(),
            options
        );
        session.begin_session(2);
        assert_eq!(session.inventory_options(), InventoryOptions::default());
        assert!(
            session
                .enqueue_inventory_event(1, 3, protocol::InventoryEvent::Options(options))
                .is_err()
        );
    }
}
