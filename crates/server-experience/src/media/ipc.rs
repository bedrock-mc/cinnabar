//! Length-prefixed binary frames between the client and its media decoder process.

use super::{
    MAX_HEIGHT, MAX_PCM_FRAMES, MAX_WIDTH,
    descriptor::Descriptor,
    frames::{PcmBlock, VideoFrame},
    service::output::Output,
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const START: u8 = 1;
const CHUNK: u8 = 2;
const NEED: u8 = 10;
const VIDEO: u8 = 11;
const AUDIO: u8 = 12;
const END: u8 = 13;
const ERROR: u8 = 14;
const MAX_ERROR_BYTES: usize = 512;
/// Largest frame either side accepts: one maximum RGBA picture plus its header.
const MAX_FRAME_BYTES: usize = MAX_WIDTH as usize * MAX_HEIGHT as usize * 4 + 64;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub descriptor: Descriptor,
    pub start_us: u64,
}

/// Parent to helper.
#[derive(Debug)]
pub enum Request {
    Start(Box<Start>),
    Chunk { index: u32, bytes: Vec<u8> },
}

/// Helper to parent; decoder output carries no generation, the parent assigns its own.
#[derive(Debug)]
pub enum Reply {
    Need(u32),
    Output(Output),
    Error(String),
}

/// Writes one request; the start record is JSON, chunks are raw bytes.
pub fn write_request(writer: &mut impl Write, request: &Request) -> Result<()> {
    match request {
        Request::Start(start) => frame(writer, START, &[&serde_json::to_vec(start)?]),
        Request::Chunk { index, bytes } => frame(writer, CHUNK, &[&index.to_le_bytes(), bytes]),
    }
}

/// Reads one request, bounded before allocation.
pub fn read_request(reader: &mut impl Read) -> Result<Request> {
    let (tag, body) = read_frame(reader)?;
    Ok(match tag {
        START => {
            ensure!(
                body.len() <= 2 * crate::policy::MAX_MARKER_BYTES,
                "start too large"
            );
            Request::Start(Box::new(serde_json::from_slice(&body)?))
        }
        CHUNK => {
            ensure!(body.len() >= 4, "short chunk");
            Request::Chunk {
                index: u32::from_le_bytes(body[..4].try_into()?),
                bytes: body[4..].to_vec(),
            }
        }
        _ => bail!("unknown media request"),
    })
}

/// Writes one reply; PCM travels as little-endian f32.
pub fn write_reply(writer: &mut impl Write, reply: &Reply) -> Result<()> {
    match reply {
        Reply::Need(index) => frame(writer, NEED, &[&index.to_le_bytes()]),
        Reply::Output(Output::Video(frame_out)) => {
            let mut header = Vec::with_capacity(16);
            header.extend_from_slice(&frame_out.pts_us.to_le_bytes());
            header.extend_from_slice(&frame_out.width.to_le_bytes());
            header.extend_from_slice(&frame_out.height.to_le_bytes());
            frame(writer, VIDEO, &[&header, &frame_out.rgba])
        }
        Reply::Output(Output::Audio(block)) => {
            let mut body = Vec::with_capacity(9 + block.samples.len() * 4);
            body.extend_from_slice(&block.pts_us.to_le_bytes());
            body.push(block.channels);
            for sample in &block.samples {
                body.extend_from_slice(&sample.to_le_bytes());
            }
            frame(writer, AUDIO, &[&body])
        }
        Reply::Output(Output::End) => frame(writer, END, &[]),
        Reply::Error(text) => {
            let mut end = text.len().min(MAX_ERROR_BYTES);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            frame(writer, ERROR, &[&text.as_bytes()[..end]])
        }
    }
}

