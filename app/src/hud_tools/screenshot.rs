//! Frame capture: one shared readback per frame, F2 screenshots encoded off-thread and
//! reported in chat.

use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use crossbeam_channel::{Receiver, Sender};

use client_ui::ui_runtime::UiRuntime;

type SaveResult = Result<String, String>;
type CaptureConsumer = Box<dyn FnOnce(&Image) + Send + Sync>;

/// Everything that wants this frame's pixels. Bevy silently drops a second capture of one
/// window in a frame, so a single readback serves every consumer.
#[derive(Resource, Default)]
pub(crate) struct FrameCapture(Vec<CaptureConsumer>);

impl FrameCapture {
    pub(crate) fn request(&mut self, consumer: impl FnOnce(&Image) + Send + Sync + 'static) {
        self.0.push(Box::new(consumer));
    }
}

/// Requests made before this set are served by the frame being rendered.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FrameCaptureSet;

pub(crate) fn configure_frame_capture(app: &mut App) {
    app.init_resource::<FrameCapture>()
        .add_systems(Last, spawn_frame_capture.in_set(FrameCaptureSet));
}

fn spawn_frame_capture(mut capture: ResMut<FrameCapture>, mut commands: Commands) {
    if capture.0.is_empty() {
        return;
    }
    let mut consumers = std::mem::take(&mut capture.0);
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>| {
            for consumer in consumers.drain(..) {
                consumer(&captured.image);
            }
        },
    );
}

#[derive(Resource)]
struct ScreenshotChannel {
    dir: PathBuf,
    sender: Sender<SaveResult>,
    receiver: Receiver<SaveResult>,
}

pub(super) fn configure(app: &mut App, dir: PathBuf) {
    configure_frame_capture(app);
    let (sender, receiver) = crossbeam_channel::unbounded();
    app.insert_resource(ScreenshotChannel {
        dir,
        sender,
        receiver,
    })
    .add_systems(
        Update,
        (capture_on_key, report_saved)
            .chain()
            .before(crate::app::ClientFrameSet::UiPreparation),
    );
    if let Some(path) = std::env::var_os("CINNABAR_CAPTURE_PATH") {
        let frames = std::env::var("CINNABAR_CAPTURE_AFTER_FRAMES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(240);
        app.insert_resource(EnvCapture {
            path: PathBuf::from(path),
            frames,
            done: None,
        })
        .add_systems(Update, capture_from_env);
    }
}

/// Dev capture: `CINNABAR_CAPTURE_PATH` saves the window after
/// `CINNABAR_CAPTURE_AFTER_FRAMES` frames (default 240) and exits.
#[derive(Resource)]
struct EnvCapture {
    path: PathBuf,
    frames: u32,
    done: Option<Receiver<SaveResult>>,
}

fn capture_from_env(
    mut capture: ResMut<EnvCapture>,
    mut frames: ResMut<FrameCapture>,
    mut exits: MessageWriter<AppExit>,
) {
    if let Some(done) = &capture.done {
        if let Ok(result) = done.try_recv() {
            eprintln!("capture: {result:?}");
            exits.write(if result.is_ok() {
                AppExit::Success
            } else {
                AppExit::error()
            });
        }
        return;
    }
    if capture.frames > 0 {
        capture.frames -= 1;
        return;
    }
    let (sender, receiver) = crossbeam_channel::bounded(1);
    let path = capture.path.clone();
    frames.request(move |image| {
        let image = image.clone();
        std::thread::spawn(move || {
            let _ = sender.send(write_png(image, &path));
        });
    });
    capture.done = Some(receiver);
}

fn capture_on_key(
    menu: Option<Res<crate::menu::MenuRuntime>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    channel: Res<ScreenshotChannel>,
    mut frames: ResMut<FrameCapture>,
) {
    if menu.as_ref().is_some_and(|menu| menu.is_visible())
        || !crate::menu::settings_options::binding_pressed(
            menu.as_deref(),
            "key.screenshot",
            &keys,
            &mouse,
        )
    {
        return;
    }
    let (path, file) = match unique_path(&channel.dir, SystemTime::now()) {
        Ok(reserved) => reserved,
        Err(error) => {
            let _ = channel.sender.send(Err(error.to_string()));
            return;
        }
    };
    let sender = channel.sender.clone();
    frames.request(move |image| {
        let image = image.clone();
        std::thread::spawn(move || {
            let _ = sender.send(write_reserved_png(image, &path, file));
        });
    });
}

fn report_saved(
    channel: Res<ScreenshotChannel>,
    mut runtime: ResMut<UiRuntime>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    for result in channel.receiver.try_iter() {
        let line = match result {
            Ok(name) => format!("Saved screenshot as {name}"),
            Err(error) => format!("Failed to save screenshot: {error}"),
        };
        runtime.push_local_chat_line(Arc::from(line), now_millis);
    }
}

/// Writes the capture as RGB so HDR alpha never reaches the file.
pub(crate) fn write_png(image: Image, path: &Path) -> SaveResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = File::create(path).map_err(|error| error.to_string())?;
    write_reserved_png(image, path, file)
}

