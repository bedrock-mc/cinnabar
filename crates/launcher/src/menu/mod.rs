//! Launcher actions and immutable menu views shared with presentation.

pub mod auth;
pub mod disconnect;
pub mod inbox;
pub mod profile;
pub mod profile_achievements;
pub mod server_list;
pub mod settings_options;
pub mod settings_storage;
pub mod settings_support;
pub mod view;
pub mod worlds_tab;

pub use profile::{
    ProfileTab, profile_banner_index, profile_count_display, profile_minutes_display,
};
pub use view::{
    ButtonArt, CatalogFile, CatalogFriend, EXPERIENCE_ADDRESS_PREFIX, InboxItem, JoinKind,
    JoinProgress, JoinStage, LiveEventCard, LocalWorldCard, MenuCaret, MenuFeeds, MenuFriendCard,
    MenuGameCard, MenuHome, MenuProfile, MenuRealmCard, MenuServerCard, MenuView, PingInfo,
    SavedServer, ServerDetails, ServerTrustPrompt, pingable,
};
pub use worlds_tab::{LocalWorldAction, civil_date, file_size};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuScreen {
    Home,
    Play,
    Social,
    Servers,
    Profile,
    DressingRoom,
    Settings,
    AddServer,
    Pause,
    Death,
    /// OreUI-only screens.
    Inbox,
    Friends,
    /// The Marketplace; its content is owned by [`crate::store`].
    Store,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuServerTab {
    Featured,
    Favorites,
    Recent,
    Saved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuDialog {
    ServerFilter,
    Accounts,
    Exit,
    RemoveSaved(usize),
    StorageDelete,
    StorageError,
    SettingsSupport(settings_support::SupportDialog),
    SettingsResetBindings(bool),
    SettingsResetGroup(settings_options::SettingsGroup),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuField {
    Name,
    Address,
    Port,
    /// The local-world create or edit screen's name field.
    WorldName,
    WorldSeed,
    SkinName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    OpenAccounts,
    AddAccount,
    SwitchAccount(usize),
    Inbox(inbox::Action),
    Navigate(MenuScreen),
    OpenExitDialog,
    OpenServerFilter,
    ConfirmExit,
    DismissDialog,
    SelectServerTab(MenuServerTab),
    ServerList(server_list::ServerListAction),
    SelectProfileTab(ProfileTab),
    RefreshProfile,
    RefreshCatalog,
    StartSignIn,
    CancelSignIn,
    PlayAddServer,
    PlaySaved(usize),
    PlayFeatured(usize),
    PlayRealm(usize),
    PlayFriend(usize),
    ToggleFavorite(usize),
    RemoveSavedDialog(usize),
    ConfirmRemoveSaved(usize),
    AddName,
    AddAddress,
    AddPort,
    EditSkinName,
    AddSave,
    AddSaveConnect,
    AddBack,
    SettingsScale(i8),
    SettingsScalePicker,
    SettingsFullscreen(bool),
    SettingsStorage(settings_storage::StorageAction),
    SettingsSupport(settings_support::SupportAction),
    SettingsOption(u16, i32),
    SettingsDropdown(u16),
    SettingsLanguage(u16),
    SettingsKey(u16),
    SettingsResetKey(u16),
    SettingsResetBindings(bool),
    SettingsResetGroup(settings_options::SettingsGroup),
    SettingsConfirmResetBindings(bool),
    SettingsConfirmResetGroup(settings_options::SettingsGroup),
    SettingsResetChat,
    SettingsAdvancedGraphics,
    ToggleRenderMode,
    PauseResume,
    PauseDisconnect,
    PauseSettings,
    /// Load a saved server into the add/edit draft.
    EditSaved(usize),
    /// Pick a settings section by its selector index.
    SettingsSection(u8),
    Respawn,
    PlayLocalWorld(usize),
    /// A press on a local-world screen (create, edit, templates) or its modals.
    LocalWorld(LocalWorldAction),
    SignOut,
    /// Show a featured server in the Servers tab's info panel.
    SelectFeatured(usize),
    /// Show a saved server's details on the Servers tab.
    SelectSaved(usize),
    /// Show a Realm's details on the Realms tab.
    SelectRealm(usize),
    /// Flip the info panel's description (0) or news (1) past "read more".
    ToggleReadMore(u8),
    /// The start screen's live-event button.
    OpenLiveEvent,
    /// A press on a Marketplace screen.
    Store(crate::store::StoreAction),
    GlobalResources(crate::global_resources::Action),
    DressingRoom(crate::dressing_room::Action),
    /// Answers the join's server trust prompt: "Trust and Join" (true) or "Don't Trust".
    ServerTrust(bool),
}

impl MenuAction {
    /// The text field a press on this control focuses.
    pub fn text_field(self) -> Option<MenuField> {
        match self {
            Self::AddName => Some(MenuField::Name),
            Self::AddAddress => Some(MenuField::Address),
            Self::AddPort => Some(MenuField::Port),
            Self::EditSkinName => Some(MenuField::SkinName),
            Self::LocalWorld(action) => action.field(),
            _ => None,
        }
    }
}

/// The vanilla screen family used by both menu history and scene composition.
pub fn menu_reference(screen: MenuScreen) -> Option<&'static str> {
    Some(match screen {
        MenuScreen::Death => "death.death_screen",
        MenuScreen::Pause => "pause.pause_screen",
        MenuScreen::Home => "start.start_screen",
        MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers => "play.play_screen",
        MenuScreen::AddServer => "add_external_server.add_external_server_screen_new",
        MenuScreen::Settings => SETTINGS_SCREEN,
        MenuScreen::Store => crate::store::SDL_SCREEN,
        MenuScreen::Profile
        | MenuScreen::DressingRoom
        | MenuScreen::Inbox
        | MenuScreen::Friends => return None,
    })
}

/// The default Bedrock server port displayed by launcher address fields.
pub const DEFAULT_PORT: &str = "19132";

/// The vanilla screen shared by launcher and in-world settings.
pub const SETTINGS_SCREEN: &str = "settings.screen_controls_and_settings";

/// Host and port of a saved `host:port`; a bare host gets the default port.
pub fn split_address(address: &str) -> (String, String) {
    let literal = address
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(address);
    if literal.parse::<std::net::Ipv6Addr>().is_ok() {
        return (literal.to_owned(), DEFAULT_PORT.to_owned());
    }
    match address.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            (host.trim_matches(['[', ']']).to_owned(), port.to_owned())
        }
        _ => (address.to_owned(), DEFAULT_PORT.to_owned()),
    }
}

#[cfg(test)]
mod address_tests;
