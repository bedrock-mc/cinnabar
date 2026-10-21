//! Launcher actions and immutable menu views shared with presentation.

use json_ui::vanilla::Reference;
pub mod auth;
pub mod death;
pub mod dialogs;
pub mod disconnect;
pub mod flow;
pub mod inbox;
pub mod invite;
pub mod join_progress;
pub mod join_requests;
mod list;
pub mod profile;
pub mod profile_achievements;
pub mod realm_membership;
pub mod server_list;
pub mod settings_options;
pub mod settings_sections;
pub mod settings_storage;
pub mod settings_support;
pub mod shader_pack_status;
pub mod sign_in;
pub mod updates;
pub mod view;
pub mod worlds_tab;

pub use list::MenuList;
pub use profile::{
    ProfileTab, profile_banner_index, profile_count_display, profile_minutes_display,
};
pub use view::{
    ButtonArt, CatalogFile, CatalogFriend, CatalogServer, EXPERIENCE_ADDRESS_PREFIX,
    FRIEND_ADDRESS_PREFIX, InboxItem, JoinKind, JoinProgress, JoinStage, LiveEventCard,
    LocalWorldCard, MenuCaret, MenuFeeds, MenuFriendCard, MenuGameCard, MenuHome, MenuProfile,
    MenuRealmCard, MenuServerCard, MenuView, PingInfo, SavedServer, ServerDetails,
    ServerTrustPrompt, pingable,
};
pub use worlds_tab::{LocalWorldAction, civil_date, file_size};

