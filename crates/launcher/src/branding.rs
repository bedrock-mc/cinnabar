//! Original embedded artwork shared by the launcher windows.

/// Title artwork drawn before game resources are installed.
pub const TITLE: &[u8] = include_bytes!("../../../assets/branding/title.png");
/// Rasterized from packaging/icons/cinnabar.svg, also used by the installer.
pub const ICON: &[u8] = include_bytes!("../../../assets/branding/icon.png");
