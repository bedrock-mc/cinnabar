//! Renderer ownership at the session boundary.

use super::{MenuScreen, MenuView, menu_reference};

/// Declares the flow states and their exhaustive traversal together.
macro_rules! menu_flows {
    ($($name:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MenuFlow { $($name),+ }
        impl MenuFlow {
            /// All declared states, generated from the enum declaration.
            pub const ALL: &[Self] = &[$(Self::$name),+];
        }
    };
}
menu_flows! { Screen, LocalProgress, Connecting, Disconnect, SignIn, RealmMembership }

impl MenuScreen {
    /// Screens entered only from an active play session.
    pub fn requires_session(self) -> bool {
        match self {
            Self::Pause | Self::Death | Self::Invite => true,
            Self::Home
            | Self::Play
            | Self::Social
            | Self::Servers
            | Self::Profile
            | Self::DressingRoom
            | Self::Settings
            | Self::AddServer
            | Self::Inbox
            | Self::Friends => false,
        }
    }
}

impl MenuView {
    /// Resolves the screen-replacing flows in their presentation priority.
    pub fn flow(&self) -> MenuFlow {
        if self.local.progress.is_some() {
            MenuFlow::LocalProgress
        } else if self.connecting {
            MenuFlow::Connecting
        } else if self.disconnect_message.is_some() {
            MenuFlow::Disconnect
        } else if self.sign_in_prompt_open() {
            MenuFlow::SignIn
        } else if self.realm_membership.is_some() {
            MenuFlow::RealmMembership
        } else {
            MenuFlow::Screen
        }
    }

    /// Identifies client-owned flows outside a play session without changing session renderers.
    pub fn out_of_game(&self) -> bool {
        !self.over_world && !self.screen.requires_session()
    }

    /// Out-of-game flows never resolve a JSON-UI menu root; session bindings stay available.
    pub fn json_ui_menu_reference(&self) -> Option<&'static str> {
        if self.out_of_game() {
            None
        } else {
            menu_reference(self.screen)
        }
    }
}
