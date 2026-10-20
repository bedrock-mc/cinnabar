//! Native local-world selection and retirement on the original world worker.

use super::{
    LocalWorlds,
    client::{NativeLocalWorldRetirement, WorldsClient},
};
use launcher::install_layout::InstallLayout;
use launcher::local_worlds::model::{Effect, Input, WorldsMenu};
use std::io;

impl LocalWorlds {
    /// Selects native session routing while retaining the same manager/store/process worker.
    pub fn attach_native(&mut self, layout: &InstallLayout) -> io::Result<()> {
        let manager = self.attach_manager(layout)?;
        self.client = Some(WorldsClient::spawn_native(manager)?);
        self.playing = false;
        self.menu = WorldsMenu::default();
        self.input(Input::Refresh);
        self.dispatch(vec![Effect::LoadPrefs]);
        Ok(())
    }

    /// Captures a real selected opening only from the actual native world worker.
    pub fn native_selection(
        &mut self,
        id: &str,
    ) -> io::Result<launcher_host::local_worlds::NativeLocalWorldSelection> {
        if self.client.as_ref().is_none_or(|client| !client.native()) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "native world worker is unavailable",
            ));
        }
        let selection = self
            .manager
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "local world manager is unavailable",
                )
            })?
            .native_local_world(id)?;
        self.native_playing = Some(selection.clone());
        Ok(selection)
    }

    /// Captures cleanup on the same native worker, never a mutable latest-world lookup.
    pub fn native_retirement(
        &self,
        selection: launcher_host::local_worlds::NativeLocalWorldSelection,
    ) -> io::Result<NativeLocalWorldRetirement> {
        let client = self
            .client
            .as_ref()
            .filter(|client| client.native())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "native world worker is unavailable",
                )
            })?;
        Ok(client.native_retirement(selection))
    }

    /// Captures a real played opening; account and network work stay with the existing host worker.
    pub fn native_host_intent(&self) -> Option<launcher_host::local_worlds::NativeHostIntent> {
        if !self.playing || self.client.is_none() {
            return None;
        }
        self.manager.as_ref()?.native_host_intent().ok()
    }
}
