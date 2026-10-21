//! Host-driven bank ranges over the same admission, decode and cache owners as native audio.

use super::*;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Bounded sound work for a host that obtains bank entries without a filesystem.
pub struct SoundDecodeDriver {
    jobs: Receiver<DecodeJob>,
    results: SyncSender<DecodeResult>,
    generation: Arc<AtomicU64>,
    server: CurrentServer,
    slots: Arc<Semaphore>,
}

/// One admitted decode; its encoded-byte and concurrency permits last until completion.
pub struct SoundDecodeRequest {
    job: DecodeJob,
    results: SyncSender<DecodeResult>,
    generation: Arc<AtomicU64>,
    server: CurrentServer,
    _slot: OwnedSemaphorePermit,
}

impl SoundBank {
    /// Installs a verified prefix and returns the host's bounded range/decode driver.
    /// The host must keep driving it for this bank's lifetime, including server-pack sounds.
    pub fn with_range_source(
        index: SoundBankIndex,
        catalog: Option<Arc<RuntimeAudioCatalog>>,
    ) -> (Self, SoundDecodeDriver) {
        let mut bank = Self::from_index(index, catalog);
        let (jobs, requests) = sync_channel(MAX_QUEUED_DECODES);
        let (results, done) = sync_channel(DECODE_WORKERS);
        let generation = Arc::new(AtomicU64::new(bank.decode_generation));
        let server = Arc::new(Mutex::new((bank.decode_generation, None)));
        bank.decoder = Some(Decoder {
            jobs,
            done: Mutex::new(done),
            generation: Arc::clone(&generation),
            encoded_bytes: Arc::new(AtomicUsize::new(0)),
            server: Arc::clone(&server),
            external: true,
        });
        let driver = SoundDecodeDriver {
            jobs: requests,
            results,
            generation,
            server,
            slots: Arc::new(Semaphore::new(DECODE_WORKERS)),
        };
        (bank, driver)
    }
}

impl SoundDecodeDriver {
    /// Waits for one current request while bounding active reads and decodes like native workers.
    pub async fn next(&mut self) -> Option<SoundDecodeRequest> {
        let slot = Arc::clone(&self.slots).acquire_owned().await.ok()?;
        loop {
            let job = self.jobs.recv().await?;
            if job.0 != self.generation.load(Ordering::Acquire) {
                continue;
            }
            return Some(SoundDecodeRequest {
                job,
                results: self.results.clone(),
                generation: Arc::clone(&self.generation),
                server: Arc::clone(&self.server),
                _slot: slot,
            });
        }
    }
}

impl SoundDecodeRequest {
    /// The exact bank byte range to fetch; server-pack sounds already own their encoded source.
    pub fn range(&self) -> Option<SoundBankEntry> {
        match &self.job.2 {
            DecodeSource::Range(entry) => Some(*entry),
            _ => None,
        }
    }

    /// Whether a pack replacement has already invalidated this request.
    pub fn is_current(&self) -> bool {
        self.job.0 == self.generation.load(Ordering::Acquire)
    }

    /// Decodes on the host's work turn and awaits result capacity without blocking its event loop.
    /// A failed range fetch supplies `None`; range bytes must match the admitted length exactly.
    pub async fn complete(self, bytes: Option<Vec<u8>>) {
        if !self.is_current() {
            return;
        }
        let (epoch, path, source, permit) = self.job;
        let pcm = match source {
            DecodeSource::Range(entry) => bytes
                .filter(|bytes| bytes.len() == entry.len as usize)
                .and_then(|bytes| decode_pcm(&bytes, &path)),
            #[cfg(not(target_arch = "wasm32"))]
            DecodeSource::Bank(entry) => entry.read().and_then(|bytes| decode_pcm(&bytes, &path)),
            DecodeSource::Server => {
                let pack = {
                    let current = self
                        .server
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    (current.0 == epoch).then(|| current.1.clone()).flatten()
                };
                pack.and_then(|pack| pack.decode(&path))
            }
        };
        drop(permit);
        if epoch == self.generation.load(Ordering::Acquire) {
            let _ = self.results.send((epoch, path, pcm)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bank with one original PCM fixture and its exact encoded byte range.
    fn fixture() -> (SoundBank, SoundDecodeDriver, Vec<u8>) {
        let tone = super::super::tests::tone();
        let bytes = assets::sound_bank::encode_sound_bank(
            b"{}",
            b"{}",
            b"{}",
            &[("sounds/tone".into(), tone.clone())],
        )
        .unwrap();
        let prefix_len = assets::sound_bank::sound_bank_prefix_len(&bytes).unwrap();
        let index = SoundBankIndex::decode_prefix(&bytes[..prefix_len]).unwrap();
        let (bank, driver) = SoundBank::with_range_source(index, None);
        (bank, driver, tone)
    }

    #[tokio::test]
    async fn losing_the_range_driver_retires_pending_reads_and_late_results() {
        let (mut bank, mut driver, bytes) = fixture();
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Pending
        ));
        let request = driver.next().await.unwrap();
        drop(driver);
        bank.poll();
        assert!(!request.is_current());
        request.complete(Some(bytes)).await;
        bank.poll();
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Failed
        ));
        assert!(bank.in_flight.is_empty());
        assert_eq!(bank.in_flight_bytes, 0);
    }

    #[tokio::test]
    async fn ranged_decode_uses_the_existing_cache_and_rejects_short_data() {
        for valid in [false, true] {
            let (mut bank, mut driver, mut bytes) = fixture();
            assert!(matches!(
                bank.lookup("sounds/tone", false),
                PcmLookup::Pending
            ));
            let request = driver.next().await.unwrap();
            assert_eq!(request.range().unwrap().len as usize, bytes.len());
            if !valid {
                bytes.pop();
            }
            request.complete(Some(bytes)).await;
            bank.poll();
            match bank.lookup("sounds/tone", false) {
                PcmLookup::Ready(pcm) if valid => {
                    assert_eq!(pcm.channels, 1);
                    assert_eq!(pcm.samples.as_ref(), &[16384, -16384]);
                    assert!(matches!(
                        bank.lookup("sounds/tone", false),
                        PcmLookup::Ready(_)
                    ));
                }
                PcmLookup::Failed if !valid => {}
                _ => panic!("range validation must decide the cached outcome"),
            }
        }
    }

    #[tokio::test]
    async fn replacing_the_pack_rejects_an_old_range_completion() {
        let (mut bank, mut driver, bytes) = fixture();
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Pending
        ));
        let old = driver.next().await.unwrap();
        bank.install_server(Some(Arc::new(ServerSoundPack::default())));
        assert!(!old.is_current());
        old.complete(Some(bytes)).await;
        bank.poll();
        assert!(bank.cache.is_empty());
        assert!(bank.in_flight.is_empty());
        assert!(matches!(
            bank.lookup("sounds/tone", false),
            PcmLookup::Pending
        ));
    }
}
