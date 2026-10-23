//! Video recording: raw frames piped into an `ffmpeg` child in frame order, with captured
//! audio muxed in afterwards. Memory stays bounded however far the encoder falls behind: a
//! queued capture holds one shared image whatever its repeat count, the queue has a fixed
//! depth, and the writer waits for a missing readback only within a fixed reorder window.

use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use serde::{Deserialize, Serialize};

/// Captures buffered ahead of the encoder; beyond this, [`FrameSink::try_push`] refuses.
const QUEUE_FRAMES: usize = 8;

/// Later captures the writer holds while an earlier frame is missing. Readbacks arrive at
/// most a few frames out of order, so a frame still missing past this window was lost (an
/// unserved readback, or a real-time capture dropped while the encoder was behind).
const REORDER_FRAMES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

/// Byte order of a captured 8-bit, 4-channel frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelLayout {
    Rgba,
    Bgra,
}

impl PixelLayout {
    fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Rgba => "rgba",
            Self::Bgra => "bgra",
        }
    }
}

/// One tightly packed capture filling video slots `index..index + copies`; `index` (from zero)
/// orders out-of-order GPU readbacks. Real-time pacing repeats a slow frame through `copies`,
/// so a long stall still costs one image.
#[derive(Debug, Clone)]
pub struct Frame {
    pub index: u64,
    pub copies: u64,
    pub size: [u32; 2],
    pub layout: PixelLayout,
    pub data: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSettings {
    pub path: PathBuf,
    pub fps: u32,
    pub codec: Codec,
}

/// Locates `ffmpeg` on PATH.
pub fn find_ffmpeg() -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "ffmpeg was not found on PATH; install it (e.g. `brew install ffmpeg`) to record video"
                .to_owned()
        })
}

/// Encoder arguments for raw frames of `size` and `layout` on stdin at a constant rate.
pub fn ffmpeg_arguments(
    settings: &VideoSettings,
    size: [u32; 2],
    layout: PixelLayout,
    output: &Path,
) -> Vec<String> {
    let [width, height] = size;
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"]
        .map(str::to_owned)
        .into();
    args.extend(["-f", "rawvideo", "-pix_fmt", layout.ffmpeg_name()].map(str::to_owned));
    args.extend(["-video_size".to_owned(), format!("{width}x{height}")]);
    args.extend(["-framerate".to_owned(), settings.fps.to_string()]);
    args.extend(["-i", "-"].map(str::to_owned));
    let codec: &[&str] = match settings.codec {
        Codec::H264 => &["-c:v", "libx264", "-preset", "medium", "-crf", "16"],
        Codec::Hevc => &[
            "-c:v", "libx265", "-preset", "medium", "-crf", "18", "-tag:v", "hvc1",
        ],
    };
    args.extend(codec.iter().map(|arg| (*arg).to_owned()));
    // yuv420p needs even dimensions; crop a trailing odd row or column.
    args.extend(["-vf", "crop=trunc(iw/2)*2:trunc(ih/2)*2"].map(str::to_owned));
    args.extend(["-pix_fmt", "yuv420p", "-movflags", "+faststart"].map(str::to_owned));
    args.push(output.display().to_string());
    args
}

/// Arguments that copy `video` and encode `audio` to AAC into `output`.
pub fn mux_arguments(video: &Path, audio: &Path, output: &Path) -> Vec<String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
        .map(str::to_owned)
        .into();
    args.push(video.display().to_string());
    args.push("-i".into());
    args.push(audio.display().to_string());
    args.extend(
        [
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-movflags",
            "+faststart",
        ]
        .map(str::to_owned),
    );
    args.push(output.display().to_string());
    args
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    pub frames: u64,
    pub size: Option<[u32; 2]>,
    /// Slots whose capture never arrived, filled by repeating the previous frame.
    pub filled: u64,
}

/// Cloneable handle the capture path pushes frames through. Every clone shares one slot,
/// so finishing the [`Recorder`] closes them all and the encoder never waits on a straggler.
#[derive(Clone)]
pub struct FrameSink(Arc<Mutex<Option<Sender<Frame>>>>);

