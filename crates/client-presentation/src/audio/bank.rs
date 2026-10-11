//! Startup-loaded sound bank: routing tables, definition catalog, on-demand FSB decode and cache.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
};

use assets::{
    AudioDefinition, RuntimeAudioCatalog, SoundBankEntry, SoundBankIndex, SoundEventTables,
    decode_sound, sound_bank_prefix_len,
};
use serde_json::Value;

use super::{server::ServerSoundPack, voice::Pcm};

pub const SOUND_BANK_FILENAME: &str = assets::carriers::AUDIO_BANK.output;
const HEADER_READ_BYTES: usize = 40;
const CACHE_BUDGET_BYTES: usize = 96 * 1024 * 1024;
/// Worker threads decoding bank entries, so a streamed track cannot hold up every first-play sound.
const DECODE_WORKERS: usize = 2;
/// Decodes queued at once, and their compressed bytes; a lookup beyond either is `Busy`.
pub(super) const MAX_QUEUED_DECODES: usize = 64;
const MAX_QUEUED_DECODE_BYTES: usize = 32 * 1024 * 1024;
const PREWARM_SOUNDS_PER_GROUP: usize = 4;
const PREWARM_BYTES: usize = 1024 * 1024;

/// Where the sound bank sits relative to the world carrier.
pub fn sound_bank_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(SOUND_BANK_FILENAME)
}

/// One `music_definitions.json` record: the sound event and the gap before the next track.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicEntry {
    pub event_name: Box<str>,
    pub min_delay: f32,
    pub max_delay: f32,
}

pub struct SoundBank {
    file: Option<Arc<File>>,
    index: SoundBankIndex,
    tables: SoundEventTables,
    catalog: Option<Arc<RuntimeAudioCatalog>>,
    server: Option<Arc<ServerSoundPack>>,
    merged: Option<SoundEventTables>,
    music: HashMap<Box<str>, MusicEntry>,
    cache: HashMap<Box<str>, Arc<Pcm>>,
    cache_order: VecDeque<Box<str>>,
    cache_bytes: usize,
    failed: HashSet<Box<str>>,
    decoder: Option<Decoder>,
    decode_generation: u64,
    /// Paths being decoded, with their stream flag and compressed size.
    in_flight: HashMap<Box<str>, (bool, usize)>,
    in_flight_bytes: usize,
    /// Decoded streamed sounds, held until the next pump's starts take them.
    ready_streams: HashMap<Box<str>, Arc<Pcm>>,
}

/// Where a sound's PCM stands; reading and decoding never run on the calling (main) thread.
pub enum PcmLookup {
    Ready(Arc<Pcm>),
    Pending,
    /// The decode backlog is full; the caller may retry later.
    Busy,
    Failed,
}

enum DecodeSource {
    Bank(BankEntry),
    Server,
}

/// A queued entry that keeps the bank file opened during loading alive.
struct BankEntry {
    file: Arc<File>,
    entry: SoundBankEntry,
}

impl BankEntry {
    /// Reads this entry by offset on a worker; returns None if its bytes cannot be read.
    fn read(&self) -> Option<Vec<u8>> {
        let mut bytes = vec![0_u8; self.entry.len as usize];
        let mut consumed = 0;
        while consumed < bytes.len() {
            let offset = self.entry.offset.checked_add(consumed as u64)?;
            #[cfg(unix)]
            let result =
                std::os::unix::fs::FileExt::read_at(&*self.file, &mut bytes[consumed..], offset);
            #[cfg(windows)]
            let result = std::os::windows::fs::FileExt::seek_read(
                &*self.file,
                &mut bytes[consumed..],
                offset,
            );
            match result {
                Ok(0) => return None,
                Ok(read) => consumed += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    bevy::log::debug!(%error, offset, "sound bank read failed");
                    return None;
                }
            }
        }
        Some(bytes)
    }
}

type DecodeJob = (u64, Box<str>, DecodeSource, EncodedPermit);
type DecodeResult = (u64, Box<str>, Option<Pcm>);
type CurrentServer = Arc<Mutex<(u64, Option<Arc<ServerSoundPack>>)>>;

struct EncodedPermit {
    bytes: usize,
    total: Arc<AtomicUsize>,
}

