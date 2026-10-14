//! Drives distant terrain from the Video settings: the session's store and its identity,
//! drawing through the renderer, the fog and far clip it needs, the cache clear action and
//! the counters' log lines.

mod clear;
mod draw;

use std::{
    path::Path,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::{
    prelude::{App, IntoScheduleConfigs, Local, Res, ResMut, Resource, Update},
    time::{Real, Time},
};
use client_presentation::local_player::LocalViewPose;
use client_runtime::distant_terrain::{
    DistantTerrainFrame, DistantTerrainReport, DistantTerrainSessions, DistantTerrainTarget,
};
use lod_store::minute::Minute;

use super::{ClientWorld, drive_world_stream};
use crate::session::SessionController;
pub(crate) use draw::DistantTerrainFogDistance;

#[cfg(test)]
mod tests;

/// Shortest time between two counter lines of one kind.
const REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// The app's distant terrain adapter; the mutex is reached only through `get_mut`.
#[derive(Resource)]
pub(crate) struct DistantTerrainResource(Mutex<DistantTerrainSessions>);

/// Registers the adapter for stores under `lod_cache_dir`. It runs after the frame's
/// stream poll, while the stream is on the frame thread; drawing follows it.
pub(crate) fn configure_distant_terrain(app: &mut App, lod_cache_dir: &Path) {
    app.insert_resource(DistantTerrainResource(Mutex::new(
        DistantTerrainSessions::new(lod_cache_dir),
    )))
    .add_systems(
        Update,
        (
            drive_distant_terrain,
            draw::draw_distant_terrain,
            draw::apply_far_clip,
        )
            .chain()
            .after(drive_world_stream),
    );
    draw::configure(app);
    clear::configure(app, lod_cache_dir);
}

/// The last line logged of one kind and when.
struct Reported<T> {
    last: T,
    at: Option<Instant>,
}

impl<T: Default> Default for Reported<T> {
    fn default() -> Self {
        Self {
            last: T::default(),
            at: None,
        }
    }
}

/// The last ingestion report logged.
type ReportedCounters = Reported<DistantTerrainReport>;

/// Drives the session's store from the Video settings and logs its counters.
#[allow(clippy::too_many_arguments)]
fn drive_distant_terrain(
    mut sessions: ResMut<DistantTerrainResource>,
    mut client_world: ResMut<ClientWorld>,
    controller: Option<Res<SessionController>>,
    settings: Res<client_state::RuntimeSettings>,
    drawing: Res<draw::DistantTerrainDrawingResource>,
    view: Res<LocalViewPose>,
    real_time: Res<Time<Real>>,
    mut reported: Local<ReportedCounters>,
) {
    let video = settings.user_settings_update().1.video;
    let enabled = video.distant_terrain;
    let sessions = sessions.0.get_mut().unwrap_or_else(PoisonError::into_inner);
    sessions.set_drawing(enabled && !drawing.unsupported());
    sessions.set_store_cap(video.distant_terrain_storage_bytes);
    // Off with nothing open or closing: no clock read and no work beyond this branch.
    if !enabled && sessions.is_idle() {
        return;
    }
    let destination = controller
        .as_deref()
        .and_then(SessionController::destination_identity);
    let target = enabled
        .then(|| {
            let (generation, identity) = client_world.world_identity.as_ref()?;
            Some(DistantTerrainTarget::new(
                destination?,
                &identity.level_id,
                identity.seed,
                *generation,
            ))
        })
        .flatten();
    let eye = view.eye_translation();
    let player_block = eye
        .is_finite()
        .then(|| [eye.x.floor() as i64, eye.z.floor() as i64]);
    sessions.update(DistantTerrainFrame {
        target,
        stream: client_world.stream.as_mut(),
        now: current_minute(),
        uptime: real_time.elapsed(),
        player_block,
    });
    if let Some(line) = report_counters(sessions.report(), &mut reported, Instant::now) {
        eprintln!("{line}");
    }
}

/// The wall-clock minute; this edge is where distant terrain reads the clock.
fn current_minute() -> Minute {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    Minute::from_unix_seconds(seconds)
}

/// The line to log for `report`: only when it differs from the last one logged, and at
/// most once per [`REPORT_INTERVAL`]. An unchanged report costs one comparison; `clock`
/// is read only for a changed one.
fn report_counters(
    report: DistantTerrainReport,
    reported: &mut ReportedCounters,
    clock: impl FnOnce() -> Instant,
) -> Option<String> {
    report_changed(report, reported, clock, DistantTerrainReport::marker_line)
}

/// [`report_counters`] for any report kind, formatted by `line`.
fn report_changed<T: PartialEq + Copy>(
    report: T,
    reported: &mut Reported<T>,
    clock: impl FnOnce() -> Instant,
    line: impl FnOnce(&T) -> String,
) -> Option<String> {
    if report == reported.last {
        return None;
    }
    let now = clock();
    if reported
        .at
        .is_some_and(|at| now.duration_since(at) < REPORT_INTERVAL)
    {
        return None;
    }
    reported.last = report;
    reported.at = Some(now);
    Some(line(&report))
}
