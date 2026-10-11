//! Local-only timings of launcher frames through the real UI carrier: the frame
//! an input produces and an idle frame. Skips without the gitignored carrier;
//! `CINNABAR_MENU_LATENCY=1` prints the table.

use std::time::{Duration, Instant};

use ui::DpiScale;

use super::super::UiPresentationRuntime;
use super::pack_harness::engine_presentation;
use super::play_flow_snapshots::fixture_view;
use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuAction, MenuScreen, MenuView};

/// A Retina window, as the owner plays.
const SIZE: [u32; 2] = [2560, 1440];
const DPI: f32 = 2.0;

struct Bench {
    presentation: UiPresentationRuntime,
    runtime: UiRuntime,
    player_runtime: player_state::PlayerState,
    view: MenuView,
    now_millis: u64,
}

impl Bench {
    fn new() -> Option<Self> {
        let presentation = engine_presentation()?;
        let dir = std::env::temp_dir().join("cinnabar-play-flow-art");
        std::fs::create_dir_all(&dir).ok()?;
        let mut view = fixture_view(&dir);
        view.screen = MenuScreen::Home;
        let mut runtime = UiRuntime::new(1);
        let lang = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("../.local/assets/compiled/vanilla-v1.mcbelang");
        if let Some(lang) = std::fs::read(lang)
            .ok()
            .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
        {
            runtime.set_lang_catalog(std::sync::Arc::new(lang));
        }
        Some(Self {
            presentation,
            runtime,
            player_runtime: player_state::PlayerState::new(1),
            view,
            now_millis: 0,
        })
    }

    /// One published frame of the current view, as `publish_ui_runtime` makes it.
    fn frame(&mut self) -> Duration {
        let started = Instant::now();
        self.now_millis += 16;
        let paths = super::super::menu_artwork::view_paths(&self.view);
        self.presentation.sync_menu_artwork(paths);
        self.presentation.set_menu_view(Some(self.view.clone()));
        self.presentation
            .build(
                &self.player_runtime,
                &self.runtime,
                self.now_millis,
                SIZE,
                DpiScale::new(DPI).unwrap(),
            )
            .unwrap();
        started.elapsed()
    }

    /// The action whose hit target is under a hover of `wanted`.
    fn find(&self, wanted: impl Fn(&MenuAction) -> bool) -> Option<MenuAction> {
        self.hits().into_iter().find(|action| wanted(action))
    }

    /// The frame right after `change`, once the current view is warm.
    fn after(&mut self, change: impl FnOnce(&mut MenuView)) -> Duration {
        change(&mut self.view);
        self.frame()
    }

    fn idle(&mut self, frames: u32) -> Duration {
        self.frame();
        (0..frames).map(|_| self.frame()).sum::<Duration>() / frames
    }

    fn hits(&self) -> Vec<MenuAction> {
        let mut actions: Vec<_> = self
            .presentation
            .menu_hit_targets
            .iter()
            .map(|(action, _)| *action)
            .collect();
        actions.dedup();
        actions
    }
}

/// `view` carrying a heavy account: hundreds of saved servers, friends and messages.
fn grow(view: &mut MenuView) {
    let saved = view.servers[0].clone();
    view.servers = (0..300)
        .map(|index| launcher::menu::SavedServer {
            name: format!("Server {index}"),
            address: format!("10.0.{}.{}:19132", index / 250, index % 250),
            ..saved.clone()
        })
        .collect();
    let friend = view.friends[0].clone();
    view.friends = (0..150)
        .map(|index| launcher::menu::MenuFriendCard {
            gamertag: format!("Friend{index}"),
            ..friend.clone()
        })
        .collect();
    view.feeds.home.inbox = (0..200)
        .map(|index| launcher::menu::InboxItem {
            header: format!("Message {index}"),
            body: "A long message body that wraps across several lines of the row.".to_owned(),
            category: ["news", "realms", "store"][index % 3].to_owned(),
            unread: index % 2 == 0,
            ..Default::default()
        })
        .collect();
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn report(rows: &[(&str, Duration)]) {
    if std::env::var_os("CINNABAR_MENU_LATENCY").is_none() {
        return;
    }
    for (name, duration) in rows {
        eprintln!(
            "menu-latency {name:<28} {:>9.2} ms",
            duration.as_secs_f64() * 1e3
        );
    }
}

fn ms(duration: &Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1e3)
}

