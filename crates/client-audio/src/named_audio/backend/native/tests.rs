use super::*;
use std::sync::atomic::Ordering;

type TestStream = rodio::source::UniformSourceIterator<MixerLease, f32>;

/// Creates the production output route with a pull-driven transport instead of hardware.
fn route() -> (
    Route<u32, TestStream>,
    Arc<dynamic_mixer::DynamicMixerController<f32>>,
) {
    let (controller, mixer) = dynamic_mixer::mixer::<f32>(CAPTURE_CHANNELS, MIXER_RATE);
    (
        Route {
            stream: None,
            mixer: Arc::new(Mutex::new(Some(mixer))),
        },
        controller,
    )
}

/// Attaches a hardware-format conversion to the retained mixer without using an audio device.
fn open(device: &u32, source: MixerLease) -> Option<TestStream> {
    Some(rodio::source::UniformSourceIterator::new(
        source,
        CAPTURE_CHANNELS,
        *device,
    ))
}

#[test]
fn changing_default_output_preserves_active_and_pending_sources() {
    let (mut route, controller) = route();
    let samples: Vec<_> = (0..128).map(|sample| sample as f32 / 256.0).collect();
    controller.add(rodio::buffer::SamplesBuffer::new(
        CAPTURE_CHANNELS,
        MIXER_RATE,
        samples.clone(),
    ));
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    let previous = &mut route.stream.as_mut().unwrap().1;
    for expected in &samples[..16] {
        assert_eq!(previous.next(), Some(*expected));
    }
    controller.add(rodio::buffer::SamplesBuffer::new(
        CAPTURE_CHANNELS,
        MIXER_RATE,
        vec![0.25; 128],
    ));
    // Endpoint identity may change even when the hardware format is identical.
    route.refresh(Some(MIXER_RATE), |_, _| false, open);
    let replacement = &mut route.stream.as_mut().unwrap().1;
    for expected in &samples[16..32] {
        assert_eq!(replacement.next(), Some(*expected + 0.25));
    }
    assert!(route.stream.is_some());
}

#[test]
fn an_unchanged_endpoint_keeps_the_existing_stream() {
    let (mut route, controller) = route();
    controller.add(rodio::buffer::SamplesBuffer::new(
        CAPTURE_CHANNELS,
        MIXER_RATE,
        vec![0.25; 128],
    ));
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    for _ in 0..3 {
        route.refresh(Some(MIXER_RATE), PartialEq::eq, |_, _| {
            panic!("reopened an unchanged device")
        });
        assert_eq!(route.stream.as_mut().unwrap().1.next(), Some(0.25));
    }
}

#[test]
fn an_idle_mixer_stays_attached_for_later_sounds() {
    let (mut route, controller) = route();
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    let (hardware, mut output) = dynamic_mixer::mixer::<f32>(CAPTURE_CHANNELS, MIXER_RATE);
    hardware.add(route.stream.take().unwrap().1);
    for _ in 0..16 {
        assert_eq!(output.next(), Some(0.0));
    }
    controller.add(rodio::buffer::SamplesBuffer::new(
        CAPTURE_CHANNELS,
        MIXER_RATE,
        vec![0.25; 128],
    ));
    assert_eq!(output.next(), Some(0.25));
}

#[test]
fn replacement_waits_for_the_old_callback_to_release_the_mixer() {
    let (mut route, _) = route();
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    // Model a backend whose callback outlives the stream owner's drop.
    let old_callback = route.stream.take().unwrap().1;
    route.refresh(Some(44_100), PartialEq::eq, |_, _| {
        panic!("old callback still owns the mixer")
    });
    assert!(route.stream.is_none());
    drop(old_callback);
    route.refresh(Some(44_100), PartialEq::eq, open);
    assert!(route.stream.is_some());
}

#[test]
fn unplug_and_failed_reopens_retain_voice_ownership_until_recovery() {
    let (mut route, controller) = route();
    let pool = Arc::new(super::super::PermitPool::default());
    let (source, control) =
        super::super::CancelablePcm::prepare(&crate::named_audio::test_support::sample(), &pool)
            .unwrap();
    controller.add(source.convert_samples::<f32>());
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    route.stream.as_mut().unwrap().1.next();
    route.refresh(None, PartialEq::eq, open);
    assert!(route.stream.is_none());
    assert_eq!(pool.occupied(), 1);
    route.refresh(Some(44_100), PartialEq::eq, |_, _| None);
    assert!(route.stream.is_none());
    assert_eq!(pool.occupied(), 1);
    control.cancel();
    route.refresh(Some(44_100), PartialEq::eq, open);
    assert!(route.stream.is_some());
    for _ in 0..1024 {
        route.stream.as_mut().unwrap().1.next();
        if pool.occupied() == 0 {
            break;
        }
    }
    assert_eq!(pool.occupied(), 0);
}

