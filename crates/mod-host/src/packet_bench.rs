//! Replays a packet-mod capture and reports per-call latency and peak guest memory.

use anyhow::{Result, ensure};
use mod_host::{
    ModGrants,
    packet_mod::{CompiledPacketMod, capture},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Loads the component with packet and WASI authority, then replays every capture record.
pub(crate) fn run(component: &Path, capture: &Path) -> Result<()> {
    let records = capture::parse(&std::fs::read(capture)?)?;
    let grants = ModGrants {
        packets: true,
        wasi: true,
        ..Default::default()
    };
    let started = Instant::now();
    let loaded = CompiledPacketMod::load(component, &grants)?;
    let compile = started.elapsed();
    let started = Instant::now();
    let mut instance = loaded.instantiate()?;
    let initialize = started.elapsed();
    println!(
        "profile={} compile_ms={:.1} initialize_ms={:.1}",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        millis(compile),
        millis(initialize)
    );
    let mut samples: [(&str, Vec<Duration>); 4] = [
        ("start", Vec::new()),
        ("from_client", Vec::new()),
        ("from_server", Vec::new()),
        ("tick", Vec::new()),
    ];
    let (mut replaced, mut dropped, mut injected) = (0_usize, 0_usize, 0_usize);
    for record in &records {
        let started = Instant::now();
        let slot = match record {
            capture::Record::Start {
                identity,
                client_data,
                startup,
            } => {
                let packs = instance.start(identity, client_data, startup)?;
                println!("start packs={}", packs.len());
                0
            }
            capture::Record::Batch { direction, body } => {
                match instance.on_batch(*direction, body)? {
                    Some(body) if body.is_empty() => dropped += 1,
                    Some(_) => replaced += 1,
                    None => {}
                }
                match direction {
                    mod_host::packet_mod::Direction::FromClient => 1,
                    mod_host::packet_mod::Direction::FromServer => 2,
                }
            }
            capture::Record::Tick => {
                let outbound = instance.tick()?;
                injected += usize::from(!outbound.to_client.is_empty())
                    + usize::from(!outbound.to_server.is_empty());
                3
            }
        };
        samples[slot].1.push(started.elapsed());
    }
    ensure!(instance.is_active(), "packet mod was quarantined");
    instance.stop();
    for (name, mut durations) in samples {
        if durations.is_empty() {
            continue;
        }
        durations.sort();
        let at = |quantile: f64| {
            let index = ((durations.len() - 1) as f64 * quantile).round() as usize;
            durations[index].as_secs_f64() * 1e6
        };
        println!(
            "{name} calls={} us_p50={:.1} us_p95={:.1} us_p99={:.1} us_max={:.1}",
            durations.len(),
            at(0.5),
            at(0.95),
            at(0.99),
            at(1.0)
        );
    }
    println!(
        "replaced={replaced} dropped={dropped} injected={injected} peak_memory_mib={:.1}",
        instance.peak_memory_bytes() as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}
