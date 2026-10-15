//! Opens the crash report window with sample data, for reviewing its look and behaviour.
//!
//! `cargo run -p crash-monitor --example crash_preview`. The sample report folder is created under
//! the system temporary directory, so Open folder has something to show.

use std::{fs, path::PathBuf};

use bootstrap_ui::Preferences;
use crash_monitor::window::{CrashSummary, Headline};
use launcher::install_layout::InstallLayout;
use launcher_host::lifecycle::crash::attachments;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/CinnanglesSans.ttf");

const LOG_TAIL: &str = "\
21:06:41.018  INFO cinnabar::startup: Cinnabar 0.1.20 (4ec3296c5, release) on Windows 11 23H2
21:06:41.020  INFO cinnabar::startup: logs at %LOCALAPPDATA%\\Cinnabar\\logs
21:06:41.391  INFO renderer_startup: adapter NVIDIA GeForce RTX 3070 (driver 566.36), backend Vulkan
21:06:41.392  INFO renderer_startup: present mode Mailbox, 2560x1440 @ 165 Hz, HDR off
21:06:41.877  INFO assets::carriers: 41 carriers verified in 212 ms
21:06:42.104  INFO client_ui::menus: title screen ready in 1.09 s
21:07:02.660  INFO launcher_host::servers: selected server 'Survival (example)'
21:07:02.661  INFO client_session::join: resolving play.example.net:19132
21:07:02.702  INFO client_session::join: resolved to 203.0.113.24:19132 in 41 ms
21:07:02.703  INFO bedrock_core::raknet: open connection request (MTU 1400)
21:07:02.781  INFO bedrock_core::raknet: connected, MTU 1400, RTT 38 ms
21:07:02.782  INFO bedrock_core::login: network settings: compression zlib, threshold 256
21:07:02.904  INFO bedrock_core::login: Xbox Live chain verified
21:07:02.948  INFO bedrock_core::login: encryption enabled (AES-256-GCM)
21:07:02.990  INFO client_session::join: play status: login success
21:07:02.991  INFO client_session::packs: server offers 3 resource packs (148.2 MiB), 0 behaviour packs
21:07:02.991  INFO client_session::packs: 2 packs cached, downloading 1 (61.7 MiB)
21:07:09.336  INFO client_session::packs: downloaded 'Example Survival Textures' in 6.3 s (9.7 MiB/s)
21:07:09.337  INFO pack_compiler: compiling 3 packs over the built-in Java-look HUD
21:07:11.802  WARN pack_compiler::json_ui: hud_screen.json: unknown control type 'custom_bossbar', skipped
21:07:11.803  WARN pack_compiler::json_ui: 2 bindings reference missing view model 'chat_tab'
21:07:12.640  INFO pack_compiler: compiled 3 packs in 3.30 s (14 812 textures, 2 atlases)
21:07:12.641  INFO client_session::join: resource pack stack applied
21:07:12.702  INFO client_session::join: start game: dimension overworld, spawn (1204, 71, -388)
21:07:12.703  INFO client_session::join: world template 'example_survival', 384 block palette entries
21:07:12.705  INFO client_world: item registry: 1 312 items, 18 custom
21:07:12.708  INFO client_world: block registry: 14 127 states, 96 custom
21:07:12.731  INFO chunk_pipeline: view distance 12 chunks, 4 mesh workers
21:07:12.733  INFO client_session::join: requesting chunk radius 12, server granted 10
21:07:12.890  INFO chunk_pipeline::stream: first sub-chunk batch received (27 sub-chunks)
21:07:13.044  INFO chunk_pipeline::stream: 84 chunks queued, 12 meshed
21:07:13.212  WARN client_session::entities: entity 1188 add_actor with unknown identifier 'example:boat_v2'
21:07:13.215  INFO particles: 6 emitters registered from resource packs
21:07:13.401  INFO chunk_pipeline::stream: 212 chunks queued, 96 meshed, 41 uploaded
21:07:13.522  INFO client_audio: 1 280 sound events, 2 music tracks loaded
21:07:13.639  INFO chunk_pipeline::stream: 318 chunks queued, 188 meshed, 140 uploaded
21:07:13.640  INFO far_terrain: horizon tiles ready (radius 24)
21:07:13.788  WARN chunk_pipeline::decode: chunk (78, -25): sub-chunk version 9 with 2 storages, decoding leniently
21:07:13.802  INFO chunk_pipeline::stream: 364 chunks queued, 280 meshed, 251 uploaded
21:07:13.901  INFO render::frame: frame 14 412 at 6.8 ms (p99 11.2 ms)
21:07:14.010  INFO client_session::join: set local player as initialised
21:07:14.011  INFO client_session::join: play status: player spawn
21:07:14.012  INFO client_session::join: joined in 11.35 s
21:07:14.207  INFO client_world::biomes: 82 biome definitions, 3 custom
21:07:14.311  WARN render::atlas: atlas 1 at 98% of 16384x16384; 6 textures downsampled
21:07:14.415  INFO chunk_pipeline::stream: 401 chunks queued, 377 meshed, 377 uploaded
21:07:14.518  INFO render::frame: frame 14 466 at 7.1 ms (p99 12.9 ms)
21:07:14.620  INFO chunk_pipeline::stream: light update batch for 64 sub-chunks
21:07:14.744  ERROR chunk_pipeline::world: thread 'world-2' faulted while applying a light update
21:07:14.745  ERROR crash: EXCEPTION_ACCESS_VIOLATION reading 0x18 at cinnabar.exe+0x1a2b3c
21:07:14.745  ERROR crash: requesting a minidump from the crash monitor
21:07:14.902  INFO crash_monitor: minidump written (38.4 MiB), report saved";

