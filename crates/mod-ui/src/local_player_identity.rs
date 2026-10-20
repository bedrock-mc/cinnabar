//! Read-only local identity bindings owned by the native host.

use std::sync::Arc;

use ui::mod_panel::local_player_head as identity;

use json_ui::{DataSource, Scalar};
use ui_engine::atlas::LOCAL_PLAYER_HEAD;

use crate::ModUi;

/// Values applied after extension bindings whenever a personal panel paints.
pub(super) struct LocalPlayerIdentity {
    name: String,
    available: bool,
}

impl Default for LocalPlayerIdentity {
    fn default() -> Self {
        Self {
            name: identity::DEFAULT_NAME.into(),
            available: false,
        }
    }
}

impl LocalPlayerIdentity {
    /// Writes reserved host values only when changed, preserving retained layout caches.
    pub(super) fn write(&self, data: &mut Arc<DataSource>) {
        let head = if self.available {
            LOCAL_PLAYER_HEAD
        } else {
            ""
        };
        if matches!(data.global_value(identity::NAME), Some(Scalar::Text(name)) if name == &self.name)
            && data.global_value(identity::AVAILABLE) == Some(&Scalar::Bool(self.available))
            && data.global_value(identity::UNAVAILABLE) == Some(&Scalar::Bool(!self.available))
            && matches!(data.global_value(identity::HEAD), Some(Scalar::Text(value)) if value == head)
        {
            return;
        }
        let values = [
            (identity::NAME, Scalar::Text(self.name.clone())),
            (identity::AVAILABLE, Scalar::Bool(self.available)),
            (identity::UNAVAILABLE, Scalar::Bool(!self.available)),
            (identity::HEAD, Scalar::Text(head.into())),
        ];
        let data = Arc::make_mut(data);
        for (key, value) in values {
            data.set_global(key, value);
        }
    }
}

impl ModUi {
    /// Publishes host identity without transferring skin texels to an extension.
    /// An absent portrait uses the neutral panel artwork and the name `Player`.
    pub fn set_local_player_panel_identity(&mut self, name: &str, available: bool) {
        let name = if available && !name.trim().is_empty() {
            name
        } else {
            identity::DEFAULT_NAME
        };
        if self.local_player_identity.name != name {
            self.local_player_identity.name = name.into();
        }
        self.local_player_identity.available = available;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_player_identity_overrides_guest_values_and_preserves_unchanged_data() {
        let mut host = ModUi::default();
        let mut guest = DataSource::new();
        guest.set_global(identity::NAME, Scalar::Text("Forged".into()));
        guest.set_global(identity::AVAILABLE, Scalar::Bool(true));
        let mut data = Arc::new(guest);
        host.local_player_identity.write(&mut data);
        assert_eq!(
            data.global_value(identity::NAME),
            Some(&Scalar::Text(identity::DEFAULT_NAME.into()))
        );
        assert_eq!(
            data.global_value(identity::HEAD),
            Some(&Scalar::Text(String::new()))
        );
        assert_eq!(
            data.global_value(identity::AVAILABLE),
            Some(&Scalar::Bool(false))
        );
        assert_eq!(
            data.global_value(identity::UNAVAILABLE),
            Some(&Scalar::Bool(true))
        );
        host.set_local_player_panel_identity("Current", true);
        host.local_player_identity.write(&mut data);
        assert_eq!(
            data.global_value(identity::NAME),
            Some(&Scalar::Text("Current".into()))
        );
        assert_eq!(
            data.global_value(identity::HEAD),
            Some(&Scalar::Text(LOCAL_PLAYER_HEAD.into()))
        );
        let retained = Arc::clone(&data);
        let (_, allocation) = test_alloc::measure(|| {
            host.set_local_player_panel_identity("Current", true);
            host.local_player_identity.write(&mut data);
        });
        assert_eq!(allocation.calls, 0);
        assert!(Arc::ptr_eq(&retained, &data));
        host.set_local_player_panel_identity("Current", false);
        host.local_player_identity.write(&mut data);
        assert_eq!(
            data.global_value(identity::NAME),
            Some(&Scalar::Text(identity::DEFAULT_NAME.into()))
        );
        assert_eq!(
            data.global_value(identity::AVAILABLE),
            Some(&Scalar::Bool(false))
        );
    }
}