/// Reads one reply and tags decoder output with the parent's `generation`.
pub fn read_reply(reader: &mut impl Read, generation: u64) -> Result<Reply> {
    let (tag, body) = read_frame(reader)?;
    Ok(match tag {
        NEED => Reply::Need(u32::from_le_bytes(body.as_slice().try_into()?)),
        VIDEO => {
            ensure!(body.len() >= 16, "short video frame");
            let frame = VideoFrame {
                generation,
                pts_us: u64::from_le_bytes(body[..8].try_into()?),
                width: u32::from_le_bytes(body[8..12].try_into()?),
                height: u32::from_le_bytes(body[12..16].try_into()?),
                rgba: body[16..].to_vec(),
            };
            frame.validate(generation)?;
            Reply::Output(Output::Video(frame))
        }
        AUDIO => {
            ensure!(
                body.len() >= 9 && (body.len() - 9).is_multiple_of(4),
                "malformed PCM frame"
            );
            ensure!(
                (body.len() - 9) / 4 <= MAX_PCM_FRAMES * 2,
                "PCM frame too large"
            );
            let block = PcmBlock {
                generation,
                pts_us: u64::from_le_bytes(body[..8].try_into()?),
                channels: body[8],
                samples: body[9..]
                    .chunks_exact(4)
                    .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .collect(),
            };
            block.validate(generation)?;
            Reply::Output(Output::Audio(block))
        }
        END => Reply::Output(Output::End),
        ERROR => {
            ensure!(body.len() <= MAX_ERROR_BYTES, "error text too long");
            Reply::Error(String::from_utf8_lossy(&body).into_owned())
        }
        _ => bail!("unknown media reply"),
    })
}

fn frame(writer: &mut impl Write, tag: u8, parts: &[&[u8]]) -> Result<()> {
    let len: usize = 1 + parts.iter().map(|part| part.len()).sum::<usize>();
    ensure!(len <= MAX_FRAME_BYTES, "media IPC frame too large");
    writer.write_all(&u32::try_from(len)?.to_le_bytes())?;
    writer.write_all(&[tag])?;
    for part in parts {
        writer.write_all(part)?;
    }
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl Read) -> Result<(u8, Vec<u8>)> {
    let mut header = [0; 5];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header[..4].try_into()?) as usize;
    ensure!((1..=MAX_FRAME_BYTES).contains(&len), "media IPC frame size");
    let mut body = vec![0; len - 1];
    reader.read_exact(&mut body)?;
    Ok((header[4], body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_round_trip_and_inherit_the_parent_generation() {
        let mut bytes = Vec::new();
        let video = VideoFrame {
            generation: 9,
            pts_us: 40_000,
            width: 2,
            height: 2,
            rgba: vec![7; 16],
        };
        write_reply(&mut bytes, &Reply::Output(Output::Video(video))).unwrap();
        let audio = PcmBlock {
            generation: 9,
            pts_us: 20_000,
            channels: 2,
            samples: vec![0.5, -0.5, 0.25, -0.25],
        };
        write_reply(&mut bytes, &Reply::Output(Output::Audio(audio))).unwrap();
        write_reply(&mut bytes, &Reply::Need(3)).unwrap();
        write_reply(&mut bytes, &Reply::Error("é".repeat(400))).unwrap();
        let mut reader = bytes.as_slice();
        let Reply::Output(Output::Video(frame)) = read_reply(&mut reader, 4).unwrap() else {
            panic!("expected video");
        };
        assert_eq!(
            (frame.generation, frame.pts_us, frame.rgba.len()),
            (4, 40_000, 16)
        );
        let Reply::Output(Output::Audio(block)) = read_reply(&mut reader, 4).unwrap() else {
            panic!("expected audio");
        };
        assert_eq!(block.samples, [0.5, -0.5, 0.25, -0.25]);
        assert!(matches!(
            read_reply(&mut reader, 4).unwrap(),
            Reply::Need(3)
        ));
        let Reply::Error(text) = read_reply(&mut reader, 4).unwrap() else {
            panic!("expected error");
        };
        assert!(text.len() <= MAX_ERROR_BYTES);
    }

    #[test]
    fn oversized_or_invalid_frames_are_rejected_before_use() {
        let mut huge = (u32::MAX).to_le_bytes().to_vec();
        huge.push(VIDEO);
        assert!(read_reply(&mut huge.as_slice(), 1).is_err());
        let mut bytes = Vec::new();
        let bad = VideoFrame {
            generation: 1,
            pts_us: 0,
            width: 2,
            height: 2,
            rgba: vec![0; 15],
        };
        write_reply(&mut bytes, &Reply::Output(Output::Video(bad))).unwrap();
        assert!(read_reply(&mut bytes.as_slice(), 1).is_err());
        let mut loud = Vec::new();
        write_reply(
            &mut loud,
            &Reply::Output(Output::Audio(PcmBlock {
                generation: 1,
                pts_us: 0,
                channels: 1,
                samples: vec![2.0],
            })),
        )
        .unwrap();
        assert!(read_reply(&mut loud.as_slice(), 1).is_err());
    }
}
