//! Voice management: definition resolution, randomization, attenuation, limits and managed loops.

use std::{collections::HashMap, sync::Arc};

use assets::FloatRange;
use bevy::prelude::Resource;

use super::{
    bank::{PcmLookup, SoundBank},
    echo::{EchoLedger, EchoOrigin, EchoSubject},
    settings::{AudioCategory, AudioSettings},
    voice::{VoiceShared, VoiceSource, attenuation, pan_for},
};

/// Concurrent voice ceiling; needs native measurement.
pub(super) const MAX_VOICES: usize = 48;
const MAX_QUEUED: usize = 256;
/// Starts waiting on a decode, across frames.
const MAX_PENDING: usize = 64;
const MAX_SAME_SOUND: usize = 6;
/// Audible radius of a positional sound without an explicit `max_distance`.
const DEFAULT_MAX_DISTANCE: f32 = 16.0;
/// Seconds a managed loop takes to fade in or out; needs native measurement.
const LOOP_FADE_SECONDS: f32 = 1.5;
/// A one-shot still decoding after this long is dropped rather than played late.
const MAX_DECODE_WAIT_SECONDS: f64 = 1.0;

mod music;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub position: [f32; 3],
    pub right: [f32; 3],
}

/// A request to play a named sound definition; ranges are sampled once at start.
#[derive(Clone, Debug)]
pub struct SoundRequest {
    pub name: Arc<str>,
    /// World position in blocks; `None` plays non-positionally.
    pub position: Option<[f32; 3]>,
    pub volume: FloatRange,
    pub pitch: FloatRange,
    pub looping: bool,
    /// Jukebox cell that owns this record, so its stop silences only this voice.
    pub jukebox: Option<[i32; 3]>,
}

impl SoundRequest {
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            position: None,
            volume: FloatRange::ONE,
            pitch: FloatRange::ONE,
            looping: false,
            jukebox: None,
        }
    }

    pub fn at(mut self, position: [f32; 3]) -> Self {
        self.position = Some(position);
        self
    }

    pub fn with_ranges(mut self, volume: FloatRange, pitch: FloatRange) -> Self {
        self.volume = volume;
        self.pitch = pitch;
        self
    }

    pub fn scaled(mut self, volume: f32, pitch: f32) -> Self {
        self.volume = FloatRange {
            min: self.volume.min * volume,
            max: self.volume.max * volume,
        };
        self.pitch = FloatRange {
            min: self.pitch.min * pitch,
            max: self.pitch.max * pitch,
        };
        self
    }
}

/// Desired state of one looping ambience channel.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopSpec {
    pub name: Arc<str>,
    pub volume: f32,
}

#[derive(Debug, Default)]
pub struct EngineStats {
    pub started: u64,
    pub no_bank: u64,
    pub no_definition: u64,
    pub no_pcm: u64,
    pub out_of_range: u64,
    pub no_listener: u64,
    pub voice_limit: u64,
    pub queue_overflow: u64,
    pub decode_backlog: u64,
    pub stale: u64,
    pub unrouted: u64,
    pub backend_failed: u64,
}

struct Voice {
    shared: Arc<VoiceShared>,
    name: Arc<str>,
    category: AudioCategory,
    /// Definition x alternative x request volume, fixed at start.
    base: f32,
    level: f32,
    target_level: f32,
    fade_per_second: f32,
    position: Option<[f32; 3]>,
    min: f32,
    max: f32,
    key: Option<&'static str>,
    jukebox: Option<[i32; 3]>,
    priority: u8,
    gain: f32,
}

