//! Sealed presentation attachment before the original catalogue worker begins.

use super::*;

impl Controller {
    /// Installs only an already validated sealed presentation fixture before its first catalogue read.
    pub(in crate::menu) fn install_fixture(
        &mut self,
        presentation: launcher_host::native_account::test_support::friend::Presentation,
        expected: &std::path::Path,
    ) -> std::io::Result<launcher::accounts::AccountProfile> {
        if !presentation.is_current() {
            return Err(std::io::Error::other("friend presenter identity retired"));
        }
        let (account, profile, path) = presentation.into_parts();
        if path != expected {
            return Err(std::io::Error::other(
                "friend presenter store differs from menu",
            ));
        }
        self.retire();
        self.selected = Some(Selection {
            account,
            path,
            identity: Some(profile.id.clone()),
        });
        Ok(profile)
    }
}