#[test]
fn no_initial_device_can_recover_and_shutdown_releases_pending_sources() {
    let (mut route, controller) = route();
    route.refresh(None, PartialEq::eq, open);
    assert!(route.stream.is_none());
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    let pool = Arc::new(super::super::PermitPool::default());
    let (source, _) =
        super::super::CancelablePcm::prepare(&crate::named_audio::test_support::sample(), &pool)
            .unwrap();
    controller.add(source.convert_samples::<f32>());
    drop(route);
    drop(controller);
    assert_eq!(pool.occupied(), 0);
}

#[test]
fn a_temporary_output_gap_does_not_discard_the_owner() {
    let (controller, _) = dynamic_mixer::mixer::<f32>(CAPTURE_CHANNELS, MIXER_RATE);
    let (release, released) = mpsc::channel();
    let mut device = super::super::AudioDevice {
        output: Some(Output {
            mixer: controller,
            _release: release,
        }),
        ..super::super::AudioDevice::disabled()
    };
    assert!(device.available());
    assert!(!device.is_opening());
    assert!(device.play_source(rodio::source::Zero::<f32>::new(
        CAPTURE_CHANNELS,
        MIXER_RATE
    )));
    assert!(matches!(
        released.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(device.available());
    assert!(device.play_source(rodio::source::Zero::<f32>::new(
        CAPTURE_CHANNELS,
        MIXER_RATE
    )));
}

#[test]
fn a_device_change_during_named_submission_preserves_all_sources() {
    let (mut route, controller) = route();
    let (release, _released) = mpsc::channel();
    let mut device = super::super::AudioDevice {
        output: Some(Output {
            mixer: controller,
            _release: release,
        }),
        ..super::super::AudioDevice::disabled()
    };
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    let sample = crate::named_audio::test_support::sample();
    let mut audio = crate::named_audio::NamedAudio::new(None);
    for slot in 0..2 {
        let (source, control) = super::super::CancelablePcm::prepare(&sample, &audio.pool).unwrap();
        audio.controls[slot] = Some(control);
        audio.pending[slot] = Some(source);
    }
    // The owner can replace the output after the game thread has checked readiness.
    assert!(!device.is_opening());
    audio.flush(|source| {
        route.refresh(None, PartialEq::eq, open);
        device.submit(source)
    });
    assert_eq!(audio.stats.backend_failed, 0);
    assert_eq!(audio.stats.submitted, 2);
    assert!(
        audio
            .controls
            .iter()
            .flatten()
            .all(|control| !control.cancel.load(Ordering::Acquire))
    );
    assert_eq!(audio.pool.occupied(), 2);
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    assert!(
        route
            .stream
            .as_mut()
            .unwrap()
            .1
            .by_ref()
            .take(64)
            .any(|sample| sample > 0.0)
    );
}

#[test]
fn named_sources_queue_without_failure_during_an_output_gap() {
    let (mut route, controller) = route();
    let (release, _released) = mpsc::channel();
    let mut device = super::super::AudioDevice {
        output: Some(Output {
            mixer: controller,
            _release: release,
        }),
        ..super::super::AudioDevice::disabled()
    };
    let sample = crate::named_audio::test_support::sample();
    let mut audio = crate::named_audio::NamedAudio::new(None);
    let (source, control) = super::super::CancelablePcm::prepare(&sample, &audio.pool).unwrap();
    audio.controls[0] = Some(control);
    audio.pending[0] = Some(source);
    audio.flush_device(&mut device);
    assert_eq!(audio.stats.submitted, 1);
    assert_eq!(audio.stats.backend_failed, 0);
    assert_eq!(audio.pool.occupied(), 1);
    route.refresh(Some(MIXER_RATE), PartialEq::eq, open);
    audio.flush_device(&mut device);
    assert_eq!(audio.stats.submitted, 1);
    assert_eq!(audio.stats.backend_failed, 0);
    assert!(
        route
            .stream
            .as_mut()
            .unwrap()
            .1
            .by_ref()
            .take(64)
            .any(|sample| sample > 0.0)
    );
}
