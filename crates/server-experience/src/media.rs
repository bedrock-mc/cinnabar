//! Host media contracts and a constrained, opt-in WebM decoder.

pub mod ceiling;
pub mod clock;
pub mod descriptor;
#[cfg(any(feature = "developer-media", test))]
mod faults;
pub mod frames;
pub mod helper;
pub mod ipc;
pub mod ranges;
pub mod service;
pub mod timeline;
#[cfg(feature = "developer-media")]
pub mod webm;
pub mod worker;

pub use service::output::Output;

pub const MAX_WIDTH: u32 = 1280;
pub const MAX_HEIGHT: u32 = 720;
pub const MAX_FPS: u32 = 30;
pub const MAX_FRAMES: usize = 6;
pub const MAX_COMPRESSED_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SAMPLE_BYTES: usize = 2 * 1024 * 1024;
pub const SAMPLE_RATE: u32 = 48_000;
pub const OPUS_PACKET_FRAMES: usize = SAMPLE_RATE as usize / 50;
pub const MAX_PCM_FRAMES: usize = SAMPLE_RATE as usize / 4;
pub const MAX_DURATION_US: u64 = 4 * 60 * 60 * 1_000_000;

/// Future MP4/H.264/AAC integration must implement the same validated output contract.
pub trait PlatformDecoder: Send {
    /// Reports a usable, policy-restricted platform decoder, never just codec presence.
    fn supports_mp4_h264_aac(&self) -> bool;
    /// Returns validated frames tagged with the caller-owned playback generation.
    fn decode(&mut self, bytes: &[u8], generation: u64) -> anyhow::Result<Vec<frames::VideoFrame>>;
}

pub struct UnavailablePlatformDecoder;

impl PlatformDecoder for UnavailablePlatformDecoder {
    /// No platform codec is advertised by the initial build.
    fn supports_mp4_h264_aac(&self) -> bool {
        false
    }

    /// Keeps unsupported containers on the poster path.
    fn decode(
        &mut self,
        _bytes: &[u8],
        _generation: u64,
    ) -> anyhow::Result<Vec<frames::VideoFrame>> {
        anyhow::bail!("MP4 platform decoder is not implemented")
    }
}
