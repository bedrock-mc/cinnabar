use super::{
    ambient::MusicScheduler,
    engine::{AudioEngine, SoundRequest},
    settings::AudioCategory,
};

const MUSIC_TRANSITION_SECONDS: f32 = 3.0;

pub(super) fn drive_music(
    engine: &mut AudioEngine,
    scheduler: &mut MusicScheduler,
    key: &str,
    dt: f32,
) {
    let previous = scheduler.context();
    if previous != Some(key) && (previous == Some("credits") || key == "credits") {
        engine.fade_out_music(MUSIC_TRANSITION_SECONDS);
    }
    let entry = engine
        .bank()
        .and_then(|bank| bank.music(key))
        .map(|entry| (entry.event_name.clone(), (entry.min_delay, entry.max_delay)));
    let playing = engine.is_category_active(AudioCategory::Music);
    let mut rolls = [engine.unit(), engine.unit()].into_iter();
    let delay = entry.as_ref().map_or((0.0, 0.0), |(_, delay)| *delay);
    let start = scheduler.update(key, delay, playing, dt, || rolls.next().unwrap_or(0.5));
    if let Some((event_name, _)) = entry
        && start
    {
        engine.enqueue(SoundRequest::new(&*event_name));
    }
}
