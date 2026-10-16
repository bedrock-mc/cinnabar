//! Environment variables this crate defines.

/// Opts a session into full packet capture: `1` writes `packets-<start unix ms>.cnbrcap` in
/// the log directory, any other non-empty value except `0` names the capture file.
pub const PACKET_CAPTURE: &str = "CINNABAR_PACKET_CAPTURE";