impl FrameSink {
    /// Blocks while the encoder is behind, so a fixed-clock capture never drops frames.
    pub fn push(&self, frame: Frame) -> Result<(), String> {
        self.sender()?
            .send(frame)
            .map_err(|_| ENCODER_EXITED.to_owned())
    }

    /// Queues `frame` without waiting; returns it when the encoder is [`QUEUE_FRAMES`] behind.
    pub fn try_push(&self, frame: Frame) -> Result<Option<Frame>, String> {
        match self.sender()?.try_send(frame) {
            Ok(()) => Ok(None),
            Err(TrySendError::Full(frame)) => Ok(Some(frame)),
            Err(TrySendError::Disconnected(_)) => Err(ENCODER_EXITED.to_owned()),
        }
    }

    fn sender(&self) -> Result<Sender<Frame>, String> {
        self.0
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or_else(|| "the recording has finished".to_owned())
    }

    fn close(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.take();
        }
    }
}

const ENCODER_EXITED: &str = "the encoder exited early; see ffmpeg's error output";

/// A running encode; ffmpeg starts with the first frame, whose size and layout it fixes.
pub struct Recorder {
    sink: FrameSink,
    writer: Option<JoinHandle<Result<Summary, String>>>,
    settings: VideoSettings,
    /// Video slots the capture side scheduled, published by [`Recorder::finish`].
    scheduled: Arc<AtomicU64>,
}

impl Recorder {
    pub fn start(settings: VideoSettings) -> Result<Self, String> {
        let ffmpeg = find_ffmpeg()?;
        if let Some(parent) = settings.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        Self::with_encoder(settings, move |settings, frames, scheduled| {
            encode(&ffmpeg, settings, frames, scheduled)
        })
    }

    /// Runs `encoder` on the writer thread over every pushed frame instead of ffmpeg, so
    /// tests can stand in a stalled or counting encoder.
    /// The encoder reads frames until every sink closes; the scheduled slot count it is
    /// handed is final once the frames end.
    pub fn with_encoder(
        settings: VideoSettings,
        encoder: impl FnOnce(&VideoSettings, &Receiver<Frame>, &AtomicU64) -> Result<Summary, String>
        + Send
        + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = crossbeam_channel::bounded(QUEUE_FRAMES);
        let thread_settings = settings.clone();
        let scheduled = Arc::new(AtomicU64::new(0));
        let thread_scheduled = Arc::clone(&scheduled);
        let writer = thread::Builder::new()
            .name("developer-recorder".into())
            .spawn(move || encoder(&thread_settings, &receiver, &thread_scheduled))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sink: FrameSink(Arc::new(Mutex::new(Some(sender)))),
            writer: Some(writer),
            settings,
            scheduled,
        })
    }

    pub fn settings(&self) -> &VideoSettings {
        &self.settings
    }

    pub fn sink(&self) -> FrameSink {
        self.sink.clone()
    }

    /// Closes every [`FrameSink`] clone and waits for ffmpeg; then muxes `audio` (a WAV) in.
    /// `scheduled` is every video slot the capture side assigned; slots whose capture was
    /// dropped or never arrived repeat the last frame, so the video spans the whole recording.
    pub fn finish(mut self, scheduled: u64, audio: Option<&Path>) -> Result<Summary, String> {
        self.scheduled.store(scheduled, Ordering::Release);
        self.sink.close();
        let summary = match self.writer.take() {
            Some(writer) => writer
                .join()
                .map_err(|_| "encoder thread panicked".to_owned())??,
            None => return Err("recording already finished".into()),
        };
        let Some(audio) = audio.filter(|_| summary.frames > 0) else {
            return Ok(summary);
        };
        mux::with_staged_video(&summary.path, find_ffmpeg, |ffmpeg, staged, output| {
            let status = Command::new(ffmpeg)
                .args(mux_arguments(staged, audio, output))
                .stdin(Stdio::null())
                .status()
                .map_err(|error| format!("start ffmpeg to mux audio: {error}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("muxing audio failed ({status})"))
            }
        })?;
        Ok(summary)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.sink.close();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

mod mux;

fn encode(
    ffmpeg: &Path,
    settings: &VideoSettings,
    frames: &Receiver<Frame>,
    scheduled: &AtomicU64,
) -> Result<Summary, String> {
    let mut frames = frames.iter().peekable();
    let Some(first) = frames.peek() else {
        return Ok(Summary {
            path: settings.path.clone(),
            frames: 0,
            size: None,
            filled: 0,
        });
    };
    let (size, layout) = (first.size, first.layout);
    let mut child = Command::new(ffmpeg)
        .args(ffmpeg_arguments(settings, size, layout, &settings.path))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("start ffmpeg: {error}"))?;
    let stdin = child.stdin.take().ok_or("ffmpeg stdin unavailable")?;
    let written = finish_child(child, stdin, |stdin| {
        write_ordered(stdin, frames, size, layout, || {
            scheduled.load(Ordering::Acquire)
        })
    })?;
    Ok(Summary {
        path: settings.path.clone(),
        frames: written.frames,
        size: Some(size),
        filled: written.filled,
    })
}

fn finish_child<T>(
    mut child: Child,
    mut stdin: ChildStdin,
    write: impl FnOnce(&mut ChildStdin) -> io::Result<T>,
) -> Result<T, String> {
    let written = write(&mut stdin).map_err(|error| error.to_string());
    drop(stdin);
    let status = child.wait().map_err(|error| error.to_string())?;
    let written = written?;
    if !status.success() {
        return Err(format!("ffmpeg exited with {status}"));
    }
    Ok(written)
}

/// Video slots written to the encoder, and how many repeated the previous frame for a lost capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Written {
    pub frames: u64,
    pub filled: u64,
}