/// Encodes RGB into the reserved file so concurrent captures cannot replace each other.
fn write_reserved_png(image: Image, path: &Path, file: File) -> SaveResult {
    let dynamic = image
        .try_into_dynamic()
        .map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    image::DynamicImage::ImageRgb8(dynamic.to_rgb8())
        .write_to(&mut writer, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    Ok(path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()))
}

/// Atomically reserves `yyyy-mm-dd_hh.mm.ss.png` in UTC, suffixed until unused.
fn unique_path(dir: &Path, now: SystemTime) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let stem = utc_stamp(seconds);
    for suffix in 0_u64.. {
        let name = if suffix == 0 {
            format!("{stem}.png")
        } else {
            format!("{stem}_{suffix}.png")
        };
        let path = dir.join(name);
        match File::options().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("screenshot suffixes exhausted")
}

fn utc_stamp(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let rem = epoch_seconds % 86_400;
    // Proleptic Gregorian civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}_{:02}.{:02}.{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn review_failed_automated_capture_exits_with_failure() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        sender.send(Err("cannot save".into())).unwrap();
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<FrameCapture>()
            .insert_resource(EnvCapture {
                path: PathBuf::new(),
                frames: 0,
                done: Some(receiver),
            })
            .add_systems(Update, capture_from_env);
        app.update();
        let exits = app
            .world_mut()
            .resource_mut::<Messages<AppExit>>()
            .drain()
            .collect::<Vec<_>>();
        assert_eq!(exits.len(), 1);
        assert!(matches!(exits[0], AppExit::Error(_)));
    }

    #[test]
    fn review_unwritten_screenshots_have_distinct_names() {
        let dir =
            std::env::temp_dir().join(format!("cinnabar-pending-shot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (first, _first) = unique_path(&dir, UNIX_EPOCH).unwrap();
        let (second, _second) = unique_path(&dir, UNIX_EPOCH).unwrap();
        assert_ne!(first, second);
        drop((_first, _second));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stamp_matches_known_epochs() {
        assert_eq!(utc_stamp(0), "1970-01-01_00.00.00");
        assert_eq!(utc_stamp(951_782_400 + 86_399), "2000-02-29_23.59.59");
        assert_eq!(utc_stamp(1_785_294_202), "2026-07-29_03.03.22");
    }

    #[test]
    fn colliding_names_gain_a_numeric_suffix() {
        let dir = std::env::temp_dir().join(format!("cinnabar-shot-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let (first, _file) = unique_path(&dir, now).unwrap();
        assert!(first.ends_with("2001-09-09_01.46.40.png"));
        fs::write(&first, b"x").unwrap();
        assert!(
            unique_path(&dir, now)
                .unwrap()
                .0
                .ends_with("2001-09-09_01.46.40_1.png")
        );
        drop(_file);
        fs::remove_dir_all(&dir).unwrap();
    }
}
