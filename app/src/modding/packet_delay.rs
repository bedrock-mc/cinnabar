//! Coalesced private core control, off the frame thread and independent of UI focus.
use super::ModRuntime;
use crate::runtime::network::NetworkHandle;
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use crossbeam_channel::{Sender, bounded};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const HEARTBEAT: Duration = Duration::from_secs(1);
const POSITION_POLL: Duration = Duration::from_millis(50);
const MAX_POSITION_AGE: Duration = Duration::from_millis(250);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const SHOW_BIT: u64 = 1 << 32;

/// Feet position last successfully relayed upstream, not a server acknowledgement.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct RealPositionSnapshot {
    pub position: Option<[f32; 3]>,
    pub session_id: u64,
}
#[derive(Clone, Copy)]
struct Capture {
    request: u64,
    received: Instant,
    snapshot: RealPositionSnapshot,
}
#[derive(Default)]
struct Shared {
    // A single atomic config and generation reject late RPC responses.
    request: AtomicU64,
    stop: AtomicBool,
    capture: Mutex<Option<Capture>>,
}
impl Shared {
    fn publish(&self, delay: u32, show: bool) -> bool {
        let config = u64::from(delay) | if show && delay != 0 { SHOW_BIT } else { 0 };
        self.request
            .try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                if old & ((1 << 33) - 1) == config {
                    None
                } else {
                    Some((old.wrapping_add(1 << 33) & !((1 << 33) - 1)) | config)
                }
            })
            .is_ok()
    }
    fn accept(&self, request: u64, lease: bridge::PacketDelayLease, received: Instant) {
        if self.request.load(Ordering::Acquire) != request || self.stop.load(Ordering::Acquire) {
            return;
        }
        *self
            .capture
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Capture {
            request,
            received,
            snapshot: witness(request, &lease),
        });
    }
    fn snapshot(&self, now: Instant) -> RealPositionSnapshot {
        let request = self.request.load(Ordering::Acquire);
        if request & SHOW_BIT == 0 || self.stop.load(Ordering::Acquire) {
            return RealPositionSnapshot::default();
        }
        self.capture
            .try_lock()
            .ok()
            .and_then(|capture| *capture)
            .filter(|capture| {
                capture.request == request
                    && now.saturating_duration_since(capture.received) <= MAX_POSITION_AGE
            })
            .map_or_else(RealPositionSnapshot::default, |capture| capture.snapshot)
    }
}
fn witness(request: u64, lease: &bridge::PacketDelayLease) -> RealPositionSnapshot {
    if request & SHOW_BIT == 0 || lease.delay_ms == 0 || lease.session_id == 0 {
        return RealPositionSnapshot::default();
    }
    let position = lease.position.and_then(|point| {
        let feet = [point.x, point.y - protocol::PLAYER_NETWORK_OFFSET, point.z];
        feet.iter().all(|value| value.is_finite()).then_some(feet)
    });
    RealPositionSnapshot {
        position,
        session_id: lease.session_id,
    }
}
pub(super) struct Worker {
    endpoint: PathBuf,
    app_session: u64,
    shared: Arc<Shared>,
    wake: Sender<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.publish(0, false);
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}
impl Worker {
    fn start(endpoint: PathBuf, app_session: u64) -> Option<Self> {
        let shared = Arc::new(Shared::default());
        let (wake, receiver) = bounded(1);
        let state = Arc::clone(&shared);
        let socket_dir = endpoint.clone();
        std::thread::Builder::new()
            .name("mod-packet-delay".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                let mut last = None;
                loop {
                    let stop = state.stop.load(Ordering::Acquire);
                    let request = state.request.load(Ordering::Acquire);
                    let delay = if stop { 0 } else { request as u32 };
                    let show = !stop && request & SHOW_BIT != 0;
                    if stop || last != Some(request) || delay != 0 {
                        let result = runtime.block_on(async {
                            tokio::time::timeout(
                                REQUEST_TIMEOUT,
                                bridge::packet_delay_with_position(&socket_dir, delay, show),
                            )
                            .await
                        });
                        match result {
                            Ok(Ok(lease)) if lease.delay_ms == delay && lease.lease_ms > 0 => {
                                state.accept(request, lease, Instant::now());
                                last = Some(request);
                            }
                            _ => {
                                *state
                                    .capture
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner()) = None;
                                last = None;
                            }
                        }
                    }
                    if stop {
                        return;
                    }
                    let _ = receiver.recv_timeout(if show { POSITION_POLL } else { HEARTBEAT });
                }
            })
            .ok()?;
        Some(Self {
            endpoint,
            app_session,
            shared,
            wake,
        })
    }
    fn publish(&self, delay: u32, show: bool) {
        if self.shared.publish(delay, show) {
            let _ = self.wake.try_send(());
        }
    }
}
/// The first active, granted nonzero-delay owner also owns its position witness opt-in.
fn requested(extension: Option<&ModRuntime>) -> (u32, bool) {
    let Some(runtime) = extension.filter(|runtime| !runtime.suspended) else {
        return (0, false);
    };
    (0..runtime.host_count())
        .map(|index| runtime.host(index))
        .filter(|host| host.is_active() && host.grants().packet_delay)
        .find(|host| host.packet_delay_ms() != 0)
        .map_or((0, false), |host| {
            (host.packet_delay_ms(), host.show_real_position())
        })
}
pub(super) fn publish_packet_delay(
    extension: Option<Res<ModRuntime>>,
    network: Option<Res<NetworkHandle>>,
    ui: Option<Res<UiRuntime>>,
    mut snapshot: ResMut<RealPositionSnapshot>,
    mut worker: Local<Option<Worker>>,
) {
    let endpoint = network.as_deref().and_then(NetworkHandle::core_socket_dir);
    let app_session = ui.as_deref().map_or(0, UiRuntime::session_id);
    if worker.as_ref().is_some_and(|worker| {
        Some(worker.endpoint.as_path()) != endpoint || worker.app_session != app_session
    }) {
        *worker = None;
    }
    let (delay, show) = requested(extension.as_deref());
    if worker.is_none()
        && delay != 0
        && let Some(endpoint) = endpoint
    {
        *worker = Worker::start(endpoint.to_owned(), app_session);
    }
    *snapshot = RealPositionSnapshot::default();
    if let Some(worker) = worker.as_ref() {
        worker.publish(delay, show);
        *snapshot = worker.shared.snapshot(Instant::now());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bridge::{PacketDelayLease, RelayedPosition};
    use mod_host::ModGrants;
    /// A component whose `init` requests `delay` milliseconds of packet delay.
    fn delaying(directory: &std::path::Path, index: usize, delay: u32) -> PathBuf {
        let package = include_str!("../../../crates/mod-api/wit/extension.wit")
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("package ")
            .trim_end_matches(';');
        let (name, version) = package.split_once('@').unwrap();
        let source = format!(
            r#"(component
  (import "{name}/gameplay@{version}" (instance $gameplay
    (export "set-packet-delay" (func (param "delay-ms" u32) (result (result (error string)))))))
  (alias export $gameplay "set-packet-delay" (func $packet-delay))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-delay (canon lower (func $packet-delay) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "delay" (func $delay (param i32 i32)))
    (func (export "init") i32.const {delay} i32.const 256 call $delay)
    (func (export "frame")))
  (core instance $host (export "delay" (func $lower-delay)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))"#
        );
        let path = directory.join(format!("delay-{index}.wat"));
        std::fs::write(&path, source).unwrap();
        path
    }

    #[test]
    fn the_earliest_granted_non_zero_packet_delay_wins_across_loaded_mods() {
        let directory = tempfile::tempdir().unwrap();
        let granted = ModGrants {
            packet_delay: true,
            ..Default::default()
        };
        let mods = [(0, true), (300, false), (200, true), (500, true)]
            .into_iter()
            .enumerate()
            .map(|(index, (delay, grant))| {
                let grants = if grant {
                    granted.clone()
                } else {
                    ModGrants::default()
                };
                (delaying(directory.path(), index, delay), grants)
            })
            .collect();
        let mut app = App::new();
        super::super::configure_set(&mut app, mods);
        let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
        assert_eq!(runtime.host_count(), 4);
        assert_eq!(requested(Some(&runtime)), (200, false));
        runtime.suspended = true;
        assert_eq!(requested(Some(&runtime)), (0, false));
    }

    #[test]
    fn multiple_mods_keep_the_visual_flag_with_the_selected_delay_owner() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        let package = include_str!("../../../crates/mod-api/wit/extension.wit")
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("package ")
            .trim_end_matches(';');
        let (name, version) = package.split_once('@').unwrap();
        let gameplay_interface = format!("{name}/gameplay@{version}");
        let entries = [(0, true, false), (200, false, true), (400, true, false)]
            .into_iter()
            .enumerate()
            .map(|(index, (delay, show, trap))| {
                let path = directory.path().join(format!("delay-{index}.wat"));
                let init = format!(
                    "i32.const {delay} i32.const 256 call $delay i32.const {} i32.const 256 call $show",
                    u8::from(show)
                );
                let source = include_str!("../../../crates/mod-host/src/tests/gameplay.wat")
                    .replace("$GAMEPLAY", &gameplay_interface)
                    .replace("$INIT", &init)
                    .replace("$FRAME", if trap { "unreachable" } else { "" });
                std::fs::write(&path, source).unwrap();
                (path, mod_host::ModGrants { packet_delay: true, ..Default::default() })
            })
            .collect();
        super::super::configure_set(&mut app, entries);
        let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
        assert_eq!(requested(Some(&runtime)), (200, false));
        assert!(runtime.host_mut(1).frame(false).is_err());
        assert_eq!(requested(Some(&runtime)), (400, true));
        runtime.suspended = true;
        assert_eq!(requested(Some(&runtime)), (0, false));
    }
    #[test]
    fn absent_offline_ui_and_network_clear_the_published_witness() {
        let mut app = App::new();
        app.insert_resource(RealPositionSnapshot {
            position: Some([1.0, 2.0, 3.0]),
            session_id: 1,
        })
        .add_systems(Update, publish_packet_delay);
        app.update();
        assert_eq!(
            *app.world().resource::<RealPositionSnapshot>(),
            RealPositionSnapshot::default()
        );
    }
    fn lease(session_id: u64) -> PacketDelayLease {
        PacketDelayLease {
            delay_ms: 200,
            lease_ms: 3000,
            session_id,
            position: Some(RelayedPosition {
                x: 3.0,
                y: 70.0 + protocol::PLAYER_NETWORK_OFFSET,
                z: 5.0,
            }),
        }
    }
    #[test]
    fn coalesced_disable_cannot_be_lost_behind_a_full_wake_queue() {
        let (wake, receiver) = bounded(1);
        let worker = Worker {
            endpoint: "fixture".into(),
            app_session: 1,
            shared: Arc::new(Shared::default()),
            wake,
        };
        worker.publish(200, true);
        worker.publish(400, true);
        worker.publish(0, false);
        assert_eq!(receiver.len(), 1);
        assert_eq!(worker.shared.request.load(Ordering::Acquire) as u32, 0);
        let shared = Arc::clone(&worker.shared);
        drop(worker);
        assert!(shared.stop.load(Ordering::Acquire));
    }
    #[test]
    fn position_witness_converts_to_feet_and_clears_off_stale_or_superseded_replies() {
        let shared = Shared::default();
        let now = Instant::now();
        shared.publish(200, true);
        let first = shared.request.load(Ordering::Acquire);
        shared.accept(first, lease(1), now);
        assert_eq!(shared.snapshot(now).position, Some([3.0, 70.0, 5.0]));
        assert_eq!(
            shared.snapshot(now + MAX_POSITION_AGE + Duration::from_nanos(1)),
            RealPositionSnapshot::default()
        );
        shared.publish(200, false);
        assert_eq!(shared.snapshot(now), RealPositionSnapshot::default());
        shared.publish(200, true);
        shared.accept(first, lease(1), now);
        assert_eq!(shared.snapshot(now), RealPositionSnapshot::default());
        let current = shared.request.load(Ordering::Acquire);
        shared.accept(current, lease(2), now);
        assert_eq!(shared.snapshot(now).session_id, 2);
        shared.publish(0, true);
        assert_eq!(shared.snapshot(now), RealPositionSnapshot::default());
    }
    #[test]
    fn position_witness_rejects_nonfinite_and_missing_session_samples() {
        assert_eq!(
            witness(SHOW_BIT | 200, &lease(0)),
            RealPositionSnapshot::default()
        );
        let mut invalid = lease(1);
        invalid.position.as_mut().unwrap().x = f32::NAN;
        assert!(witness(SHOW_BIT | 200, &invalid).position.is_none());
    }
}