/// Rows of a report's `packets.txt`, which the window lists through the same formatter.
const PACKETS_TABLE: &str = "\
     18402       11912  clientbound  accepted       4216  LevelChunk (58)
     18403       11913  clientbound  accepted      21904  SubChunk (174)
     18404       11940  clientbound  accepted         12  UpdateBlock (21)
     18405       11951  serverbound  accepted         64  PlayerAuthInput (144)
     18406       11988  clientbound  accepted         18  MoveActorDelta (111)
     18407       12003  clientbound  accepted       5102  LevelChunk (58)
     18408       12004  clientbound  skipped       33870  SubChunk (174)
     18409       12020  clientbound  accepted          6  SetTime (10)
     18410       12031  serverbound  accepted         64  PlayerAuthInput (144)
     18411       12044  clientbound  failed         1712  UpdateSubChunkBlocks (172)";

#[expect(
    clippy::print_stdout,
    reason = "the preview reports the chosen action to the developer who launched it"
)]
fn main() {
    let folder = sample_folder();
    let summary = CrashSummary {
        headline: Headline::Crashed {
            activity: "joining a server".into(),
        },
        fault: "EXCEPTION_ACCESS_VIOLATION reading 0x18".into(),
        location: "cinnabar.exe+0x1a2b3c on thread world-2".into(),
        version: "0.1.20 (4ec3296c5, release)".into(),
        graphics: "NVIDIA GeForce RTX 3070, driver 566.36, Vulkan".into(),
        session: "32 min, joining a server".into(),
        log_tail: LOG_TAIL.lines().map(str::to_owned).collect(),
        last_packets: attachments::window_lines(PACKETS_TABLE, attachments::WINDOW_PACKETS),
        folder,
    };
    let preferences = InstallLayout::discover()
        .map(|layout| Preferences::load(&layout))
        .unwrap_or_default();
    let choice = crash_monitor::window::show_with_font(summary, FONT, preferences);
    println!("crash preview closed: {choice:?}");
}

/// A throwaway report folder holding placeholder files, so Open folder has something to show.
fn sample_folder() -> PathBuf {
    let folder = std::env::temp_dir()
        .join("cinnabar-crash-preview")
        .join("crash-2026-10-14T21-07-14Z");
    fs::create_dir_all(&folder).expect("create the sample report folder");
    let files: [(&str, &[u8]); 3] = [
        (
            "report.json",
            br#"{"kind":"native","sample":true,"note":"crash window preview placeholder"}"#,
        ),
        ("client.log", LOG_TAIL.as_bytes()),
        (
            "crash.dmp",
            b"MDMP placeholder for the crash window preview\n",
        ),
    ];
    for (name, contents) in files {
        fs::write(folder.join(name), contents).expect("write a sample report file");
    }
    folder
}
