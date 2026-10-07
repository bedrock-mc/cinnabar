//! Small SDK tools; none of these commands starts a client or network session.

use anyhow::{Result, bail, ensure};
use mod_host::{ModGrants, ModHost};
use std::{hint::black_box, path::Path, time::Instant};

/// Dispatches packaging, an input smoke test, or the bounded callback benchmark.
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "server-helper" => mod_host::helper::serve_developer()?,
        [command, source, output] if command == "pack" => {
            let bytes = std::fs::read(source)?;
            let component = wit_component::ComponentEncoder::default()
                .module(&bytes)?
                .validate(true)
                .encode()?;
            std::fs::write(output, component)?;
        }
        [command, path] if command == "probe" || command == "probe-environment" => {
            let mut host = ModHost::load_with_grants(
                Path::new(path),
                ModGrants {
                    environment: command == "probe-environment",
                    ..Default::default()
                },
            )?;
            let initial = host.label().map(str::to_owned);
            ensure!(initial.is_some(), "sample did not publish its label");
            host.frame(false)?;
            ensure!(
                host.label() == initial.as_deref(),
                "idle frame changed the label"
            );
            host.frame(true)?;
            ensure!(
                host.label().is_some() && host.label() != initial.as_deref(),
                "sample did not react to input"
            );
            println!("time_override={:?}", host.time_override());
            println!("initial={initial:?}\nafter_key={:?}", host.label());
        }
        [command, path] if command == "bench" => benchmark(Path::new(path))?,
        [command, path] if command == "probe-render" => probe_render(Path::new(path))?,
        _ => bail!(
            "usage: mod-host pack CORE.wasm COMPONENT.wasm | probe[-environment] COMPONENT.wasm | probe-render COMPONENT.wasm | bench COMPONENT.wasm"
        ),
    }
    Ok(())
}

/// Drives a render mod through synthetic gameplay, every key edge and panel event.
fn probe_render(path: &Path) -> Result<()> {
    let grants = ModGrants {
        players: true,
        controls: true,
        render: true,
        ..Default::default()
    };
    let mut host = ModHost::load_with_grants(path, grants)?;
    println!(
        "passes={:?}",
        host.render()
            .0
            .passes
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );
    let keys = ["Digit1", "Digit2", "Digit3", "Digit4", "Space", "Space"];
    let panel = ["telegraph", "phase2", "health", "die"];
    let mut peak = [0; 4];
    for frame in 0..240_usize {
        let snapshot = mod_host::GameplaySnapshot {
            session: 1,
            dimension: 0,
            eye: mod_host::GameplayVector3 {
                x: frame as f32 * 0.05,
                y: 65.62,
                z: 0.0,
            },
            yaw: frame as f32 * 0.01,
            pitch: 0.0,
            attack_held: frame % 20 < 3,
            frame_seconds: 1.0 / 60.0,
            players: Vec::new(),
        };
        let mut controls = mod_host::empty_controls();
        controls.seconds = 1.0 / 60.0;
        controls.focused = true;
        controls.gameplay = true;
        if let Some(key) = keys.get(frame / 10).filter(|_| frame % 10 == 0) {
            controls.keys_pressed.push((*key).into());
        }
        if let Some(id) = panel.get(frame / 30).filter(|_| frame % 30 == 15) {
            controls.events.push(mod_host::ControlEvent {
                id: (*id).into(),
                value: if *id == "health" { 0.15 } else { 1.0 },
            });
        }
        host.frame_with_controls(false, Some(snapshot), controls)?;
        let primitives = &host.render().0.primitives;
        for (slot, count) in [
            primitives.decals.len(),
            primitives.ribbons.len(),
            primitives.beams.len(),
            primitives.billboards.len(),
        ]
        .into_iter()
        .enumerate()
        {
            peak[slot] = peak[slot].max(count);
        }
    }
    ensure!(host.is_active(), "render mod was quarantined");
    let enabled: Vec<_> = host.render().0.passes.iter().map(|p| p.enabled).collect();
    println!(
        "frames=240 peak_decals={} peak_ribbons={} peak_beams={} peak_billboards={} enabled={enabled:?}",
        peak[0], peak[1], peak[2], peak[3]
    );
    Ok(())
}

/// Measures warmed, idle frame crossings in batches, excluding compilation and UI.
fn benchmark(path: &Path) -> Result<()> {
    let start = Instant::now();
    let mut host = ModHost::load(path)?;
    let load = start.elapsed();
    println!(
        "profile={} arch={} os={} load_ms={:.3}",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
        load.as_secs_f64() * 1000.0,
    );
    measure_frames(&mut host, false)?;
    measure_frames(&mut host, true)
}

/// Reports separate idle and action costs so retained UI does not hide updates.
fn measure_frames(host: &mut ModHost, pressed: bool) -> Result<()> {
    for _ in 0..10_000 {
        host.frame(black_box(pressed))?;
    }
    let mut samples = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        for _ in 0..1_000 {
            host.frame(black_box(pressed))?;
            black_box(host.label());
        }
        samples.push(start.elapsed().as_nanos() as f64 / 1_000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "pressed={pressed} batches=100 frames_per_batch=1000 frame_ns_p50={:.1} frame_ns_p95={:.1} frame_ns_max={:.1}",
        samples[50], samples[95], samples[99]
    );
    Ok(())
}
