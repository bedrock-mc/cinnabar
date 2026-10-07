//! A loopback control surface for driving a developer build of the client: the wire
//! protocol, its token-authenticated server and controller, and the recording primitives.

pub mod camera;
pub mod client;
pub mod clock;
pub mod endpoint;
pub mod input;
pub mod protocol;
pub mod recorder;
pub mod server;
pub mod wav;

/// Names the endpoint file a client should publish; unset leaves the control server off.
pub const ENDPOINT_ENV: &str = "CINNABAR_DEVELOPER_CONTROL";
/// `WIDTHxHEIGHT` in physical pixels for the window, at scale factor 1.
pub const WINDOW_SIZE_ENV: &str = "CINNABAR_WINDOW_SIZE";
/// `1` creates the window hidden, for headless capture.
pub const HIDDEN_WINDOW_ENV: &str = "CINNABAR_HIDDEN_WINDOW";
/// Endpoint files live here under the install's `.local` data root.
pub const ENDPOINT_DIR: &str = "developer-control";

/// Parses `WIDTHxHEIGHT`.
pub fn parse_size(value: &str) -> Option<[u32; 2]> {
    let (width, height) = value.trim().split_once(['x', 'X'])?;
    let size = [width.parse().ok()?, height.parse().ok()?];
    size.iter()
        .all(|&side| (16..=16_384).contains(&side))
        .then_some(size)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_parse_within_bounds() {
        assert_eq!(super::parse_size("1920x1080"), Some([1920, 1080]));
        assert_eq!(super::parse_size("0x1080"), None);
        assert_eq!(super::parse_size("1920"), None);
    }
}