// From a fresh launch, pressing Play costs no frame far above an idle one.
#[test]
fn home_to_play_from_a_fresh_launch() {
    let Some(mut bench) = Bench::new() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    let home: Vec<_> = (0..8).map(|_| bench.frame()).collect();
    let play = bench
        .find(|action| *action == MenuAction::Navigate(MenuScreen::Play))
        .expect("the start screen offers Play");
    let hover: Vec<_> = (0..3)
        .map(|_| bench.after(|view| view.hovered = Some(play)))
        .collect();
    let press = bench.after(|view| {
        view.pressed = Some(play);
        view.screen = MenuScreen::Play;
        view.hovered = None;
    });
    bench.view.pressed = None;
    let after: Vec<_> = (0..5).map(|_| bench.frame()).collect();
    // Settings after the start screen idled while drawing frames.
    bench.after(to(MenuScreen::Home));
    let idled = Instant::now();
    while idled.elapsed() < Duration::from_secs(3) {
        bench.frame();
        std::thread::sleep(Duration::from_millis(16));
    }
    let settings = bench.after(to(MenuScreen::Settings));
    if std::env::var_os("CINNABAR_MENU_LATENCY").is_some() {
        let list = |frames: &[Duration]| frames.iter().map(ms).collect::<Vec<_>>().join(" ");
        eprintln!("menu-latency fresh home frames   {}", list(&home));
        eprintln!("menu-latency hover play button   {}", list(&hover));
        eprintln!("menu-latency press (first play)  {}", ms(&press));
        eprintln!("menu-latency play frames         {}", list(&after));
        eprintln!("menu-latency settings (after idle) {}", ms(&settings));
    }
    let idle = *home.last().unwrap();
    let worst = hover.iter().chain(&after).chain([&press]).max().unwrap();
    assert!(
        *worst < idle * 4 + Duration::from_millis(16),
        "{worst:?} vs {idle:?}"
    );
    assert!(
        settings < Duration::from_millis(16),
        "settings after home idle: {settings:?}"
    );
}

/// `view` drawing downloaded service art from `CINNABAR_MENU_ART_DIR`, a copy
/// of a launcher core's artwork cache; `None` without one.
fn real_art(view: &mut MenuView) -> Option<()> {
    let dir = std::env::var_os("CINNABAR_MENU_ART_DIR")?;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path().to_string_lossy().into_owned())
        .collect();
    files.sort();
    let mut next = files.iter().cycle().cloned();
    let card = view.featured[0].clone();
    view.featured = (0..14)
        .map(|index| launcher::menu::MenuServerCard {
            name: format!("Server {index}"),
            address: format!("server{index}.example.net:19132"),
            image_path: next.next().unwrap(),
            ..card.clone()
        })
        .collect();
    let details = view.feeds.details.values().next()?.clone();
    for server in &view.featured {
        let mut details = details.clone();
        details.screenshots = (0..3).map(|_| next.next().unwrap()).collect();
        for game in &mut details.games {
            game.image_path = next.next().unwrap();
        }
        view.feeds.details.insert(server.address.clone(), details);
    }
    view.feeds.home.persona_head = next.next().unwrap();
    if let Some(event) = view.feeds.home.live_event.as_mut() {
        event.badge_path = next.next().unwrap();
    }
    Some(())
}

// With real downloaded art, no Home, Play or Servers frame waits on decoding it.
#[test]
fn transitions_with_downloaded_art() {
    let Some(mut bench) = Bench::new() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    if real_art(&mut bench.view).is_none() {
        eprintln!("skipping: CINNABAR_MENU_ART_DIR unset");
        return;
    }
    let first = bench.frame();
    let home: Vec<_> = (0..5).map(|_| bench.frame()).collect();
    let play = bench.after(to(MenuScreen::Play));
    let play_idle: Vec<_> = (0..3).map(|_| bench.frame()).collect();
    let servers = bench.after(to(MenuScreen::Servers));
    let select: Vec<_> = (1..5)
        .map(|index| bench.after(|view| view.feeds.select(index)))
        .collect();
    // Frames until the selected server's banner is installed, as the worker finishes.
    let banner = bench.view.feeds.details[&bench.view.featured[4].address].screenshots[0].clone();
    let started = Instant::now();
    let mut settle = Vec::new();
    while bench.presentation.menu_artwork_icon(&banner).is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "art never installed"
        );
        settle.push(bench.frame());
    }
    let list = |frames: &[Duration]| frames.iter().map(ms).collect::<Vec<_>>().join(" ");
    if std::env::var_os("CINNABAR_MENU_LATENCY").is_some() {
        eprintln!("menu-latency art first frame      {}", ms(&first));
        eprintln!("menu-latency art home frames      {}", list(&home));
        eprintln!("menu-latency art home to play     {}", ms(&play));
        eprintln!("menu-latency art play frames      {}", list(&play_idle));
        eprintln!("menu-latency art play to servers  {}", ms(&servers));
        eprintln!("menu-latency art select featured  {}", list(&select));
        let worst = settle.iter().max().map_or_else(String::new, ms);
        eprintln!(
            "menu-latency art until installed  {} frames, max {worst}",
            settle.len()
        );
    }
    let worst = [play, servers]
        .iter()
        .chain(&play_idle)
        .chain(&select)
        .chain(&settle)
        .max()
        .copied()
        .unwrap();
    assert!(worst < Duration::from_millis(16), "{worst:?}");
}

fn to(screen: MenuScreen) -> impl FnOnce(&mut MenuView) {
    move |view| view.screen = screen
}

