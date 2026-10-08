//! Video recording: raw frames piped into an `ffmpeg` child in frame order, with captured
//! audio muxed in afterwards.

use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

/// Frames buffered ahead of the encoder before capture blocks the game loop.
const QUEUE_FRAMES: usize = 8;

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

/// One tightly packed frame; `index` (from zero) orders out-of-order GPU readbacks.
#[derive(Debug, Clone)]
pub struct Frame {
    pub index: u64,
    pub size: [u32; 2],
    pub layout: PixelLayout,
    pub data: Vec<u8>,
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
}

/// Cloneable handle the capture path pushes frames through. Every clone shares one slot,
/// so finishing the [`Recorder`] closes them all and the encoder never waits on a straggler.
#[derive(Clone)]
pub struct FrameSink(Arc<Mutex<Option<Sender<Frame>>>>);

impl FrameSink {
    /// Blocks while the encoder is behind, so a fixed-clock capture never drops frames.
    pub fn push(&self, frame: Frame) -> Result<(), String> {
        let sender = self.0.lock().ok().and_then(|slot| slot.clone());
        sender
            .ok_or_else(|| "the recording has finished".to_owned())?
            .send(frame)
            .map_err(|_| "the encoder exited early; see ffmpeg's error output".to_owned())
    }

    fn close(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.take();
        }
    }
}

/// A running encode; ffmpeg starts with the first frame, whose size and layout it fixes.
pub struct Recorder {
    sink: FrameSink,
    writer: Option<JoinHandle<Result<Summary, String>>>,
    settings: VideoSettings,
}

impl Recorder {
    pub fn start(settings: VideoSettings) -> Result<Self, String> {
        let ffmpeg = find_ffmpeg()?;
        if let Some(parent) = settings.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        Self::spawn(settings, move |settings, frames| {
            encode(&ffmpeg, settings, frames)
        })
    }

    /// Runs `encoder` on the writer thread over every pushed frame.
    fn spawn(
        settings: VideoSettings,
        encoder: impl FnOnce(&VideoSettings, &Receiver<Frame>) -> Result<Summary, String>
        + Send
        + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = crossbeam_channel::bounded(QUEUE_FRAMES);
        let thread_settings = settings.clone();
        let writer = thread::Builder::new()
            .name("developer-recorder".into())
            .spawn(move || encoder(&thread_settings, &receiver))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sink: FrameSink(Arc::new(Mutex::new(Some(sender)))),
            writer: Some(writer),
            settings,
        })
    }

    pub fn settings(&self) -> &VideoSettings {
        &self.settings
    }

    pub fn sink(&self) -> FrameSink {
        self.sink.clone()
    }

    /// Closes every [`FrameSink`] clone and waits for ffmpeg; then muxes `audio` (a WAV) in.
    pub fn finish(mut self, audio: Option<&Path>) -> Result<Summary, String> {
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
        let staged = video_staging_path(&summary.path);
        std::fs::rename(&summary.path, &staged).map_err(|error| error.to_string())?;
        let status = Command::new(find_ffmpeg()?)
            .args(mux_arguments(&staged, audio, &summary.path))
            .stdin(Stdio::null())
            .status()
            .map_err(|error| format!("start ffmpeg to mux audio: {error}"))?;
        if !status.success() {
            std::fs::rename(&staged, &summary.path).map_err(|error| error.to_string())?;
            return Err(format!(
                "muxing audio failed ({status}); the silent video is at {}",
                summary.path.display()
            ));
        }
        let _ = std::fs::remove_file(staged);
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

fn video_staging_path(path: &Path) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push(".video.mp4");
    path.with_file_name(name)
}

fn encode(
    ffmpeg: &Path,
    settings: &VideoSettings,
    frames: &Receiver<Frame>,
) -> Result<Summary, String> {
    let mut frames = frames.iter().peekable();
    let Some(first) = frames.peek() else {
        return Ok(Summary {
            path: settings.path.clone(),
            frames: 0,
            size: None,
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
        write_ordered(stdin, frames, size, layout)
    })?;
    Ok(Summary {
        path: settings.path.clone(),
        frames: written,
        size: Some(size),
    })
}

fn finish_child(
    mut child: Child,
    mut stdin: ChildStdin,
    write: impl FnOnce(&mut ChildStdin) -> io::Result<u64>,
) -> Result<u64, String> {
    let written = write(&mut stdin).map_err(|error| error.to_string());
    drop(stdin);
    let status = child.wait().map_err(|error| error.to_string())?;
    let written = written?;
    if !status.success() {
        return Err(format!("ffmpeg exited with {status}"));
    }
    Ok(written)
}

/// Writes frames strictly in index order from zero; gaps left at the end are closed up.
pub(crate) fn write_ordered(
    out: &mut impl Write,
    frames: impl Iterator<Item = Frame>,
    size: [u32; 2],
    layout: PixelLayout,
) -> io::Result<u64> {
    let frame_bytes = size[0] as usize * size[1] as usize * 4;
    let mut held = BTreeMap::new();
    let mut next = 0;
    let mut written = 0;
    for frame in frames {
        if frame.size != size || frame.layout != layout || frame.data.len() != frame_bytes {
            return Err(io::Error::other(format!(
                "frame {} is {:?} {:?}, expected {size:?} {layout:?}; the window changed size",
                frame.index, frame.size, frame.layout
            )));
        }
        held.insert(frame.index, frame.data);
        while let Some(data) = held.remove(&next) {
            out.write_all(&data)?;
            written += 1;
            next += 1;
        }
    }
    for data in held.into_values() {
        out.write_all(&data)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(index: u64, fill: u8) -> Frame {
        Frame {
            index,
            size: [1, 1],
            layout: PixelLayout::Bgra,
            data: vec![fill; 4],
        }
    }

    #[test]
    fn readbacks_are_written_in_frame_order() {
        let frames = [frame(2, 2), frame(0, 0), frame(1, 1), frame(3, 3)];
        let mut out = Vec::new();
        let written =
            write_ordered(&mut out, frames.into_iter(), [1, 1], PixelLayout::Bgra).unwrap();
        assert_eq!(written, 4);
        assert_eq!(out, [[0; 4], [1; 4], [2; 4], [3; 4]].concat());
    }

    #[test]
    fn dropping_a_recorder_with_live_sinks_finishes_the_encode() {
        let settings = VideoSettings {
            path: PathBuf::from("unused.mp4"),
            fps: 60,
            codec: Codec::H264,
        };
        let recorder = Recorder::spawn(settings, |settings, frames| {
            Ok(Summary {
                path: settings.path.clone(),
                frames: frames.iter().count() as u64,
                size: None,
            })
        })
        .unwrap();
        let sink = recorder.sink();
        let (done, finished) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            drop(recorder);
            let _ = done.send(());
        });
        assert!(
            finished
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok(),
            "dropping the recorder deadlocked on an outstanding sink"
        );
        assert!(sink.push(frame(0, 0)).is_err());
    }

    #[test]
    fn resized_frames_are_rejected() {
        let mut resized = frame(1, 1);
        resized.size = [2, 1];
        let mut out = Vec::new();
        let frames = [frame(0, 0), resized].into_iter();
        assert!(write_ordered(&mut out, frames, [1, 1], PixelLayout::Bgra).is_err());
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
        assert_eq!(
            video_staging_path(Path::new("out/clip.mp4")),
            Path::new("out/clip.video.mp4")
        );
    }
}
