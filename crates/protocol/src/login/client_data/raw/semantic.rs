//! Consuming relay metadata validation before any canonical identity overlay or credential mint.

use super::RawClientData;
use crate::login::client_data::NativeDeviceProfile;
use bridge::contract::{
    DISCONNECT_KEY_CANT_CONNECT, DISCONNECT_KEY_INVALID_NAME, DISCONNECT_KEY_INVALID_SKIN,
    DISCONNECT_KEY_OUTDATED_CLIENT, DISCONNECT_KEY_OUTDATED_SERVER,
};
use std::{fmt, io};

mod address;
mod json_generated;
mod locale;
mod locale_generated;
mod name;
mod skin;
mod unicode_generated;

/// A redacted metadata refusal; these presentation keys grant no account or transport authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientDataRefusal {
    /// Framing/types or independent invalid metadata cannot be repaired by one presentation change.
    Generic,
    /// The downstream protocol/version does not match the selected title version.
    OutdatedClient,
    /// The downstream protocol is newer than this selected title version.
    OutdatedServer,
    /// Replacing the whole skin aggregate makes all metadata valid.
    InvalidSkin,
    /// Replacing only the name makes the original metadata valid.
    InvalidName,
}

impl ClientDataRefusal {
    /// Uses the same generated presentation contract as the existing Go admission consumer.
    pub const fn disconnect_key(self) -> &'static str {
        match self {
            Self::Generic => DISCONNECT_KEY_CANT_CONNECT,
            Self::OutdatedClient => DISCONNECT_KEY_OUTDATED_CLIENT,
            Self::OutdatedServer => DISCONNECT_KEY_OUTDATED_SERVER,
            Self::InvalidSkin => DISCONNECT_KEY_INVALID_SKIN,
            Self::InvalidName => DISCONNECT_KEY_INVALID_NAME,
        }
    }
}

impl fmt::Display for ClientDataRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("native client metadata refused")
    }
}

impl std::error::Error for ClientDataRefusal {}

/// Metadata accepted by the title validator under one owned device profile, without an identity.
/// It cannot be decoded or constructed directly, and its final raw claims require consumption.
/// Incoming addresses never grant authority; DNS/OS-service resolution is deliberately omitted.
/// Raw lexeme retention does not promise Go's normalization of arbitrary duplicate known claims.
#[must_use]
pub struct ValidatedClientData {
    raw: RawClientData,
    name: String,
    derived_id: uuid::Uuid,
}

impl fmt::Debug for ValidatedClientData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValidatedClientData")
            .finish_non_exhaustive()
    }
}

impl ValidatedClientData {
    /// Returns requested presentation after validation, never a verified account name or XUID.
    pub fn requested_name(&self) -> &str {
        &self.name
    }

    /// Applies the caller's canonical account name, or the validated requested guest name.
    /// Account verification and final transport address/nonce remain their existing owners.
    ///
    /// # Errors
    /// Returns a redacted error if canonical claim serialization fails.
    pub fn into_relay_claims(mut self, canonical_name: &str) -> io::Result<RawClientData> {
        NativeDeviceProfile::apply_relay_identity(&mut self.raw, canonical_name, self.derived_id)?;
        Ok(self.raw)
    }
}

/// Retains Go's typed -> owned device -> version -> original metadata phase order.
pub(super) fn validate(
    mut raw: RawClientData,
    profile: &NativeDeviceProfile,
    protocol: i32,
) -> Result<ValidatedClientData, ClientDataRefusal> {
    profile
        .apply_relay_device(&mut raw)
        .map_err(|_| ClientDataRefusal::Generic)?;
    let version = raw
        .game_version()
        .map_err(|_| ClientDataRefusal::Generic)?
        .unwrap_or_default();
    if protocol != crate::PROTOCOL_VERSION || version != crate::GAME_VERSION {
        return Err(if protocol > crate::PROTOCOL_VERSION {
            ClientDataRefusal::OutdatedServer
        } else {
            ClientDataRefusal::OutdatedClient
        });
    }
    let view = View::read(&raw).map_err(|_| ClientDataRefusal::Generic)?;
    view.check()?;
    let random_id = raw
        .random_id()
        .map_err(|_| ClientDataRefusal::Generic)?
        .unwrap_or_default();
    Ok(ValidatedClientData {
        raw,
        name: view.name,
        derived_id: profile.relay_identity(random_id),
    })
}

struct View {
    device_os: i64,
    language: String,
    offline_id: String,
    online_id: String,
    self_signed_id: String,
    address: String,
    playfab: String,
    ui: i64,
    name: String,
    skin: skin::Skin,
}

impl View {
    /// Projects only values actually consumed by Validate; exhaustive JSON types have another owner.
    fn read(raw: &RawClientData) -> io::Result<Self> {
        Ok(Self {
            device_os: raw.integer("DeviceOS")?.unwrap_or_default(),
            language: raw.read("LanguageCode")?.unwrap_or_default(),
            offline_id: raw.read("PlatformOfflineId")?.unwrap_or_default(),
            online_id: raw.read("PlatformOnlineId")?.unwrap_or_default(),
            self_signed_id: raw.read("SelfSignedId")?.unwrap_or_default(),
            address: raw.read("ServerAddress")?.unwrap_or_default(),
            playfab: raw.read("PlayFabId")?.unwrap_or_default(),
            ui: raw.integer("UIProfile")?.unwrap_or_default(),
            name: raw.read("ThirdPartyName")?.unwrap_or_default(),
            skin: skin::Skin::read(raw)?,
        })
    }

    /// Replaces whole skin, then whole name, while retaining every independent invalid field.
    fn check(&self) -> Result<(), ClientDataRefusal> {
        let other = (1..=15).contains(&self.device_os)
            && locale::valid(&self.language)
            && optional_uuid(&self.offline_id)
            && (self.online_id.is_empty()
                || self.online_id.bytes().all(|byte| byte.is_ascii_digit())
                    && self.online_id.parse::<u64>().is_ok())
            && optional_uuid(&self.self_signed_id)
            && address::valid(&self.address)
            && self
                .playfab
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && (0..=2).contains(&self.ui);
        let skin = self.skin.valid();
        let name = name::valid(&self.name);
        if other && skin && name {
            Ok(())
        } else if other && name {
            // A valid replacement changes the entire skin aggregate and no other field.
            Err(ClientDataRefusal::InvalidSkin)
        } else if other && skin {
            // This tests the original skin with only the name replaced, not the previous replacement.
            Err(ClientDataRefusal::InvalidName)
        } else {
            Err(ClientDataRefusal::Generic)
        }
    }
}

/// Matches google/uuid.Parse's accepted encodings, including its unchecked38-byte wrapper bytes.
fn optional_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    let body = match bytes.len() {
        0 => return true,
        32 | 36 => bytes,
        38 => &bytes[1..37],
        45 if bytes[..9].eq_ignore_ascii_case(b"urn:uuid:") => &bytes[9..],
        _ => return false,
    };
    std::str::from_utf8(body).is_ok_and(|body| uuid::Uuid::parse_str(body).is_ok())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod limits_tests;