/// Measures the owner's five menu states through the real carrier, including edit boxes.
#[test]
fn idle_screen_costs() {
    let Some(mut bench) = Bench::new() else {
        return;
    };
    let mut rows = Vec::new();
    for (name, screen) in [
        ("home", MenuScreen::Home),
        ("inbox", MenuScreen::Inbox),
        ("play", MenuScreen::Play),
        ("servers", MenuScreen::Servers),
        ("edit server", MenuScreen::AddServer),
        ("settings", MenuScreen::Settings),
    ] {
        bench.view.screen = screen;
        bench.view.editing = (screen == MenuScreen::AddServer).then_some(0);
        bench.frame();
        // Discard preparation and texture arrival frames before sampling steady state.
        for _ in 0..10 {
            bench.frame();
        }
        rows.push((name, median((0..31).map(|_| bench.frame()).collect())));
    }
    report(&rows);
}

// A hover repaints over the laid-out screen, so a sweep costs no more than idling.
#[test]
fn menu_input_frames_cost_about_an_idle_frame() {
    let Some(mut bench) = Bench::new() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    // Opened on the frame after the first start screen frame.
    let early_settings = Bench::new().map_or(Duration::ZERO, |mut early| {
        early.frame();
        early.after(to(MenuScreen::Settings))
    });
    let cold_settings = bench.after(to(MenuScreen::Settings));
    let idle_settings = bench.idle(10);
    let hovers: Vec<_> = bench
        .hits()
        .into_iter()
        .filter(|action| matches!(action, MenuAction::SettingsSection(_)))
        .collect();
    let sweep = median(
        hovers
            .iter()
            .map(|action| bench.after(|view| view.hovered = Some(*action)))
            .collect(),
    );
    bench.view.hovered = None;
    bench.after(to(MenuScreen::Home));
    let idle_home = bench.idle(10);
    let home_to_play = median(
        (0..5)
            .map(|_| {
                bench.after(to(MenuScreen::Home));
                bench.after(to(MenuScreen::Play))
            })
            .collect(),
    );
    let idle_play = bench.idle(10);
    let tabs = median(
        [MenuScreen::Social, MenuScreen::Servers, MenuScreen::Play]
            .into_iter()
            .cycle()
            .take(9)
            .map(|screen| bench.after(to(screen)))
            .collect(),
    );
    let settings_open = median(
        (0..5)
            .map(|_| {
                bench.after(to(MenuScreen::Home));
                bench.after(to(MenuScreen::Settings))
            })
            .collect(),
    );
    grow(&mut bench.view);
    let clone = median(
        (0..9)
            .map(|_| {
                let started = Instant::now();
                std::hint::black_box(bench.view.clone());
                started.elapsed()
            })
            .collect(),
    );
    bench.after(to(MenuScreen::Friends));
    let idle_friends = bench.idle(10);
    bench.after(to(MenuScreen::Inbox));
    let idle_inbox = bench.idle(10);
    bench.view.server_tab = launcher::menu::MenuServerTab::Saved;
    bench.after(to(MenuScreen::Servers));
    let idle_saved = bench.idle(10);
    report(&[
        ("settings open (cold)", cold_settings),
        ("settings open (1 home frame)", early_settings),
        ("settings open (warm)", settings_open),
        ("settings hover sweep", sweep),
        ("home to play", home_to_play),
        ("play tab switch", tabs),
        ("idle home", idle_home),
        ("idle play", idle_play),
        ("idle settings", idle_settings),
        ("heavy view clone", clone),
        ("idle friends (150)", idle_friends),
        ("idle inbox (200)", idle_inbox),
        ("idle saved servers (300)", idle_saved),
    ]);
    assert!(!hovers.is_empty(), "settings exposes its section selector");
    assert!(
        sweep < idle_settings * 3 + Duration::from_millis(5),
        "hover {sweep:?} vs idle {idle_settings:?}"
    );
}

/// Settings publishes navigation and edits on its first frame after either cold entry or Home.
#[test]
fn settings_first_frame_publishes_navigation_and_edit_controls() {
    for home_frames in 0..=1 {
        let Some(mut bench) = Bench::new() else {
            eprintln!(
                "skipping settings_first_frame_publishes_navigation_and_edit_controls: missing installed UI carrier (make assets)"
            );
            return;
        };
        for _ in 0..home_frames {
            bench.frame();
        }
        bench.view.screen = MenuScreen::Settings;
        bench.view.settings_section = super::menu_screens::SETTINGS_SECTIONS
            .iter()
            .find_map(|(name, index)| (*name == "accessibility_forced_index").then_some(*index))
            .expect("registered accessibility section");
        bench.frame();
        let actions = bench.hits();
        assert!(actions.contains(&MenuAction::AddBack));
        assert!(actions.contains(&MenuAction::SettingsSection(bench.view.settings_section)));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, MenuAction::SettingsOption(..))),
            "first frame must allow editing a setting: {actions:?}"
        );
    }
}

#[path = "menu_work_probe.rs"]
mod work_probe;