/// A start waiting for its PCM; replayed with the same rolls so it resolves identically.
struct PendingStart {
    request: SoundRequest,
    managed: Option<(&'static str, f32)>,
    roll: [f32; 3],
    path: Box<str>,
    category: AudioCategory,
    /// Loops and streamed tracks wait for their decode however long it takes.
    patient: bool,
    queued_at: f64,
}

fn priority(category: AudioCategory) -> u8 {
    match category {
        AudioCategory::Ui => 4,
        AudioCategory::Music | AudioCategory::Records | AudioCategory::Weather => 3,
        AudioCategory::Ambient => 1,
        _ => 2,
    }
}

#[derive(Resource)]
pub struct AudioEngine {
    bank: Option<SoundBank>,
    voices: Vec<Voice>,
    queue: Vec<SoundRequest>,
    loops: HashMap<&'static str, LoopSpec>,
    rng: u64,
    pub stats: EngineStats,
    server_seen: u64,
    clock: f64,
    echoes: EchoLedger,
    last_server_thunder: f64,
    pending: Vec<PendingStart>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AudioEngine {
    pub fn new(bank: Option<SoundBank>) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9, |elapsed| elapsed.as_nanos() as u64);
        Self {
            bank,
            voices: Vec::new(),
            queue: Vec::new(),
            loops: HashMap::new(),
            rng: seed | 1,
            stats: EngineStats::default(),
            server_seen: 0,
            clock: 0.0,
            echoes: EchoLedger::default(),
            last_server_thunder: f64::NEG_INFINITY,
            pending: Vec::new(),
        }
    }

    /// Whether a sound both the client and the server may voice should play; see [`EchoLedger`].
    pub fn admit_echo(
        &mut self,
        origin: EchoOrigin,
        event: &str,
        subject: EchoSubject,
        window: f64,
    ) -> bool {
        self.echoes
            .admit(origin, event, subject, window, self.clock)
    }

    pub fn note_server_thunder(&mut self) {
        self.last_server_thunder = self.clock;
    }

    /// Whether the server voiced any thunder within the last `seconds`.
    pub fn server_thundered_within(&self, seconds: f64) -> bool {
        self.clock - self.last_server_thunder <= seconds
    }

    pub fn has_bank(&self) -> bool {
        self.bank.is_some()
    }

    pub fn bank(&self) -> Option<&SoundBank> {
        self.bank.as_ref()
    }

