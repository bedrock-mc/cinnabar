//! Construction of the session worker's owned login inputs.

#[cfg(feature = "native-networking")]
use super::NativeConnectionPreparation;
use super::NetworkConfig;
use simulation_clock::identity::SessionGeneration;

impl NetworkConfig {
    /// Retains the caller's real login facts, with optional capture and account preparation unset.
    pub fn new(
        session_generation: SessionGeneration,
        endpoint: protocol::SessionEndpoint,
        display_name: String,
        client_random_id: protocol::ClientRandomId,
        client_blob_cache: protocol::ClientBlobCache,
        player_skin: protocol::ClientSkin,
        login_settings: protocol::LoginSettings,
    ) -> Self {
        Self {
            session_generation,
            endpoint,
            #[cfg(feature = "native-networking")]
            native_preparation: None,
            display_name,
            client_random_id,
            client_blob_cache,
            player_skin,
            login_settings,
            packet_capture: None,
        }
    }

    /// Moves the original account acquisition into the same session worker's cancellation lifetime.
    #[must_use]
    #[cfg(feature = "native-networking")]
    pub fn with_native_preparation(
        mut self,
        preparation: Option<NativeConnectionPreparation>,
    ) -> Self {
        self.native_preparation = preparation;
        self
    }
}
