use super::*;
use crate::audio::{
    voice::OUTPUT_RATE,
    weather::{RAIN_SOUND, RainColumn, RainSoundScheduler},
};

/// An exposed warm surface near the listener.
fn rain_column(surface_y: i32) -> RainColumn {
    RainColumn {
        surface_y,
        temperature: 0.8,
        downfall: 0.4,
    }
}

#[test]
fn rain_emits_finite_positional_sounds_every_two_to_four_eligible_ticks() {
    let mut engine = engine(&[(RAIN_SOUND, "weather")]);
    engine.rng = 7;
    let mut scheduler = RainSoundScheduler::default();
    let mut last = 0;
    let mut emitted = 0;
    for tick in 1..=200 {
        scheduler.tick(1.0, [0.5, 1.6, 0.5], true, &mut engine, |_, _| {
            Some(rain_column(0))
        });
        for request in engine.queue.drain(..) {
            assert!((2..=4).contains(&(tick - last)));
            assert_eq!(&*request.name, RAIN_SOUND);
            assert!(!request.looping);
            assert_eq!(request.volume, FloatRange::ONE);
            assert_eq!(request.pitch, FloatRange::ONE);
            let [x, y, z] = request.position.expect("positional rain");
            assert!((-9.0..10.0).contains(&x));
            assert!((-9.0..10.0).contains(&z));
            assert_eq!(y, 0.2);
            last = tick;
            emitted += 1;
        }
    }
    assert!(emitted >= 50);
}

#[test]
fn clear_unloaded_dry_snowy_and_distant_surfaces_emit_no_rain() {
    let mut engine = engine(&[(RAIN_SOUND, "weather")]);
    let samples = [
        None,
        Some(RainColumn {
            downfall: 0.0,
            ..rain_column(0)
        }),
        Some(RainColumn {
            temperature: 0.0,
            ..rain_column(0)
        }),
        Some(rain_column(12)),
        Some(rain_column(-10)),
    ];
    for sample in samples {
        let mut scheduler = RainSoundScheduler::default();
        for _ in 0..20 {
            scheduler.tick(1.0, [0.0, 1.6, 0.0], true, &mut engine, |_, _| sample);
        }
        assert!(engine.queue.is_empty());
    }
    for level in [0.0, -1.0, 0.09, f32::NAN] {
        let mut scheduler = RainSoundScheduler::default();
        let mut lookups = 0;
        for _ in 0..20 {
            scheduler.tick(level, [0.0; 3], true, &mut engine, |_, _| {
                lookups += 1;
                Some(rain_column(0))
            });
        }
        assert_eq!(lookups, 0);
        assert!(engine.queue.is_empty());
    }
}

#[test]
fn graphics_density_does_not_reduce_the_request_volume() {
    for fancy in [false, true] {
        let mut engine = engine(&[(RAIN_SOUND, "weather")]);
        let mut scheduler = RainSoundScheduler::default();
        let mut lookups = 0;
        scheduler.tick(0.5, [0.0; 3], fancy, &mut engine, |_, _| {
            lookups += 1;
            Some(rain_column(0))
        });
        assert_eq!(lookups, if fancy { 25 } else { 6 });
        for _ in 0..3 {
            scheduler.tick(0.5, [0.0; 3], fancy, &mut engine, |_, _| {
                Some(rain_column(0))
            });
        }
        let request = engine.queue.first().expect("rain by fourth eligible tick");
        assert_eq!(request.volume.min, 0.5);
        assert_eq!(request.volume.max, 0.5);
    }
}

#[test]
fn sampled_rain_coverage_and_shelter_scale_volume_and_pitch() {
    for (height, gain, pitch) in [(0, 1.0, 1.0), (4, 0.62, 0.5), (10, 0.05, 0.5)] {
        let mut engine = engine(&[(RAIN_SOUND, "weather")]);
        let mut scheduler = RainSoundScheduler::default();
        for _ in 0..4 {
            let mut trial = 0;
            scheduler.tick(1.0, [0.0; 3], true, &mut engine, |_, _| {
                trial += 1;
                (trial % 2 == 1).then(|| rain_column(height))
            });
        }
        let request = engine.queue.first().expect("rain by fourth eligible tick");
        assert!((request.volume.min - gain * 0.5).abs() < 1e-6);
        assert_eq!(request.volume.min, request.volume.max);
        assert_eq!(request.pitch.min, pitch);
        assert_eq!(request.pitch.min, request.pitch.max);
    }
}

#[test]
fn ineligible_ticks_preserve_the_pending_emission() {
    let mut engine = engine(&[(RAIN_SOUND, "weather")]);
    let mut scheduler = RainSoundScheduler::default();
    for _ in 0..4 {
        scheduler.tick(1.0, [0.0; 3], true, &mut engine, |_, _| {
            Some(rain_column(0))
        });
        for _ in 0..100 {
            scheduler.tick(0.0, [0.0; 3], true, &mut engine, |_, _| None);
        }
    }
    assert!(!engine.queue.is_empty());
}

#[test]
fn sustained_rain_overlaps_finite_faded_samples_and_clear_lets_them_expire() {
    let mut engine = engine(&[(RAIN_SOUND, "weather")]);
    engine.rng = 7;
    let frames = OUTPUT_RATE as usize * 2;
    let pcm = Arc::new(Pcm {
        channels: 1,
        rate: OUTPUT_RATE,
        samples: (0..frames)
            .map(|i| {
                let fade = i.min(frames - 1 - i).min(OUTPUT_RATE as usize / 4);
                (16000 * fade / (OUTPUT_RATE as usize / 4)) as i16
            })
            .collect::<Vec<_>>()
            .into(),
    });
    engine
        .bank
        .as_mut()
        .unwrap()
        .insert_test_pcm(&format!("sounds/{RAIN_SOUND}"), pcm);
    let settings = AudioSettings::default();
    let listener = Listener {
        position: [0.5, 1.6, 0.5],
        right: [1.0, 0.0, 0.0],
    };
    let (controller, mut mixer) = rodio::dynamic_mixer::mixer::<f32>(2, OUTPUT_RATE);
    let mut scheduler = RainSoundScheduler::default();
    let samples_per_tick = OUTPUT_RATE as usize * 2 / world::TICKS_PER_SECOND as usize;
    let mut levels = Vec::new();
    for tick in 0..180 {
        let rain = if tick < 120 { 1.0 } else { 0.0 };
        scheduler.tick(rain, listener.position, true, &mut engine, |_, _| {
            Some(RainColumn {
                surface_y: 0,
                temperature: 0.8,
                downfall: 0.4,
            })
        });
        for source in engine.pump(
            Some(listener),
            1.0 / world::TICKS_PER_SECOND as f32,
            &settings,
        ) {
            controller.add(source);
        }
        let energy = (&mut mixer)
            .take(samples_per_tick)
            .map(|sample| sample * sample)
            .sum::<f32>();
        levels.push((energy / samples_per_tick as f32).sqrt());
    }
    assert!(
        levels[40..120].iter().all(|rms| *rms > 0.1),
        "rain faded to silence: {:?}",
        &levels[40..120]
    );
    assert!(engine.stats.started > 1, "finite samples must overlap");
    assert!(
        levels[165..].iter().all(|rms| *rms == 0.0),
        "clear weather retained a rain loop"
    );
}
