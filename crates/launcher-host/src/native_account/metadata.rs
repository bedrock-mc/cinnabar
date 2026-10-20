//! Converts the shared pure device UUID at the concrete service SDK boundary.

use std::io;

/// Keeps service-specific formatting with the selected desktop account owner.
pub(super) fn service_device_id(xuid: &str) -> io::Result<bedrock_services::minecraft::DeviceId> {
    protocol::NativeDeviceProfile::service_device_id(xuid)
        .map(bedrock_services::minecraft::DeviceId::compact)
}