impl Drop for EncodedPermit {
    fn drop(&mut self) {
        self.total.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

struct Decoder {
    jobs: SyncSender<DecodeJob>,
    done: Mutex<Receiver<DecodeResult>>,
    generation: Arc<AtomicU64>,
    encoded_bytes: Arc<AtomicUsize>,
    server: CurrentServer,
}

impl Decoder {
    /// Starts the bounded pool shared by reads and decodes across pack generations.
    fn spawn(generation: u64, server: Option<Arc<ServerSoundPack>>) -> Option<Self> {
        let (jobs, job_queue) = sync_channel::<DecodeJob>(MAX_QUEUED_DECODES);
        let (results, done) = sync_channel(DECODE_WORKERS);
        let job_queue = Arc::new(Mutex::new(job_queue));
        let server = Arc::new(Mutex::new((generation, server)));
        let generation = Arc::new(AtomicU64::new(generation));
        for index in 0..DECODE_WORKERS {
            let job_queue = Arc::clone(&job_queue);
            let results = results.clone();
            let generation = Arc::clone(&generation);
            let server = Arc::clone(&server);
            std::thread::Builder::new()
                .name(format!("sound-decode-{index}"))
                .spawn(move || {
                    loop {
                        let job = job_queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv();
                        let Ok((epoch, path, source, permit)) = job else {
                            return;
                        };
                        if generation.load(Ordering::Acquire) != epoch {
                            continue;
                        }
                        let pcm = match source {
                            DecodeSource::Bank(entry) => {
                                entry.read().and_then(|bytes| decode_pcm(&bytes, &path))
                            }
                            DecodeSource::Server => {
                                let pack = {
                                    let current = server
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    (current.0 == epoch).then(|| current.1.clone()).flatten()
                                };
                                pack.and_then(|pack| pack.decode(&path))
                            }
                        };
                        drop(permit);
                        if generation.load(Ordering::Acquire) != epoch {
                            continue;
                        }
                        if results.send((epoch, path, pcm)).is_err() {
                            return;
                        }
                    }
                })
                .ok()?;
        }
        Some(Self {
            jobs,
            done: Mutex::new(done),
            generation,
            encoded_bytes: Arc::new(AtomicUsize::new(0)),
            server,
        })
    }

    /// Reserves encoded bytes until a worker finishes or discards the job.
    fn reserve(&self, bytes: usize) -> Option<EncodedPermit> {
        self.encoded_bytes
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |held| {
                held.checked_add(bytes)
                    .filter(|total| *total <= MAX_QUEUED_DECODE_BYTES)
            })
            .ok()?;
        Some(EncodedPermit {
            bytes,
            total: Arc::clone(&self.encoded_bytes),
        })
    }
}

/// Decodes one sound into shared playback data on a worker.
fn decode_pcm(bytes: &[u8], path: &str) -> Option<Pcm> {
    let sound = decode_sound(bytes)
        .map_err(|error| bevy::log::debug!(%error, path, "sound decode failed"))
        .ok()?;
    Some(Pcm {
        channels: sound.channels,
        rate: sound.sample_rate,
        samples: sound.samples.into(),
    })
}

fn parse_music(bytes: &[u8]) -> HashMap<Box<str>, MusicEntry> {
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(bytes) else {
        return HashMap::new();
    };
    map.iter()
        .filter_map(|(key, value)| {
            let event_name = value.get("event_name")?.as_str()?;
            let delay = |name: &str, fallback: f64| {
                value.get(name).and_then(Value::as_f64).unwrap_or(fallback) as f32
            };
            Some((
                Box::from(key.as_str()),
                MusicEntry {
                    event_name: event_name.into(),
                    min_delay: delay("min_delay", 60.0),
                    max_delay: delay("max_delay", 180.0),
                },
            ))
        })
        .collect()
}

impl SoundBank {
    /// Opens the bank file; `Ok(None)` when absent so audio stays silent instead of failing startup.
    pub fn open(
        path: &Path,
        catalog: Option<Arc<RuntimeAudioCatalog>>,
    ) -> Result<Option<Self>, String> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("open {}: {error}", path.display())),
        };
        let mut header = [0_u8; HEADER_READ_BYTES];
        file.read_exact(&mut header)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let prefix_len = sound_bank_prefix_len(&header).map_err(|error| error.to_string())?;
        let mut prefix = vec![0_u8; prefix_len];
        prefix[..HEADER_READ_BYTES].copy_from_slice(&header);
        file.read_exact(&mut prefix[HEADER_READ_BYTES..])
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let index = SoundBankIndex::decode_prefix(&prefix).map_err(|error| error.to_string())?;
        let json = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null);
        let tables =
            SoundEventTables::from_json(&json(index.sounds_json()), &json(index.materials_json()));
        Ok(Some(Self {
            file: Some(Arc::new(file)),
            music: parse_music(index.music_json()),
            index,
            tables,
            catalog,
            server: None,
            merged: None,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            cache_bytes: 0,
            failed: HashSet::new(),
            decoder: None,
            decode_generation: 0,
            in_flight: HashMap::new(),
            in_flight_bytes: 0,
            ready_streams: HashMap::new(),
        }))
    }

    /// Queues a small set of finite UI, footstep and interaction sounds during loading.
    pub fn prewarm_common(&mut self) {
        let Some(catalog) = &self.catalog else { return };
        let mut counts = [0; 3];
        let mut bytes = 0;
        let mut paths = Vec::new();
        for definition in catalog.definitions() {
            let group = if definition.category.as_deref() == Some("ui") {
                0
            } else if definition.identifier.starts_with("step.") {
                1
            } else if definition.identifier.starts_with("dig.")
                || definition.identifier.starts_with("hit.")
            {
                2
            } else {
                continue;
            };
            for alternative in &definition.alternatives {
                if counts[group] == PREWARM_SOUNDS_PER_GROUP {
                    break;
                }
                if alternative.stream == Some(true) || paths.contains(&alternative.name) {
                    continue;
                }
                let Some(entry) = self.index.entry(&alternative.name) else {
                    continue;
                };
                let size = entry.len as usize;
                if bytes + size > PREWARM_BYTES {
                    continue;
                }
                paths.push(alternative.name.clone());
                bytes += size;
                counts[group] += 1;
            }
        }
        for path in paths {
            if matches!(self.lookup(&path, false), PcmLookup::Busy) {
                break;
            }
        }
    }

    /// Vanilla routing with the server pack's `sounds.json` layered on top.
    pub fn tables(&self) -> &SoundEventTables {
        self.merged.as_ref().unwrap_or(&self.tables)
    }

    pub fn music(&self, key: &str) -> Option<&MusicEntry> {
        self.music.get(key)
    }

    pub fn file_count(&self) -> usize {
        self.index.len()
    }

    /// Replaces the session's server pack; `None` restores vanilla definitions.
    pub fn install_server(&mut self, pack: Option<Arc<ServerSoundPack>>) {
        if match (&self.server, &pack) {
            (None, None) => true,
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            _ => false,
        } {
            return;
        }
        self.merged = pack
            .as_ref()
            .and_then(|pack| pack.tables.clone())
            .map(|overlay| {
                let mut merged = self.tables.clone();
                merged.merge(overlay);
                merged
            });
        self.server = pack;
        self.decode_generation = self.decode_generation.wrapping_add(1);
        if let Some(decoder) = &self.decoder {
            *decoder
                .server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                (self.decode_generation, self.server.clone());
            decoder
                .generation
                .store(self.decode_generation, Ordering::Release);
        }
        self.in_flight.clear();
        self.in_flight_bytes = 0;
        self.ready_streams.clear();
        self.failed.clear();
        self.cache.clear();
        self.cache_order.clear();
        self.cache_bytes = 0;
    }

    /// Definition by name, the server pack winning over the vanilla catalog.
    pub fn definition(&self, name: &str) -> Option<&AudioDefinition> {
        self.server
            .as_ref()
            .and_then(|pack| pack.definitions.get(name))
            .or_else(|| self.catalog.as_ref()?.lookup(name))
    }

    /// PCM for an alternative's sound path (no extension), queueing a background decode on a
    /// miss; workers read and decode the file, and non-streaming sounds are cached.
    pub fn lookup(&mut self, path: &str, stream: bool) -> PcmLookup {
        if let Some(found) = self.cache.get(path) {
            return PcmLookup::Ready(Arc::clone(found));
        }
        if let Some(found) = self.ready_streams.get(path) {
            return PcmLookup::Ready(Arc::clone(found));
        }
        if self.failed.contains(path) {
            return PcmLookup::Failed;
        }
        if self.in_flight.contains_key(path) {
            return PcmLookup::Pending;
        }
        let server = self.server.as_ref().is_some_and(|pack| pack.contains(path));
        let entry = self.index.entry(path);
        if !server && entry.is_none() {
            self.failed.insert(path.into());
            return PcmLookup::Failed;
        }
        // Server jobs retain a path and generation; bank jobs retain the opened file.
        let size = if server {
            0
        } else {
            entry.map_or(0, |entry| entry.len as usize)
        };
        if !self.in_flight.is_empty()
            && (self.in_flight.len() >= MAX_QUEUED_DECODES
                || self.in_flight_bytes.saturating_add(size) > MAX_QUEUED_DECODE_BYTES)
        {
            return PcmLookup::Busy;
        }
        if self.decoder.is_none() {
            self.decoder = Decoder::spawn(self.decode_generation, self.server.clone());
        }
        let Some(decoder) = &self.decoder else {
            self.failed.insert(path.into());
            return PcmLookup::Failed;
        };
        let Some(permit) = decoder.reserve(size) else {
            return PcmLookup::Busy;
        };
        let jobs = decoder.jobs.clone();
        let source = match server {
            true => Some(DecodeSource::Server),
            false => entry.zip(self.file.as_ref()).map(|(entry, file)| {
                DecodeSource::Bank(BankEntry {
                    file: Arc::clone(file),
                    entry,
                })
            }),
        };
        let Some(source) = source else {
            self.failed.insert(path.into());
            return PcmLookup::Failed;
        };
        let job = (self.decode_generation, path.into(), source, permit);
        match jobs.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return PcmLookup::Busy,
            Err(TrySendError::Disconnected(_)) => {
                self.failed.insert(path.into());
                return PcmLookup::Failed;
            }
        }
        self.in_flight.insert(path.into(), (stream, size));
        self.in_flight_bytes += size;
        PcmLookup::Pending
    }

    /// Whether a decode of `path` is still running.
    pub fn is_decoding(&self, path: &str) -> bool {
        self.in_flight.contains_key(path)
    }

    /// Collects finished decodes.
    pub fn poll(&mut self) {
        for _ in 0..DECODE_WORKERS {
            let finished = self.decoder.as_ref().and_then(|decoder| {
                decoder
                    .done
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .try_recv()
                    .ok()
            });
            let Some((generation, path, pcm)) = finished else {
                break;
            };
            if generation != self.decode_generation {
                continue;
            }
            let (stream, size) = self.in_flight.remove(&path).unwrap_or_default();
            self.in_flight_bytes -= size;
            match pcm.map(Arc::new) {
                None => {
                    self.failed.insert(path);
                }
                Some(pcm) if stream => {
                    self.ready_streams.insert(path, pcm);
                }
                Some(pcm) => self.remember(&path, &pcm),
            }
        }
    }

    /// Drops decoded streams no start claimed, so an abandoned track is not held in memory.
    pub fn release_unclaimed_streams(&mut self) {
        self.ready_streams.clear();
    }

    fn remember(&mut self, path: &str, pcm: &Arc<Pcm>) {
        let size = pcm.samples.len() * 2;
        if size > CACHE_BUDGET_BYTES {
            self.ready_streams.insert(path.into(), Arc::clone(pcm));
            return;
        }
        while self.cache_bytes + size > CACHE_BUDGET_BYTES {
            let Some(oldest) = self.cache_order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.cache.remove(&oldest) {
                self.cache_bytes -= evicted.samples.len() * 2;
            }
        }
        self.cache.insert(path.into(), Arc::clone(pcm));
        self.cache_order.push_back(path.into());
        self.cache_bytes += size;
    }

    #[cfg(test)]
    pub fn insert_test_pcm(&mut self, path: &str, pcm: Arc<Pcm>) {
        self.remember(path, &pcm);
    }

    #[cfg(test)]
    pub fn from_parts(
        index: SoundBankIndex,
        tables: SoundEventTables,
        catalog: Option<Arc<RuntimeAudioCatalog>>,
    ) -> Self {
        Self {
            file: None,
            music: parse_music(index.music_json()),
            index,
            tables,
            catalog,
            server: None,
            merged: None,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            cache_bytes: 0,
            failed: HashSet::new(),
            decoder: None,
            decode_generation: 0,
            in_flight: HashMap::new(),
            in_flight_bytes: 0,
            ready_streams: HashMap::new(),
        }
    }

    /// Blocks until `path` decodes; tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pcm(&mut self, path: &str, stream: bool) -> Option<Arc<Pcm>> {
        loop {
            match self.lookup(path, stream) {
                PcmLookup::Ready(pcm) => return Some(pcm),
                PcmLookup::Failed => return None,
                PcmLookup::Pending | PcmLookup::Busy => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    self.poll();
                }
            }
        }
    }
}