/// Writes frames in index order from zero, holding at most [`REORDER_FRAMES`] early captures.
/// A slot still missing beyond that window, at the end, or up to `scheduled` (read once the
/// frames end) repeats the previous frame, so a lost capture neither stalls the stream nor
/// shortens the video; late arrivals are dropped.
pub(crate) fn write_ordered(
    out: &mut impl Write,
    frames: impl Iterator<Item = Frame>,
    size: [u32; 2],
    layout: PixelLayout,
    scheduled: impl FnOnce() -> u64,
) -> io::Result<Written> {
    let frame_bytes = size[0] as usize * size[1] as usize * 4;
    let mut order = Order::default();
    for frame in frames {
        if frame.size != size || frame.layout != layout || frame.data.len() != frame_bytes {
            return Err(io::Error::other(format!(
                "frame {} is {:?} {:?}, expected {size:?} {layout:?}; the window changed size",
                frame.index, frame.size, frame.layout
            )));
        }
        order.hold(frame);
        order.write_ready(out)?;
        while order.held.len() > REORDER_FRAMES {
            order.fill_gap(out)?;
            order.write_ready(out)?;
        }
    }
    while !order.held.is_empty() {
        order.fill_gap(out)?;
        order.write_ready(out)?;
    }
    order.fill_to(out, scheduled())?;
    Ok(order.written)
}

/// The writer's position, its early captures by index, and the last image written.
#[derive(Default)]
struct Order {
    next: u64,
    held: BTreeMap<u64, Frame>,
    last: Option<Arc<[u8]>>,
    written: Written,
}

impl Order {
    /// Keeps the slots of `frame` not yet written.
    fn hold(&mut self, mut frame: Frame) {
        let end = frame.index.saturating_add(frame.copies);
        if end <= self.next {
            return;
        }
        if frame.index < self.next {
            frame.copies = end - self.next;
            frame.index = self.next;
        }
        self.held.insert(frame.index, frame);
    }

    /// Writes every held capture that starts at the next slot.
    fn write_ready(&mut self, out: &mut impl Write) -> io::Result<()> {
        while let Some(frame) = self.held.remove(&self.next) {
            repeat(out, &frame.data, frame.copies)?;
            self.next += frame.copies;
            self.written.frames += frame.copies;
            self.last = Some(frame.data);
        }
        Ok(())
    }

    /// Fills the slots before the earliest held capture with the previous image (or that
    /// capture's, when nothing was written yet).
    fn fill_gap(&mut self, out: &mut impl Write) -> io::Result<()> {
        let Some((&index, frame)) = self.held.first_key_value() else {
            return Ok(());
        };
        let image = self.last.clone().unwrap_or_else(|| Arc::clone(&frame.data));
        self.fill(out, &image, index)
    }

