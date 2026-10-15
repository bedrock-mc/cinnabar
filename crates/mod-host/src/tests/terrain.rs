use super::*;
use crate::{
    TerrainCheck, TerrainQuery, TerrainRefresh, TerrainRegion, TerrainScheduler, TerrainSource,
    TerrainStamp,
};
use std::path::Path;
#[path = "terrain_guest.rs"]
mod terrain_guest;
use terrain_guest::{CHANGED, PENDING, UNCHANGED, expect_error, expect_reads, terrain_guest};

const MIN: [i32; 3] = [0, 63, 0];
const MAX: [i32; 3] = [2, 65, 2];
/// `geometry` flag bits: collision only.
const COLLISION: u8 = 1;

fn load(check: &str, terrain: bool) -> ModHost {
    ModHost::load_snapshot_with_grants(
        Path::new("terrain.wasm"),
        &terrain_guest(MIN, MAX, COLLISION, true, check),
        ModGrants {
            terrain,
            ..Default::default()
        },
    )
    .unwrap()
}
fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.,
            y: 64.,
            z: 0.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: false,
        frame_seconds: 0.016,
        players: vec![],
    }
}
/// Observes the terrain session, then runs one frame.
fn run(host: &mut ModHost) -> anyhow::Result<()> {
    let world = crate::WorldObservation {
        session: 1,
        dimension: 0,
        world: "fixture".into(),
        position: (0., 63., 0.),
        alive: true,
    };
    host.observe_presentation(Some(world), None, None, Vec::new())?;
    host.frame_with_gameplay(false, Some(snapshot()))
}
fn region() -> TerrainRegion {
    TerrainRegion {
        stamp: TerrainStamp {
            session: 1,
            dimension: 0,
            registry: 3,
            revision: 0,
        },
        chunks: vec![],
        layer_count: 1,
        loaded: vec![0xff],
        states: vec![0; 8],
        palette: vec![],
        shape_offsets: vec![0, 0],
        shape_boxes: vec![],
        overrides: vec![],
    }
}
/// A world whose every check reports `check`, recording what it samples.
struct Script {
    check: TerrainCheck,
    sampled: Vec<TerrainQuery>,
}
impl TerrainSource for Script {
    fn check(&mut self, _: &TerrainQuery, _: Option<&TerrainRegion>) -> TerrainCheck {
        self.check
    }
    fn sample(&mut self, query: &TerrainQuery) -> TerrainRefresh {
        self.sampled.push(*query);
        TerrainRefresh::Replace(Box::new(region()))
    }
}
fn script(check: TerrainCheck) -> Script {
    Script {
        check,
        sampled: Vec::new(),
    }
}
const STALE: TerrainCheck = TerrainCheck::Stale { cells: 8 };

/// Samples before the callback as the client does, then runs one frame.
fn frame(host: &mut ModHost, check: TerrainCheck, budget: usize) -> anyhow::Result<()> {
    TerrainScheduler::default().refresh(&mut [&mut *host], budget, &mut script(check));
    run(host)
}

/// New queries are pending, then deliver a region once and an unchanged stamp while current.
#[test]
fn polling_crosses_the_component_boundary() {
    let mut host = load(
        &expect_reads(&[PENDING, CHANGED, UNCHANGED, CHANGED, UNCHANGED, PENDING]),
        true,
    );
    run(&mut host).unwrap();
    for (check, budget) in [
        (STALE, 8),
        (TerrainCheck::Current, 8),
        (STALE, 8),
        (TerrainCheck::Current, 8),
        (STALE, 7),
    ] {
        frame(&mut host, check, budget).unwrap();
    }
}

/// A deferred callback keeps its subscription, so it reads its last sample when it resumes.
#[test]
fn deferred_callbacks_keep_their_subscriptions() {
    let mut host = load(&expect_reads(&[PENDING, CHANGED, UNCHANGED]), true);
    let budget = CallbackBudget::new;
    host.set_frame_authority(
        budget(mod_api::TOTAL_MOD_FRAME_FUEL),
        false,
        false,
        0,
        HudClaims::default(),
    );
    run(&mut host).unwrap();
    frame(&mut host, STALE, 8).unwrap();
    host.set_frame_authority(budget(0), false, false, 0, HudClaims::default());
    frame(&mut host, TerrainCheck::Current, 8).unwrap();
    assert_eq!(host.instance.terrain_subscriptions(), 1);
    host.set_frame_authority(
        budget(mod_api::TOTAL_MOD_FRAME_FUEL),
        false,
        false,
        0,
        HudClaims::default(),
    );
    frame(&mut host, TerrainCheck::Current, 8).unwrap();
}

/// With every region changing every frame, round-robin sampling reaches each of two to four
/// mods' full-size subscriptions within ceil(total cells / budget) frames, without callbacks.
#[test]
fn round_robin_sampling_reaches_every_mod() {
    let cells = mod_api::MAX_TERRAIN_CELLS;
    let side = i32::from(mod_api::MAX_TERRAIN_EXTENT);
    for count in 2..=4 {
        let mut hosts: Vec<ModHost> = (0..count)
            .map(|index| {
                let min = [index * side, 56, 0];
                let max = [min[0] + side, min[1] + side, side];
                ModHost::load_snapshot_with_grants(
                    Path::new("terrain.wasm"),
                    &terrain_guest(min, max, COLLISION, true, ""),
                    ModGrants {
                        terrain: true,
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        for host in &mut hosts {
            let world = crate::WorldObservation {
                session: 1,
                dimension: 0,
                world: "fixture".into(),
                position: (32., 64., 8.),
                alive: true,
            };
            host.observe_presentation(Some(world), None, None, Vec::new())
                .unwrap();
            host.frame_with_gameplay(false, Some(snapshot())).unwrap();
            assert_eq!(host.instance.terrain_subscriptions(), 1);
        }
        let budget = mod_api::MAX_TERRAIN_FRAME_CELLS;
        let bound = (count as usize * cells).div_ceil(budget);
        let mut scheduler = TerrainScheduler::default();
        let mut last = vec![0; count as usize];
        for frame in 1..=6 {
            let mut source = script(TerrainCheck::Stale { cells });
            let mut borrowed: Vec<&mut ModHost> = hosts.iter_mut().collect();
            scheduler.refresh(&mut borrowed, budget, &mut source);
            for query in source.sampled {
                last[(query.min.x / side) as usize] = frame;
            }
            if frame >= bound {
                assert!(
                    last.iter().all(|&at| frame - at < bound),
                    "{count}: {last:?}"
                );
            }
        }
    }
}

#[test]
fn denied_or_trapped_reads_subscribe_nothing() {
    let mut denied = load(&expect_error(0), false);
    run(&mut denied).unwrap();
    assert_eq!(denied.instance.terrain_subscriptions(), 0);
    let mut trapped = load("unreachable", true);
    assert!(run(&mut trapped).is_err());
    assert!(!trapped.is_active());
    assert_eq!(trapped.instance.terrain_subscriptions(), 0);
}

#[test]
fn terrain_compute_remains_bounded() {
    let mut host = load("(loop $forever br $forever)", true);
    assert!(run(&mut host).is_err());
    assert!(!host.is_active());
}

/// A read result the guest does not expect traps its frame.
#[test]
fn unexpected_results_trap() {
    let mut host = load(&expect_reads(&[CHANGED]), true);
    assert!(run(&mut host).is_err());
    assert!(!host.is_active());
}