#[cfg(test)]
mod prewarm_tests;
#[cfg(test)]
mod reload_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;

    #[test]
    fn review_ready_stream_is_shared_by_all_starts_until_released() {
        let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &[]).unwrap();
        let index = SoundBankIndex::decode_prefix(&bytes).unwrap();
        let mut bank = SoundBank::from_parts(index, SoundEventTables::default(), None);
        let pcm = Arc::new(Pcm {
            channels: 1,
            rate: 48_000,
            samples: vec![1000; 2].into(),
        });
        bank.ready_streams
            .insert("sounds/stream".into(), pcm.clone());
        for _ in 0..2 {
            let PcmLookup::Ready(found) = bank.lookup("sounds/stream", true) else {
                panic!("concurrent starts must share the completed decode");
            };
            assert!(Arc::ptr_eq(&pcm, &found));
        }
        bank.release_unclaimed_streams();
        assert!(bank.ready_streams.is_empty());
    }

    #[test]
    fn absent_bank_is_none_and_music_parses() {
        let missing = std::env::temp_dir().join("cinnabar-no-such-sound-bank.mcbesnd");
        assert!(SoundBank::open(&missing, None).unwrap().is_none());
        let music = parse_music(
            br#"{"creative":{"event_name":"music.game.creative","min_delay":30,"max_delay":90}}"#,
        );
        assert_eq!(music["creative"].max_delay, 90.0);
    }

    /// One PCM16 mono FSB5 of two frames at 48 kHz.
    pub(super) fn tone() -> Vec<u8> {
        let mut fsb = b"FSB5".to_vec();
        let mode = (9_u64 << 1) | (2_u64 << 34);
        for value in [1_u32, 1, 8, 0, 4, 2, 0, 0] {
            fsb.extend(value.to_le_bytes());
        }
        fsb.resize(60, 0);
        fsb.extend(mode.to_le_bytes());
        fsb.extend([0, 0x40, 0, 0xc0]);
        fsb
    }

    #[test]
    fn lookup_queues_a_descriptor_without_touching_the_file() {
        let bytes =
            assets::encode_sound_bank(b"{}", b"{}", b"{}", &[("sounds/tone".to_owned(), tone())])
                .unwrap();
        let path = std::env::temp_dir().join(format!(
            "cinnabar-no-frame-read-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = SoundBank::open(&path, None).unwrap().unwrap();
        let mut probe = bank.file.as_ref().unwrap().try_clone().unwrap();
        let before = probe.stream_position().unwrap();
        let (jobs, job_queue) = sync_channel(MAX_QUEUED_DECODES);
        let (_results, done) = sync_channel(DECODE_WORKERS);
        bank.decoder = Some(Decoder {
            jobs,
            done: Mutex::new(done),
            generation: Arc::new(AtomicU64::new(0)),
            encoded_bytes: Arc::new(AtomicUsize::new(0)),
            server: Arc::new(Mutex::new((0, None))),
        });
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Pending
        ));
        assert_eq!(
            probe.stream_position().unwrap(),
            before,
            "lookup must leave file I/O to workers"
        );
        assert!(job_queue.try_recv().is_ok());
        drop(bank);
        drop(probe);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn file_read_failure_is_reported_by_the_worker() {
        let bytes =
            assets::encode_sound_bank(b"{}", b"{}", b"{}", &[("sounds/tone".to_owned(), tone())])
                .unwrap();
        let path = std::env::temp_dir().join(format!(
            "cinnabar-worker-read-failure-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = SoundBank::open(&path, None).unwrap().unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(0)
            .unwrap();
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Pending
        ));
        assert!(bank.pcm("sounds/tone", false).is_none());
        assert!(bank.in_flight.is_empty());
        assert_eq!(bank.in_flight_bytes, 0);
        assert_eq!(
            bank.decoder
                .as_ref()
                .unwrap()
                .encoded_bytes
                .load(Ordering::Relaxed),
            0
        );
        drop(bank);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn queued_entries_decode_their_own_samples() {
        let files: Vec<_> = (0..8_i16)
            .map(|value| {
                let mut bytes = tone();
                let start = bytes.len() - 2 * std::mem::size_of::<i16>();
                bytes[start..start + std::mem::size_of::<i16>()]
                    .copy_from_slice(&value.to_le_bytes());
                (format!("sounds/offset{value}"), bytes)
            })
            .collect();
        let path = std::env::temp_dir().join(format!(
            "cinnabar-bank-offsets-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(
            &path,
            assets::encode_sound_bank(b"{}", b"{}", b"{}", &files).unwrap(),
        )
        .unwrap();
        let mut bank = SoundBank::open(&path, None).unwrap().unwrap();
        for (name, _) in &files {
            assert!(matches!(bank.lookup(name, false), PcmLookup::Pending));
        }
        for (value, (name, _)) in files.iter().enumerate() {
            let pcm = bank.pcm(name, false).unwrap();
            assert_eq!(pcm.samples[0], value as i16);
        }
        drop(bank);
        std::fs::remove_file(path).unwrap();
    }

    // A first play must not read its compressed file on the frame thread that asked for it.
    #[test]
    fn a_first_play_reads_its_file_on_a_decode_worker() {
        const LARGE: usize = 4 * 1024 * 1024;
        let mut large = tone();
        large.resize(LARGE, 0);
        let files = [
            ("sounds/warm".to_owned(), tone()),
            ("sounds/large".to_owned(), large),
        ];
        let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &files).expect("encode");
        let path = std::env::temp_dir().join(format!(
            "cinnabar-bank-caller-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write");
        let mut bank = SoundBank::open(&path, None)
            .expect("open")
            .expect("present");
        assert!(bank.pcm("sounds/warm", false).is_some(), "workers started");
        let before = crate::test_allocations::bytes();
        let lookup = bank.lookup("sounds/large", false);
        let copied = crate::test_allocations::bytes() - before;
        assert!(matches!(lookup, PcmLookup::Pending));
        assert!(
            copied < (LARGE / 16) as u64,
            "the caller allocated {copied} bytes for a {LARGE}-byte file"
        );
        bank.pcm("sounds/large", false);
        assert!(!bank.is_decoding("sounds/large"), "a worker finished it");
        drop(bank);
        let _ = std::fs::remove_file(&path);
    }

    // Distinct first plays must not queue every compressed file at once.
    #[test]
    fn queued_decodes_are_bounded_until_polled() {
        let files: Vec<_> = (0..MAX_QUEUED_DECODES + 8)
            .map(|index| (format!("sounds/tone{index}"), tone()))
            .collect();
        let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &files).expect("encode");
        let path = std::env::temp_dir().join(format!(
            "cinnabar-bank-backlog-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write");
        let mut bank = SoundBank::open(&path, None)
            .expect("open")
            .expect("present");
        let busy = files
            .iter()
            .filter(|(name, _)| matches!(bank.lookup(name, false), PcmLookup::Busy))
            .count();
        assert_eq!(busy, 8);
        assert!(
            bank.pcm("sounds/tone0", false).is_some(),
            "the backlog drains"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_reads_an_encoded_bank_and_decodes_a_pcm_file() {
        // One PCM16 mono FSB5 of two frames at 48 kHz.
        let mut fsb = b"FSB5".to_vec();
        let mode = (9_u64 << 1) | (2_u64 << 34);
        for value in [1_u32, 1, 8, 0, 4, 2, 0, 0] {
            fsb.extend(value.to_le_bytes());
        }
        fsb.resize(60, 0);
        fsb.extend(mode.to_le_bytes());
        fsb.extend([0, 0x40, 0, 0xc0]);
        let bytes = assets::encode_sound_bank(
            br#"{}"#,
            b"{}",
            b"{}",
            &[("sounds/test/tone".to_owned(), fsb)],
        )
        .expect("encode");
        let path =
            std::env::temp_dir().join(format!("cinnabar-bank-{}.mcbesnd", std::process::id()));
        std::fs::write(&path, bytes).expect("write");
        let mut bank = SoundBank::open(&path, None)
            .expect("open")
            .expect("present");
        let pcm = bank.pcm("sounds/test/tone", false).expect("decode");
        assert_eq!((pcm.channels, pcm.rate, pcm.frames()), (1, 48_000, 2));
        assert!(bank.pcm("sounds/test/missing", false).is_none());
        assert_eq!(bank.file_count(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
