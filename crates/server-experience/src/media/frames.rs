//! Validation shared by decoder output, IPC, texture upload and audio submission.

use super::*;
use anyhow::{Result, ensure};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub generation: u64,
    pub pts_us: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl VideoFrame {
    /// Validates helper output again before any GPU copy.
    pub fn validate(&self, generation: u64) -> Result<()> {
        ensure!(
            self.generation == generation && self.pts_us <= MAX_DURATION_US,
            "stale video frame"
        );
        ensure!(
            self.width > 0
                && self.width <= MAX_WIDTH
                && self.height > 0
                && self.height <= MAX_HEIGHT,
            "invalid video dimensions"
        );
        ensure!(
            self.rgba.len() == self.width as usize * self.height as usize * 4,
            "invalid RGBA stride or length"
        );
        Ok(())
    }
}

#[derive(Debug)]
pub struct PcmBlock {
    pub generation: u64,
    pub pts_us: u64,
    pub channels: u8,
    pub samples: Vec<f32>,
}

impl PcmBlock {
    /// Rejects invalid PCM before it enters the real-time mixer ring.
    pub fn validate(&self, generation: u64) -> Result<()> {
        ensure!(
            self.generation == generation && self.pts_us <= MAX_DURATION_US,
            "stale PCM"
        );
        ensure!((1..=2).contains(&self.channels), "invalid PCM channels");
        ensure!(
            self.samples
                .len()
                .is_multiple_of(usize::from(self.channels))
                && self.samples.len() <= MAX_PCM_FRAMES * usize::from(self.channels),
            "PCM queue budget exceeded"
        );
        ensure!(
            self.samples
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0),
            "invalid PCM samples"
        );
        Ok(())
    }
}

#[derive(Default)]
pub struct FrameQueue {
    frames: VecDeque<VideoFrame>,
}

impl FrameQueue {
    /// Lets a consumer stop polling the decoder before the frame ceiling is reached.
    pub fn has_capacity(&self) -> bool {
        self.frames.len() < MAX_FRAMES
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Sorts by presentation timestamp, with a fixed queue ceiling.
    pub fn push(&mut self, frame: VideoFrame, generation: u64) -> Result<()> {
        frame.validate(generation)?;
        ensure!(self.frames.len() < MAX_FRAMES, "video queue full");
        let index = self
            .frames
            .iter()
            .position(|f| f.pts_us > frame.pts_us)
            .unwrap_or(self.frames.len());
        self.frames.insert(index, frame);
        Ok(())
    }

    /// Holds early frames and discards superseded late frames without touching decoder references.
    pub fn present(&mut self, clock_us: u64, generation: u64) -> Option<VideoFrame> {
        self.frames.retain(|frame| frame.generation == generation);
        let mut selected = None;
        while self
            .frames
            .front()
            .is_some_and(|frame| frame.pts_us <= clock_us)
        {
            selected = self.frames.pop_front();
        }
        selected
    }
}

/// Converts the constrained BT.709 limited-range 8-bit 4:2:0 profile to sRGB RGBA.
pub fn bt709_rgba(
    width: u32,
    height: u32,
    planes: [&[u8]; 3],
    strides: [usize; 3],
) -> Result<Vec<u8>> {
    ensure!(
        width > 0
            && height > 0
            && width <= MAX_WIDTH
            && height <= MAX_HEIGHT
            && width.is_multiple_of(2)
            && height.is_multiple_of(2),
        "invalid YUV extent"
    );
    let w = width as usize;
    let h = height as usize;
    for index in 0..3 {
        let (columns, rows) = if index == 0 { (w, h) } else { (w / 2, h / 2) };
        ensure!(
            strides[index] >= columns
                && strides[index] <= MAX_WIDTH as usize * 4
                && planes[index].len() >= strides[index] * (rows - 1) + columns,
            "invalid YUV plane stride"
        );
    }
    let lut = SRGB_LUT.get_or_init(|| std::array::from_fn(|i| encode(i as f32 / LUT_MAX as f32)));
    let mut rgba = vec![0; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = (f32::from(planes[0][row * strides[0] + col]) - 16.0) / 219.0;
            let u = (f32::from(planes[1][row / 2 * strides[1] + col / 2]) - 128.0) / 224.0;
            let v = (f32::from(planes[2][row / 2 * strides[2] + col / 2]) - 128.0) / 224.0;
            let offset = (row * w + col) * 4;
            rgba[offset..offset + 4].copy_from_slice(&[
                srgb(lut, y + 1.5748 * v),
                srgb(lut, y - 0.1873 * u - 0.4681 * v),
                srgb(lut, y + 1.8556 * u),
                255,
            ]);
        }
    }
    Ok(rgba)
}

const LUT_MAX: usize = 4095;
static SRGB_LUT: std::sync::OnceLock<[u8; LUT_MAX + 1]> = std::sync::OnceLock::new();

/// Looks up the transfer conversion; per-pixel powf is too slow for 720p in real time.
fn srgb(lut: &[u8; LUT_MAX + 1], value: f32) -> u8 {
    lut[(value.clamp(0.0, 1.0) * LUT_MAX as f32).round() as usize]
}

/// Converts the BT.709 transfer curve to the renderer's sRGB texture encoding.
fn encode(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let linear = if value < 0.081 {
        value / 4.5
    } else {
        ((value + 0.099) / 1.099).powf(1.0 / 0.45)
    };
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_black_and_white_and_truncated_planes() {
        let black = bt709_rgba(2, 2, [&[16; 4], &[128], &[128]], [2, 1, 1]).unwrap();
        assert_eq!(black, [0, 0, 0, 255].repeat(4));
        let white = bt709_rgba(2, 2, [&[235; 4], &[128], &[128]], [2, 1, 1]).unwrap();
        assert_eq!(white, [255; 4].repeat(4));
        assert!(bt709_rgba(2, 2, [&[16; 3], &[128], &[128]], [2, 1, 1]).is_err());
    }
}
