//! Bevy resource holding the account feed workers.

#[derive(bevy::prelude::Resource, bevy::prelude::Deref, bevy::prelude::DerefMut)]
pub struct LauncherAccount(pub launcher_host::launcher_account::LauncherAccount);
