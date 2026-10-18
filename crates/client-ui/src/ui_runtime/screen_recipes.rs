//! UI choices projected through the inventory recipe authority.
use super::UiRuntime;
use protocol::{RecipeCatalog, ScreenRecipe};

impl UiRuntime {
    /// Borrows the currently credited recipe catalog.
    pub fn screen_catalog<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Option<&'a RecipeCatalog> {
        player_runtime.inventory.screen_catalog()
    }

    /// Projects stonecutter options from the current ledger input.
    pub fn stonecutter_options<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Vec<&'a ScreenRecipe> {
        player_runtime.inventory.stonecutter_options()
    }

    /// Projects the recipe selected by this screen's local choice.
    pub fn active_screen_recipe<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Option<&'a ScreenRecipe> {
        player_runtime
            .inventory
            .active_screen_recipe(self.screen_state().recipe_choice)
    }

    /// Projects the output of the current screen without owning recipe state.
    pub fn predicted_screen_output(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<protocol::RecipeOutput> {
        player_runtime
            .inventory
            .predicted_screen_output(self.screen_state().recipe_choice)
    }

    /// Resolves the recipe-book filter choice against the current game mode.
    pub fn recipe_filtering(&self, player_runtime: &player_state::PlayerState) -> bool {
        if let Some(options) = self.furnace_options(player_runtime) {
            return options.filtering;
        }
        self.inventory_controller(player_runtime)
            .is_some_and(|controller| controller.options.filtering)
            && player_runtime.facts.player_game_mode() != Some(protocol::PlayerGameMode::Creative)
    }

    /// Projects one recipe-book page using the screen's filter choice.
    pub fn book_recipes(
        &self,
        player_runtime: &player_state::PlayerState,
        skip: usize,
        take: usize,
    ) -> Vec<inventory::BookRecipe> {
        player_runtime.inventory.book_recipes(
            self.recipe_filtering(player_runtime),
            !self.screen_state().search.is_empty(),
            skip,
            take,
        )
    }
}
