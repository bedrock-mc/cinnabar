use super::*;
use crate::named_audio::test_support::sample;

#[test]
fn actual_mixer_stall_keeps_all_cancelled_permits_until_node_drop() {
    let sample = sample();
    let pool = Arc::new(PermitPool::default());
    let (controller, mut mixer) = rodio::dynamic_mixer::mixer::<f32>(2, 44100);
    let mut controls = Vec::new();
    for _ in 0..VOICE_LIMIT {
        let (source, control) = CancelablePcm::prepare(&sample, &pool).unwrap();
        controller.add(source.convert_samples::<f32>());
        controls.push(control);
    }
    for control in &controls {
        control.cancel();
    }
    assert_eq!(pool.occupied(), 16);
    assert!(CancelablePcm::prepare(&sample, &pool).is_none());
    // Uniform/resampling may retain a bounded buffered tail; don't assert zero latency.
    for _ in 0..1024 {
        mixer.next();
        if pool.occupied() == 0 {
            break;
        }
    }
    assert_eq!(pool.occupied(), 0);
    for control in controls {
        assert!(pool.retired(control.slot, control.token));
    }
}
#[test]
fn actual_uniform_retains_naturally_exhausted_source_until_outer_drop() {
    let sample = sample();
    let pool = Arc::new(PermitPool::default());
    let (source, _) = CancelablePcm::prepare(&sample, &pool).unwrap();
    let mut outer = rodio::source::UniformSourceIterator::<_, f32>::new(source, 1, 32000);
    let mut exhausted = false;
    for _ in 0..1024 {
        if outer.next().is_none() {
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "qualified finite sample must exhaust within bound"
    );
    assert_eq!(pool.occupied(), 1, "None is not backend Drop");
    drop(outer);
    assert_eq!(pool.occupied(), 0);
}
#[test]
fn mixer_and_controller_drop_release_pending_and_active_nodes() {
    let sample = sample();
    let pool = Arc::new(PermitPool::default());
    let (controller, mut mixer) = rodio::dynamic_mixer::mixer::<f32>(2, 48000);
    for _ in 0..2 {
        let (source, _) = CancelablePcm::prepare(&sample, &pool).unwrap();
        controller.add(source.convert_samples::<f32>());
    }
    mixer.next();
    let (source, _) = CancelablePcm::prepare(&sample, &pool).unwrap();
    controller.add(source.convert_samples::<f32>());
    assert!(pool.occupied() > 0);
    drop(mixer);
    drop(controller);
    assert_eq!(pool.occupied(), 0);
}
#[test]
fn repeated_cancellation_cycles_and_no_device_cannot_remint_or_leak() {
    let sample = sample();
    let pool = Arc::new(PermitPool::default());
    let mut device = AudioDevice::disabled();
    for _ in 0..128 {
        let (source, control) = CancelablePcm::prepare(&sample, &pool).unwrap();
        assert!(!device.submit(source));
        assert!(pool.retired(control.slot, control.token));
        assert_eq!(pool.occupied(), 0);
    }
    pool.slots[0].store(u64::MAX - 1, Ordering::Release);
    let mut permits = Vec::new();
    for _ in 0..15 {
        permits.push(pool.acquire().unwrap());
    }
    assert!(pool.acquire().is_none());
    drop(permits);
    assert_eq!(pool.occupied(), 0);
}

#[test]
fn actual_mixer_repeated_retirement_reuses_capacity_without_old_control_cancelling_new_source() {
    let sample = sample();
    let pool = Arc::new(PermitPool::default());
    let (controller, mut mixer) = rodio::dynamic_mixer::mixer::<f32>(2, 44100);
    let mut previous: Option<VoiceControl> = None;
    for _ in 0..64 {
        let (source, control) = CancelablePcm::prepare(&sample, &pool).unwrap();
        if let Some(old) = previous.take() {
            assert!(pool.retired(old.slot, old.token));
            old.cancel();
            assert!(!control.cancel.load(Ordering::Acquire));
        }
        controller.add(source.convert_samples::<f32>());
        assert_eq!(pool.occupied(), 1);
        control.cancel();
        for _ in 0..1024 {
            mixer.next();
            if pool.occupied() == 0 {
                break;
            }
        }
        assert_eq!(pool.occupied(), 0);
        previous = Some(control);
    }
}
