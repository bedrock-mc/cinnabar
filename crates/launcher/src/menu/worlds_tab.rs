//! Local-world control intents and their model inputs.
use super::{LocalWorldCard, MenuField};
use crate::local_worlds::{Input, PromptButton, Tab, game_mode_label, world_type_label};
use protocol::world_control::{Backend, Difficulty, GameMode, World};

/// A press on a local-world screen or modal; the menu forwards it to the module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalWorldAction {
    Edit(usize),
    BeginCreate,
    OpenTemplates,
    Back,
    Tab(Tab),
    /// Focuses the world name field (create or edit).
    NameField,
    SeedField,
    GameMode(GameMode),
    Difficulty(Difficulty),
    Flat(bool),
    Backend(Backend),
    Create,
    Save,
    Discard,
    PlayFromEdit,
    Delete,
    ConfirmDelete,
    AcceptEula,
    ViewEula,
    Prompt(PromptButton),
}

impl LocalWorldAction {
    /// The text field this press focuses, if any.
    pub fn field(self) -> Option<MenuField> {
        match self {
            Self::NameField => Some(MenuField::WorldName),
            Self::SeedField => Some(MenuField::WorldSeed),
            _ => None,
        }
    }

    /// Converts a launcher control into a local-world model intent.
    pub fn input(self) -> Option<Input> {
        Some(match self {
            Self::Edit(index) => Input::BeginEdit(index),
            Self::BeginCreate => Input::BeginCreate,
            Self::OpenTemplates => Input::OpenTemplates,
            Self::Back => Input::Back,
            Self::Tab(tab) => Input::SelectTab(tab),
            Self::NameField | Self::SeedField => return None,
            Self::GameMode(mode) => Input::SetGameMode(mode),
            Self::Difficulty(difficulty) => Input::SetDifficulty(difficulty),
            Self::Flat(flat) => Input::SetFlat(flat),
            Self::Backend(backend) => Input::SetBackend(backend),
            Self::Create => Input::SubmitCreate,
            Self::Save => Input::SubmitEdit,
            Self::Discard => Input::DiscardEdit,
            Self::PlayFromEdit => Input::PlayFromEdit,
            Self::Delete => Input::RequestDelete,
            Self::ConfirmDelete => Input::ConfirmDelete,
            Self::AcceptEula => Input::AcceptEula,
            Self::ViewEula => Input::OpenEulaLink,
            Self::Prompt(button) => button.input(),
        })
    }
}

/// Presents metadata from the core catalog consistently in Play and Storage.
pub fn world_card(world: &World) -> LocalWorldCard {
    LocalWorldCard {
        name: world.name.clone(),
        game_mode: game_mode_label(world.game_mode).to_owned(),
        world_type: world_type_label(world.generator).to_owned(),
        date: civil_date(world.last_played_unix.max(world.created_unix)),
        size: file_size(world.size_bytes),
    }
}

/// A world's size as the Worlds tab captions it: one decimal in KB, MB or GB.
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `month/day/year` of a UTC unix time; empty before the epoch.
pub fn civil_date(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    // Days-to-civil over 400-year eras (proleptic Gregorian).
    let days = unix / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{month}/{day}/{year}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_render_as_month_day_year() {
        assert_eq!(civil_date(0), "");
        assert_eq!(civil_date(951_782_400), "2/29/2000");
        assert_eq!(civil_date(1_790_553_600), "9/28/2026");
    }

    #[test]
    fn sizes_render_in_the_largest_whole_unit() {
        assert_eq!(file_size(0), "0.0 KB");
        assert_eq!(file_size(512 * 1024), "512.0 KB");
        assert_eq!(file_size(5 * 1024 * 1024 + 300 * 1024), "5.3 MB");
        assert_eq!(file_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
