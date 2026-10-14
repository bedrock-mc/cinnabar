//! Renderer ownership at the session boundary.

use super::{MenuView, menu_reference};

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

    /// Only ordinary screens within play may resolve a server-restylable JSON-UI root.
    pub fn json_ui_menu_reference(&self) -> Option<&'static str> {
        if self.flow() != MenuFlow::Screen {
            return None;
        }
        let in_game = self.over_world || self.screen.requires_session();
        if in_game {
            menu_reference(self.screen)
        } else {
            None
        }
    }
}