    /// Repeats the last written image through slot `end`, once every capture is written.
    fn fill_to(&mut self, out: &mut impl Write, end: u64) -> io::Result<()> {
        match self.last.clone() {
            Some(image) if end > self.next => self.fill(out, &image, end),
            _ => Ok(()),
        }
    }

    fn fill(&mut self, out: &mut impl Write, image: &[u8], end: u64) -> io::Result<()> {
        let missing = end - self.next;
        repeat(out, image, missing)?;
        self.next = end;
        self.written.frames += missing;
        self.written.filled += missing;
        Ok(())
    }
}

fn repeat(out: &mut impl Write, data: &[u8], copies: u64) -> io::Result<()> {
    for _ in 0..copies {
        out.write_all(data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn frame(index: u64, fill: u8) -> Frame {
        Frame {
            index,
            copies: 1,
            size: [1, 1],
            layout: PixelLayout::Bgra,
            data: Arc::from([fill; 4]),
        }
    }

    fn settings() -> VideoSettings {
        VideoSettings {
            path: PathBuf::from("unused.mp4"),
            fps: 60,
            codec: Codec::H264,
        }
    }

    /// An encoder output the test can read while `write_ordered` is still consuming frames.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Shared {
        fn slots(&self) -> Vec<u8> {
            self.0
                .lock()
                .unwrap()
                .chunks(4)
                .map(|slot| slot[0])
                .collect()
        }
    }

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn readbacks_are_written_in_frame_order() {
        let frames = [frame(2, 2), frame(0, 0), frame(1, 1), frame(3, 3)];
        let mut out = Vec::new();
        let written = write_ordered(
            &mut out,
            frames.into_iter(),
            [1, 1],
            PixelLayout::Bgra,
            || 0,
        )
        .unwrap();
        assert_eq!(
            written,
            Written {
                frames: 4,
                filled: 0
            }
        );
        assert_eq!(out, [[0; 4], [1; 4], [2; 4], [3; 4]].concat());
    }

    #[test]
    fn a_repeated_capture_fills_its_slots_from_one_image() {
        let mut slow = frame(1, 7);
        slow.copies = 3;
        let mut out = Vec::new();
        let frames = [frame(0, 0), slow, frame(4, 4)].into_iter();
        let written = write_ordered(&mut out, frames, [1, 1], PixelLayout::Bgra, || 0).unwrap();
        assert_eq!(written.frames, 5);
        assert_eq!(out, [[0; 4], [7; 4], [7; 4], [7; 4], [4; 4]].concat());
    }

    #[test]
    fn a_lost_capture_is_filled_within_the_reorder_window() {
        let out = Shared::default();
        let seen = out.clone();
        let frames = (0..40u64).filter(|&index| index != 1).map(move |index| {
            let written = seen.slots().len() as u64;
            assert!(
                index <= REORDER_FRAMES as u64 + 2 || written + REORDER_FRAMES as u64 + 2 >= index,
                "frame {index} arrived with only {written} written: a lost capture held the stream"
            );
            frame(index, index as u8)
        });
        let written =
            write_ordered(&mut out.clone(), frames, [1, 1], PixelLayout::Bgra, || 0).unwrap();
        assert_eq!(
            written,
            Written {
                frames: 40,
                filled: 1
            }
        );
        let slots = out.slots();
        assert_eq!(
            slots[..3],
            [0, 0, 2],
            "the lost slot repeats the previous frame"
        );
        assert_eq!(slots[39], 39);
    }

    #[test]
    fn a_capture_after_its_slot_was_filled_is_dropped() {
        let late = frame(1, 1);
        let frames = std::iter::once(frame(0, 0))
            .chain((2..=REORDER_FRAMES as u64 + 2).map(|index| frame(index, index as u8)))
            .chain(std::iter::once(late));
        let out = Shared::default();
        let written =
            write_ordered(&mut out.clone(), frames, [1, 1], PixelLayout::Bgra, || 0).unwrap();
        assert_eq!(written.frames, REORDER_FRAMES as u64 + 3);
        assert_eq!(out.slots()[1], 0);
    }

    #[test]
    fn trailing_dropped_slots_repeat_the_last_frame_through_the_scheduled_end() {
        let mut out = Vec::new();
        let frames = [frame(0, 0), frame(1, 1), frame(2, 2)].into_iter();
        let written = write_ordered(&mut out, frames, [1, 1], PixelLayout::Bgra, || 6).unwrap();
        assert_eq!(
            written,
            Written {
                frames: 6,
                filled: 3
            }
        );
        assert_eq!(
            out,
            [[0; 4], [1; 4], [2; 4], [2; 4], [2; 4], [2; 4]].concat()
        );
    }

    #[test]
    fn finishing_hands_the_scheduled_slot_count_to_the_encoder() {
        let recorder = Recorder::with_encoder(settings(), |settings, frames, scheduled| {
            frames.iter().for_each(drop);
            Ok(Summary {
                path: settings.path.clone(),
                frames: scheduled.load(Ordering::Acquire),
                size: None,
                filled: 0,
            })
        })
        .unwrap();
        assert_eq!(recorder.finish(25, None).unwrap().frames, 25);
    }

    #[test]
    fn a_stalled_encoder_neither_blocks_nor_buffers_real_time_pushes() {
        let (release, gate) = crossbeam_channel::bounded::<()>(0);
        let recorder = Recorder::with_encoder(settings(), move |settings, frames, _| {
            let _ = gate.recv();
            Ok(Summary {
                path: settings.path.clone(),
                frames: frames.iter().count() as u64,
                size: None,
                filled: 0,
            })
        })
        .unwrap();
        let sink = recorder.sink();
        let done = Arc::new(AtomicBool::new(false));
        let refused = Arc::new(Mutex::new(0_u64));
        let (flag, count) = (Arc::clone(&done), Arc::clone(&refused));
        thread::spawn(move || {
            for index in 0..100 {
                if sink.try_push(frame(index, 0)).unwrap().is_some() {
                    *count.lock().unwrap() += 1;
                }
            }
            flag.store(true, Ordering::Release);
        });
        test_time::eventually("real-time pushes behind a stalled encoder", || {
            done.load(Ordering::Acquire)
        });
        assert_eq!(*refused.lock().unwrap(), 100 - QUEUE_FRAMES as u64);
        drop(release);
        assert_eq!(
            recorder.finish(100, None).unwrap().frames,
            QUEUE_FRAMES as u64
        );
    }

    #[test]
    fn dropping_a_recorder_with_live_sinks_finishes_the_encode() {
        let recorder = Recorder::with_encoder(settings(), |settings, frames, _| {
            Ok(Summary {
                path: settings.path.clone(),
                frames: frames.iter().count() as u64,
                size: None,
                filled: 0,
            })
        })
        .unwrap();
        let sink = recorder.sink();
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        thread::spawn(move || {
            drop(recorder);
            flag.store(true, Ordering::Release);
        });
        test_time::eventually("dropping the recorder with an outstanding sink", || {
            finished.load(Ordering::Acquire)
        });
        assert!(sink.push(frame(0, 0)).is_err());
    }

    #[test]
    fn resized_frames_are_rejected() {
        let mut resized = frame(1, 1);
        resized.size = [2, 1];
        let mut out = Vec::new();
        let frames = [frame(0, 0), resized].into_iter();
        assert!(write_ordered(&mut out, frames, [1, 1], PixelLayout::Bgra, || 0).is_err());
    }

    #[test]
    fn arguments_select_the_codec_layout_and_rate() {
        let settings = VideoSettings {
            path: PathBuf::from("clip.mp4"),
            fps: 60,
            codec: Codec::Hevc,
        };
        let args = ffmpeg_arguments(&settings, [1920, 1080], PixelLayout::Bgra, &settings.path);
        let joined = args.join(" ");
        assert!(joined.contains("-pix_fmt bgra -video_size 1920x1080 -framerate 60 -i -"));
        assert!(joined.contains("libx265"));
        assert_eq!(args.last().unwrap(), "clip.mp4");
        let mux = mux_arguments(Path::new("v.mp4"), Path::new("a.wav"), Path::new("o.mp4"));
        assert!(mux.join(" ").contains("-c:v copy -c:a aac"));
    }
}
