//! Synchronous native packet-delay policy, independent of UI focus.
use super::ModRuntime;
use crate::runtime::network::resource_packs::PackApplication;
use bevy::prelude::*;
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiRuntimeResource;
#[cfg(test)]
use std::path::PathBuf;

/// Feet position last successfully relayed upstream, not a server acknowledgement.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct RealPositionSnapshot {
    pub position: Option<[f32; 3]>,
    pub session_id: u64,
}
/// The first active, granted nonzero-delay owner also owns its position witness opt-in.
fn requested(extension: Option<&ModRuntime>) -> (u32, bool) {
    let Some(runtime) = extension.filter(|runtime| !runtime.suspended) else {
        return (0, false);
    };
    (0..runtime.hosts.host_count())
        .map(|index| runtime.hosts.host(index))
        .filter(|host| host.is_active() && host.grants().packet_delay)
        .find(|host| host.packet_delay_ms() != 0)
        .map_or((0, false), |host| {
            (host.packet_delay_ms(), host.show_real_position())
        })
}
pub(super) fn publish_packet_delay(
    extension: Option<Res<ModRuntime>>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    ui: Option<Res<UiRuntimeResource>>,
    mut snapshot: ResMut<RealPositionSnapshot>,
) {
    let app_session = ui.as_deref().map_or(0, |ui| ui.session_id());
    let (delay, show) = requested(extension.as_deref());
    *snapshot = RealPositionSnapshot::default();
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    {
        if let Some(network) = network.as_deref()
            && network
                .configure_packet_delay(app_session, delay, show)
                .is_ok()
            && delay != 0
            && show
        {
            snapshot.position = network
                .packet_delay_position(app_session)
                .and_then(|point| {
                    let feet = [
                        point[0],
                        point[1] - protocol::PLAYER_NETWORK_OFFSET,
                        point[2],
                    ];
                    feet.iter().all(|value| value.is_finite()).then_some(feet)
                });
            snapshot.session_id = app_session;
        }
    }
    #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
    let _ = (network, app_session, delay, show);
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(runtime.hosts.host_count(), 4);
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
        assert!(runtime.hosts.host_mut(1).frame(false).is_err());
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
}