    /// Uniform sample in `[0, 1)` from an internal SplitMix64 stream.
    pub fn unit(&mut self) -> f32 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1_u64 << 24) as f32
    }

    pub fn enqueue(&mut self, request: SoundRequest) {
        if self.queue.len() >= MAX_QUEUED {
            self.stats.queue_overflow += 1;
            return;
        }
        self.queue.push(request);
    }

    pub fn is_playing_category(&self, category: AudioCategory) -> bool {
        self.voices
            .iter()
            .any(|voice| voice.category == category && !voice.shared.finished())
            || self.pending.iter().any(|start| start.category == category)
    }

    /// Cancels every voice playing `name`, and starts of it requested earlier but not yet begun.
    pub fn stop_named(&mut self, name: &str) {
        for voice in self.voices.iter().filter(|voice| &*voice.name == name) {
            voice.shared.cancel();
        }
        self.queue.retain(|request| &*request.name != name);
        self.pending.retain(|start| &*start.request.name != name);
    }

    /// Cancels the record owned by the jukebox at `cell`, playing or not yet started.
    pub fn stop_jukebox(&mut self, cell: [i32; 3]) {
        for voice in self
            .voices
            .iter()
            .filter(|voice| voice.jukebox == Some(cell))
        {
            voice.shared.cancel();
        }
        self.queue.retain(|request| request.jukebox != Some(cell));
        self.pending
            .retain(|start| start.request.jukebox != Some(cell));
    }

    /// Whether the jukebox at `cell` has a record queued, decoding, or playing.
    pub fn is_jukebox_active(&self, cell: [i32; 3]) -> bool {
        self.queue
            .iter()
            .any(|request| request.jukebox == Some(cell))
            || (self.pending.iter()).any(|start| start.request.jukebox == Some(cell))
            || (self.voices.iter())
                .any(|voice| voice.jukebox == Some(cell) && !voice.shared.finished())
    }

    /// Cancels every voice and not-yet-started sound of `category`.
    pub fn stop_category(&mut self, category: AudioCategory) {
        for voice in self
            .voices
            .iter()
            .filter(|voice| voice.category == category)
        {
            voice.shared.cancel();
        }
        self.pending.retain(|start| start.category != category);
        if let Some(bank) = self.bank.as_ref() {
            self.queue.retain(|request| {
                bank.definition(&request.name).is_none_or(|definition| {
                    AudioCategory::from_definition(definition.category.as_deref()) != category
                })
            });
        }
    }

    pub fn stop_all(&mut self) {
        self.queue.clear();
        self.pending.clear();
        self.loops.clear();
        self.echoes.clear();
        for voice in &self.voices {
            voice.shared.cancel();
        }
    }

    /// Declares (or clears) the looping ambience on `key`; reconciled on the next pump.
    pub fn set_loop(&mut self, key: &'static str, spec: Option<LoopSpec>) {
        self.pending.retain(|start| {
            start.managed.is_none_or(|(pending_key, _)| {
                pending_key != key
                    || spec
                        .as_ref()
                        .is_some_and(|spec| spec.name == start.request.name)
            })
        });
        match spec {
            Some(spec) => {
                self.loops.insert(key, spec);
            }
            None => {
                self.loops.remove(key);
            }
        }
    }

    pub fn install_server(&mut self, pack: Option<Arc<super::server::ServerSoundPack>>) {
        if let Some(bank) = self.bank.as_mut() {
            bank.install_server(pack);
        }
    }

    /// Drops the server pack on disconnect unless a newer session already published its own.
    pub fn clear_server_if_current(&mut self) {
        if super::server::current_generation() == self.server_seen {
            self.install_server(None);
        }
    }

    pub fn poll_server(&mut self) {
        if let Some(update) = super::server::poll_server_sounds(&mut self.server_seen) {
            self.install_server(update);
        }
    }

    /// Advances fades, starts queued and loop voices, and refreshes every voice's gain and pan.
    pub fn pump(
        &mut self,
        listener: Option<Listener>,
        dt: f32,
        settings: &AudioSettings,
    ) -> Vec<VoiceSource> {
        self.voices.retain(|voice| !voice.shared.finished());
        self.clock += f64::from(dt.max(0.0));
        let mut started = Vec::new();
        self.resume_decoded(&mut started, listener, settings);
        self.reconcile_loops(&mut started, listener, settings);
        for request in std::mem::take(&mut self.queue) {
            if let Some(source) = self.start(&request, listener, settings, None) {
                started.push(source);
            }
        }
        self.refresh(listener, dt, settings);
        if let Some(bank) = self.bank.as_mut() {
            bank.release_unclaimed_streams();
        }
        started
    }

    /// Starts deferred sounds whose decode finished; drops one-shots that waited too long.
    fn resume_decoded(
        &mut self,
        started: &mut Vec<VoiceSource>,
        listener: Option<Listener>,
        settings: &AudioSettings,
    ) {
        let Some(bank) = self.bank.as_mut() else {
            self.pending.clear();
            return;
        };
        bank.poll();
        let clock = self.clock;
        for start in std::mem::take(&mut self.pending) {
            if !start.patient && clock - start.queued_at > MAX_DECODE_WAIT_SECONDS {
                continue;
            }
            if self
                .bank
                .as_ref()
                .is_some_and(|bank| bank.is_decoding(&start.path))
            {
                self.pending.push(start);
                continue;
            }
            let pending_count = self.pending.len();
            if let Some(source) = self.start_rolled(
                &start.request,
                listener,
                settings,
                start.managed,
                start.roll,
            ) {
                started.push(source);
            }
            if self.pending.len() > pending_count {
                self.pending.last_mut().expect("deferred start").queued_at = start.queued_at;
            }
        }
    }

    fn reconcile_loops(
        &mut self,
        started: &mut Vec<VoiceSource>,
        listener: Option<Listener>,
        settings: &AudioSettings,
    ) {
        let fade = 1.0 / LOOP_FADE_SECONDS;
        for voice in &mut self.voices {
            let Some(key) = voice.key else { continue };
            match self.loops.get(key) {
                Some(spec) if spec.name == voice.name => {
                    voice.target_level = spec.volume;
                    voice.fade_per_second = fade;
                }
                _ => {
                    voice.target_level = 0.0;
                    voice.fade_per_second = fade;
                    voice.key = None;
                }
            }
        }
        let missing: Vec<(&'static str, LoopSpec)> = self
            .loops
            .iter()
            .filter(|(key, _)| {
                !self.voices.iter().any(|voice| voice.key == Some(**key))
                    && !self
                        .pending
                        .iter()
                        .any(|start| start.managed.is_some_and(|(pending, _)| pending == **key))
            })
            .map(|(key, spec)| (*key, spec.clone()))
            .collect();
        for (key, spec) in missing {
            let mut request = SoundRequest::new(Arc::clone(&spec.name));
            request.looping = true;
            if let Some(source) = self.start(&request, listener, settings, Some((key, spec.volume)))
            {
                started.push(source);
            }
        }
    }

    fn refresh(&mut self, listener: Option<Listener>, dt: f32, settings: &AudioSettings) {
        for voice in &mut self.voices {
            let step = voice.fade_per_second * dt;
            voice.level = if step.is_finite() && voice.fade_per_second > 0.0 {
                if voice.level < voice.target_level {
                    (voice.level + step).min(voice.target_level)
                } else {
                    (voice.level - step).max(voice.target_level)
                }
            } else {
                voice.target_level
            };
            let (spatial, pan) = match (voice.position, listener) {
                (Some(position), Some(listener)) => {
                    let delta = [
                        position[0] - listener.position[0],
                        position[1] - listener.position[1],
                        position[2] - listener.position[2],
                    ];
                    let distance =
                        (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
                    (
                        attenuation(distance, voice.min, voice.max),
                        pan_for(delta, listener.right),
                    )
                }
                (Some(_), None) => (0.0, 0.0),
                (None, _) => (1.0, 0.0),
            };
            voice.gain = settings.effective(voice.category) * voice.base * voice.level * spatial;
            voice.shared.set(voice.gain, pan);
            if voice.key.is_none() && voice.target_level == 0.0 && voice.level == 0.0 {
                voice.shared.cancel();
            }
        }
    }

    fn start(
        &mut self,
        request: &SoundRequest,
        listener: Option<Listener>,
        settings: &AudioSettings,
        managed: Option<(&'static str, f32)>,
    ) -> Option<VoiceSource> {
        if self.bank.is_none() {
            self.stats.no_bank += 1;
            return None;
        }
        let roll = [self.unit(), self.unit(), self.unit()];
        self.start_rolled(request, listener, settings, managed, roll)
    }

    fn start_rolled(
        &mut self,
        request: &SoundRequest,
        listener: Option<Listener>,
        settings: &AudioSettings,
        managed: Option<(&'static str, f32)>,
        roll: [f32; 3],
    ) -> Option<VoiceSource> {
        let bank = self.bank.as_mut()?;
        let Some(definition) = bank.definition(&request.name) else {
            self.stats.no_definition += 1;
            return None;
        };
        let alternatives: Vec<&assets::AudioAlternative> = definition
            .alternatives
            .iter()
            .filter(|alt| !(settings.low_memory && alt.load_on_low_memory == Some(false)))
            .collect();
        // u64: server alternatives are unbounded in count before admission caps them.
        let total: u64 = alternatives.iter().map(|alt| u64::from(alt.weight)).sum();
        if total == 0 {
            self.stats.no_definition += 1;
            return None;
        }
        let mut pick = ((f64::from(roll[0]) * total as f64) as u64).min(total - 1);
        let chosen = alternatives
            .iter()
            .find(|alt| {
                let weight = u64::from(alt.weight);
                if pick < weight {
                    true
                } else {
                    pick -= weight;
                    false
                }
            })
            .copied()
            .unwrap_or(alternatives[0]);
        let category = AudioCategory::from_definition(definition.category.as_deref());
        let request_volume = request.volume.sample(roll[1]);
        // Vanilla widens the audible range by the raw volume but clamps the playback gain to unity.
        let request_gain = if request_volume.is_finite() {
            request_volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let volume = definition.volume.unwrap_or(1.0) * chosen.volume.unwrap_or(1.0) * request_gain;
        let pitch = (definition.pitch.unwrap_or(1.0)
            * chosen.pitch.unwrap_or(1.0)
            * request.pitch.sample(roll[2]))
        .clamp(0.05, 8.0);
        let flat = matches!(category, AudioCategory::Music | AudioCategory::Ui);
        let positional = request.position.is_some() && chosen.is_3d != Some(false) && !flat;
        let min = definition.min_distance.unwrap_or(0.0).max(0.0);
        let max = definition.max_distance.unwrap_or(DEFAULT_MAX_DISTANCE) * request_volume.max(1.0);
        let stream = chosen.stream == Some(true);
        let path = chosen.name.clone();
        let identifier = definition.identifier.clone();
        let position = positional.then_some(request.position).flatten();
        let mut distance_gain = 1.0;
        if let Some(position) = position {
            let Some(listener) = listener else {
                self.stats.no_listener += 1;
                return None;
            };
            let delta: f32 = (0..3)
                .map(|axis| (position[axis] - listener.position[axis]).powi(2))
                .sum::<f32>()
                .sqrt();
            distance_gain = attenuation(delta, min, max);
            if distance_gain <= 0.0 {
                self.stats.out_of_range += 1;
                return None;
            }
        }
        let lookup = self.bank.as_mut()?.lookup(&path, stream);
        if matches!(lookup, PcmLookup::Busy) {
            self.stats.decode_backlog += 1;
        }
        let pcm = match lookup {
            PcmLookup::Ready(pcm) => pcm,
            PcmLookup::Pending | PcmLookup::Busy => {
                // Unmanaged starts beyond the same-sound cap would be refused once ready.
                let waiting = (self.pending.iter())
                    .filter(|start| start.managed.is_none() && start.path == path)
                    .count();
                if managed.is_none() && waiting >= MAX_SAME_SOUND {
                    self.stats.voice_limit += 1;
                    return None;
                }
                if self.pending.len() >= MAX_PENDING {
                    self.stats.queue_overflow += 1;
                    return None;
                }
                self.pending.push(PendingStart {
                    request: request.clone(),
                    managed,
                    roll,
                    path: path.clone(),
                    category,
                    patient: managed.is_some() || stream,
                    queued_at: self.clock,
                });
                return None;
            }
            PcmLookup::Failed => {
                self.stats.no_pcm += 1;
                return None;
            }
        };
        let same = self
            .voices
            .iter()
            .filter(|voice| *voice.name == *identifier)
            .count();
        if same >= MAX_SAME_SOUND && managed.is_none() {
            self.stats.voice_limit += 1;
            return None;
        }
        let new_priority = priority(category);
        let new_gain = settings.effective(category) * volume * distance_gain;
        if self.voices.len() >= MAX_VOICES {
            let victim = self
                .voices
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (a.priority, a.gain)
                        .partial_cmp(&(b.priority, b.gain))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(index, voice)| (index, voice.priority, voice.gain));
            match victim {
                Some((index, victim_priority, victim_gain))
                    if (new_priority, new_gain) > (victim_priority, victim_gain) =>
                {
                    self.voices[index].shared.cancel();
                    self.voices.swap_remove(index);
                }
                _ => {
                    self.stats.voice_limit += 1;
                    return None;
                }
            }
        }
        let (level, target_level, key) = match managed {
            Some((key, target)) => (0.0, target, Some(key)),
            None => (1.0, 1.0, None),
        };
        let shared = VoiceShared::new(if managed.is_some() { 0.0 } else { new_gain }, 0.0, pitch);
        let source = VoiceSource::new(Arc::clone(&pcm), Arc::clone(&shared), request.looping);
        self.voices.push(Voice {
            shared,
            name: Arc::from(&*identifier),
            category,
            base: volume,
            level,
            target_level,
            fade_per_second: if managed.is_some() {
                1.0 / LOOP_FADE_SECONDS
            } else {
                0.0
            },
            position,
            min,
            max,
            key,
            jukebox: request.jukebox,
            priority: new_priority,
            gain: new_gain,
        });
        self.stats.started += 1;
        Some(source)
    }
}

#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
#[path = "engine_review_tests.rs"]
mod review_tests;
