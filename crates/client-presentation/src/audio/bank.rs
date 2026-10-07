//! Startup-loaded sound bank: routing tables, definition catalog, on-demand FSB decode and cache.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
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
const MAX_QUEUED_DECODES: usize = 64;
const MAX_QUEUED_DECODE_BYTES: usize = 32 * 1024 * 1024;

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
    file: Option<File>,
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
    /// Paths being decoded, with their stream flag and compressed size.
    in_flight: HashMap<Box<str>, (bool, usize)>,
    in_flight_bytes: usize,
    /// Decoded streamed sounds, held until the next pump's starts take them.
    ready_streams: HashMap<Box<str>, Arc<Pcm>>,
}

/// Where a sound's PCM stands; decoding never runs on the calling (main) thread.
pub enum PcmLookup {
    Ready(Arc<Pcm>),
    Pending,
    /// The decode backlog is full; the caller may retry later.
    Busy,
    Failed,
}

type DecodeJob = (Box<str>, Vec<u8>);
type DecodeResult = (Box<str>, Option<Pcm>);

struct Decoder {
    jobs: Sender<DecodeJob>,
    done: Mutex<Receiver<DecodeResult>>,
}

impl Decoder {
    fn spawn() -> Option<Self> {
        let (jobs, job_queue) = channel::<DecodeJob>();
        let (results, done) = channel();
        let job_queue = Arc::new(Mutex::new(job_queue));
        for index in 0..DECODE_WORKERS {
            let job_queue = Arc::clone(&job_queue);
            let results = results.clone();
            std::thread::Builder::new()
                .name(format!("sound-decode-{index}"))
                .spawn(move || {
                    loop {
                        let job = job_queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv();
                        let Ok((path, bytes)) = job else { return };
                        let pcm = decode_pcm(&bytes, &path);
                        if results.send((path, pcm)).is_err() {
                            return;
                        }
                    }
                })
                .ok()?;
        }
        Some(Self {
            jobs,
            done: Mutex::new(done),
        })
    }
}

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
            file: Some(file),
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
            in_flight: HashMap::new(),
            in_flight_bytes: 0,
            ready_streams: HashMap::new(),
        }))
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
        self.merged = pack
            .as_ref()
            .and_then(|pack| pack.tables.clone())
            .map(|overlay| {
                let mut merged = self.tables.clone();
                merged.merge(overlay);
                merged
            });
        self.server = pack;
    }

    /// Definition by name, the server pack winning over the vanilla catalog.
    pub fn definition(&self, name: &str) -> Option<&AudioDefinition> {
        self.server
            .as_ref()
            .and_then(|pack| pack.definitions.get(name))
            .or_else(|| self.catalog.as_ref()?.lookup(name))
    }

    /// PCM for an alternative's sound path (no extension), queueing a background decode on a
    /// miss; non-streaming sounds are cached once decoded.
    pub fn lookup(&mut self, path: &str, stream: bool) -> PcmLookup {
        if let Some(found) = self.server.as_ref().and_then(|pack| pack.files.get(path)) {
            return PcmLookup::Ready(Arc::clone(found));
        }
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
        let Some(entry) = self.index.entry(path) else {
            self.failed.insert(path.into());
            return PcmLookup::Failed;
        };
        let size = entry.len as usize;
        if !self.in_flight.is_empty()
            && (self.in_flight.len() >= MAX_QUEUED_DECODES
                || self.in_flight_bytes.saturating_add(size) > MAX_QUEUED_DECODE_BYTES)
        {
            return PcmLookup::Busy;
        }
        let bytes = self.read_entry(entry);
        if self.decoder.is_none() {
            self.decoder = Decoder::spawn();
        }
        let queued = bytes
            .zip(self.decoder.as_ref())
            .is_some_and(|(bytes, decoder)| decoder.jobs.send((path.into(), bytes)).is_ok());
        if !queued {
            self.failed.insert(path.into());
            return PcmLookup::Failed;
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
        let Some(decoder) = self.decoder.as_ref() else {
            return;
        };
        let finished: Vec<DecodeResult> = decoder
            .done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_iter()
            .collect();
        for (path, pcm) in finished {
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

    fn read_entry(&mut self, entry: SoundBankEntry) -> Option<Vec<u8>> {
        let file = self.file.as_mut()?;
        let mut bytes = vec![0_u8; entry.len as usize];
        file.seek(SeekFrom::Start(entry.offset)).ok()?;
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    fn remember(&mut self, path: &str, pcm: &Arc<Pcm>) {
        let size = pcm.samples.len() * 2;
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
mod tests {
    use super::*;

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
    fn tone() -> Vec<u8> {
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