/// Declares menu routes and their exhaustive traversal from one source.
macro_rules! menu_screens {
    ($($(#[$attribute:meta])* $name:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MenuScreen { $($(#[$attribute])* $name),+ }
        impl MenuScreen {
            /// All declared routes, including routes added in future changes.
            pub const ALL: &[Self] = &[$(Self::$name),+];
            /// Screens entered only from an active play session.
            pub fn requires_session(self) -> bool {
                match self {
                    Self::Pause | Self::Death | Self::Invite => true,
                    Self::Home | Self::Play | Self::Social | Self::Servers | Self::Profile | Self::DressingRoom | Self::Settings | Self::AddServer | Self::Inbox | Self::Friends => false,
                }
            }
        }
    };
}
menu_screens! {
    Home, Play, Social, Servers, Profile, DressingRoom, Settings, AddServer,
    Pause, Death,
    /// OreUI-only screen.
    Inbox,
    /// OreUI-only screen.
    Friends,
    /// The invite screen over Pause while the world is hosted for friends.
    Invite,
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
    MinecraftImport,
    Accounts,
    Exit,
    DeathQuit,
    RemoveSaved(usize),
    StorageDelete,
    StorageError,
    /// A dismissible host notice whose body is supplied by the menu view.
    Notice,
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
    RealmCode,
    /// One command macro slot's command box.
    MacroCommand(usize),
}

/// One Video-section renderer choice. Shader packs carry their list revision so an old
/// picker action cannot select a different pack after discovery changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RendererChoice {
    /// The vanilla renderer ("Off").
    Vanilla,
    Enhanced,
    ShaderPack(u16, u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    MinecraftImport(crate::minecraft_import::Action),
    Update(updates::Action),
    OpenAccounts,
    AddAccount,
    SwitchAccount(usize),
    Inbox(inbox::Action),
    Navigate(MenuScreen),
    OpenExitDialog,
    ConfirmExit,
    DismissDialog,
    /// The quit dialog's header close control, independent of its Cancel button.
    CloseExitDialog,
    Reconnect,
    SelectServerTab(MenuServerTab),
    ServerList(server_list::ServerListAction),
    SelectProfileTab(ProfileTab),
    RefreshProfile,
    RefreshCatalog,
    StartSignIn,
    CancelSignIn,
    /// The sign-in header's close control, with feedback independent of the cancel button.
    CloseSignIn,
    OpenSignInLink,
    PlayAddServer,
    PlaySaved(usize),
    PlayFeatured(usize),
    PlayRealm(usize),
    /// Opens or advances Realm membership through an invite link or code.
    RealmMembership(realm_membership::Action),
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
    /// Applies one graphics preset without changing personal video preferences.
    SettingsVideoPreset(settings_options::video_presets::VideoPreset),
    SettingsDropdown(u16),
    SettingsLanguage(u16),
    SettingsKey(u16),
    SettingsResetKey(u16),
    SettingsResetBindings(bool),
    /// Presses a command macro's command text box.
    SettingsMacroCommand(usize),
    SettingsResetGroup(settings_options::SettingsGroup),
    SettingsConfirmResetBindings(bool),
    SettingsConfirmResetGroup(settings_options::SettingsGroup),
    SettingsResetChat,
    SettingsAdvancedGraphics,
    /// The JSON-UI Enhanced switch: Vanilla and Enhanced flip; a shader pack turns into Enhanced.
    ToggleRenderMode,
    /// The OreUI Video renderer picker.
    SelectRenderer(RendererChoice),
    PauseResume,
    PauseDisconnect,
    OpenDeathQuit,
    OpenDeathGameMenu,
    DeathExitWorld,
    ConfirmDeathQuit,
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
    GlobalResources(crate::global_resources::Action),
    /// A press on the Settings Mods tab.
    Mods(crate::mods::Action),
    DressingRoom(crate::dressing_room::Action),
    /// Answers the join's server trust prompt: "Trust and Join" (true) or "Don't Trust".
    ServerTrust(bool),
    /// A press on the pause screen's invite button or the invite screen.
    Invite(invite::Action),
    /// Answers the oldest Discord join request: Accept (true) or Decline.
    JoinRequest(bool),
    /// A press on the Shader Pack Settings screen or its Renderer-row button.
    ShaderOptions(crate::shader_options::Action),
}

impl MenuAction {
    /// The text field a press on this control focuses.
    pub fn text_field(self) -> Option<MenuField> {
        match self {
            Self::SettingsMacroCommand(slot) => Some(MenuField::MacroCommand(slot)),
            Self::RealmMembership(realm_membership::Action::EditCode) => Some(MenuField::RealmCode),
            Self::AddName => Some(MenuField::Name),
            Self::AddAddress => Some(MenuField::Address),
            Self::AddPort => Some(MenuField::Port),
            Self::EditSkinName => Some(MenuField::SkinName),
            Self::LocalWorld(action) => action.field(),
            _ => None,
        }
    }
}

/// Joining the featured-list entry at `index`.
pub fn play_featured(view: &MenuView, index: usize) -> Option<MenuAction> {
    (index < view.featured.len()).then_some(MenuAction::PlayFeatured(index))
}

/// The vanilla screen family used by both menu history and scene composition.
pub fn menu_reference(screen: MenuScreen) -> Option<&'static str> {
    Some(match screen {
        MenuScreen::Death => Reference::DeathDeathScreen.as_str(),
        MenuScreen::Pause => Reference::PausePauseScreen.as_str(),
        MenuScreen::Home => Reference::StartStartScreen.as_str(),
        MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers => {
            Reference::PlayPlayScreen.as_str()
        }
        MenuScreen::AddServer => Reference::AddExternalServerAddExternalServerScreenNew.as_str(),
        MenuScreen::Settings => Reference::SettingsScreenControlsAndSettings.as_str(),
        MenuScreen::Invite => Reference::InviteInviteScreen.as_str(),
        MenuScreen::Profile
        | MenuScreen::DressingRoom
        | MenuScreen::Inbox
        | MenuScreen::Friends => return None,
    })
}

/// The default Bedrock server port displayed by launcher address fields.
pub const DEFAULT_PORT: &str = bridge::contract::DEFAULT_BEDROCK_PORT_TEXT;

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

/// Maximum saved server name size in UTF-8 bytes.
pub const MAX_SERVER_NAME_BYTES: usize = 64;
/// Maximum saved server address size in UTF-8 bytes.
pub const MAX_SERVER_ADDRESS_BYTES: usize = 128;
