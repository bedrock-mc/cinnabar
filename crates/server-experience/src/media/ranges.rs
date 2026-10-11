//! Seekable authenticated ranges. Use only from a media worker, never rendering or audio.

use super::{MAX_COMPRESSED_BYTES, descriptor::Descriptor};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeSet, VecDeque},
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Supplies raw bytes of one signed chunk; the caller verifies them.
pub trait ChunkSource {
    fn load(&mut self, index: usize, start: u64, length: u64) -> Result<Vec<u8>>;
}

/// Loads chunk `index` and checks it against the signed index before anyone reads it.
pub fn load_verified(
    descriptor: &Descriptor,
    source: &mut impl ChunkSource,
    index: usize,
) -> Result<Vec<u8>> {
    let expected = descriptor
        .chunk_hashes
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("range outside signed index"))?;
    let start = index as u64 * u64::from(descriptor.chunk_bytes);
    let length = (start + u64::from(descriptor.chunk_bytes)).min(descriptor.bytes) - start;
    let bytes = source.load(index, start, length)?;
    ensure!(
        bytes.len() as u64 == length && crate::crypto::digest(&bytes) == *expected,
        "media range hash mismatch"
    );
    Ok(bytes)
}

/// Fetches chunks over HTTPS from approved origins, charged to a shared download allowance.
pub struct HttpsChunks {
    descriptor: Descriptor,
    origins: BTreeSet<String>,
    runtime: tokio::runtime::Runtime,
    cancelled: Arc<AtomicBool>,
    data_budget: Arc<AtomicU64>,
}

impl HttpsChunks {
    /// Creates the fetcher without starting a request; authority is supplied by the host.
    pub fn new(
        descriptor: Descriptor,
        origins: BTreeSet<String>,
        cancelled: Arc<AtomicBool>,
        data_budget: Arc<AtomicU64>,
    ) -> Result<Self> {
        descriptor.validate(&origins)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        Ok(Self {
            descriptor,
            origins,
            runtime,
            cancelled,
            data_budget,
        })
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
}

impl ChunkSource for HttpsChunks {
    fn load(&mut self, _index: usize, start: u64, length: u64) -> Result<Vec<u8>> {
        ensure!(!self.cancelled.load(Ordering::Acquire), "media cancelled");
        ensure!(
            self.data_budget
                .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| remaining
                    .checked_sub(length))
                .is_ok(),
            "media data allowance exhausted"
        );
        let download = crate::fetch::fetch(
            &self.descriptor.url,
            &self.origins,
            length as usize,
            Some((start, start + length - 1, self.descriptor.bytes)),
        );
        let cancelled = Arc::clone(&self.cancelled);
        self.runtime.block_on(async {
            tokio::select! {
                result = download => result,
                _ = cancellation(cancelled) => anyhow::bail!("media cancelled"),
            }
        })
    }
}

pub struct RangeReader<S> {
    descriptor: Descriptor,
    source: S,
    position: u64,
    chunks: VecDeque<(usize, Vec<u8>)>,
}

impl<S: ChunkSource> RangeReader<S> {
    /// Wraps a chunk source with a small verified cache bounded by the compressed budget.
    pub fn new(descriptor: Descriptor, source: S) -> Self {
        Self {
            descriptor,
            source,
            position: 0,
            chunks: VecDeque::new(),
        }
    }

    fn chunk(&mut self, index: usize) -> Result<&[u8]> {
        if let Some(existing) = self.chunks.iter().position(|(number, _)| *number == index) {
            let chunk = self.chunks.remove(existing).expect("existing chunk");
            self.chunks.push_front(chunk);
        } else {
            let bytes = load_verified(&self.descriptor, &mut self.source, index)?;
            let max_chunks = MAX_COMPRESSED_BYTES / self.descriptor.chunk_bytes as usize;
            while self.chunks.len() >= max_chunks {
                self.chunks.pop_back();
            }
            self.chunks.push_front((index, bytes));
        }
        Ok(&self.chunks.front().expect("chunk loaded").1)
    }
}

impl<S: ChunkSource> Read for RangeReader<S> {
    /// Reads at most the current verified chunk; ordinary Read callers retry as needed.
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.position == self.descriptor.bytes {
            return Ok(0);
        }
        let index = (self.position / u64::from(self.descriptor.chunk_bytes)) as usize;
        let offset = (self.position % u64::from(self.descriptor.chunk_bytes)) as usize;
        let chunk = self.chunk(index).map_err(io::Error::other)?;
        let length = output.len().min(chunk.len() - offset);
        output[..length].copy_from_slice(&chunk[offset..offset + length]);
        self.position += length as u64;
        Ok(length)
    }
}

impl<S> Seek for RangeReader<S> {
    /// Rejects invalid offsets rather than allowing wraparound or sparse allocation.
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::End(delta) => i128::from(self.descriptor.bytes) + i128::from(delta),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
        };
        if next < 0 || next > i128::from(self.descriptor.bytes) {
            return Err(io::Error::other("media seek outside object"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

/// Interrupts a pending request after revocation without a worker join on the main thread.
async fn cancellation(cancelled: Arc<AtomicBool>) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Serves an in-memory object, optionally corrupting one chunk.
    pub(crate) struct MemoryChunks {
        pub bytes: Vec<u8>,
        pub corrupt: Option<usize>,
        pub loads: usize,
    }

    impl ChunkSource for MemoryChunks {
        fn load(&mut self, index: usize, start: u64, length: u64) -> Result<Vec<u8>> {
            self.loads += 1;
            let mut bytes = self.bytes[start as usize..(start + length) as usize].to_vec();
            if self.corrupt == Some(index) {
                bytes[0] ^= 1;
            }
            Ok(bytes)
        }
    }

    /// Indexes `bytes` the way a publisher would, with the smallest legal chunk.
    pub(crate) fn descriptor_for(bytes: &[u8]) -> Descriptor {
        let chunk_bytes = 64 * 1024;
        Descriptor {
            id: "fixture.media".into(),
            timeline: "fixture.timeline".into(),
            profile: super::super::descriptor::Profile::WebmAv1OpusBt709,
            url: "https://example.com/media.webm".into(),
            bytes: bytes.len() as u64,
            chunk_bytes,
            chunk_hashes: bytes
                .chunks(chunk_bytes as usize)
                .map(crate::crypto::digest)
                .collect(),
            sha256: crate::crypto::digest(bytes),
            width: 64,
            height: 64,
            fps: 10,
            duration_us: 500_000,
            audio_channels: 1,
            poster: "poster.png".into(),
        }
    }

    #[test]
    fn reader_serves_verified_bytes_and_refuses_a_tampered_chunk() {
        let bytes: Vec<u8> = (0..150_000u32).map(|i| i as u8).collect();
        let descriptor = descriptor_for(&bytes);
        let mut reader = RangeReader::new(
            descriptor.clone(),
            MemoryChunks {
                bytes: bytes.clone(),
                corrupt: None,
                loads: 0,
            },
        );
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, bytes);
        reader.seek(SeekFrom::Start(10)).unwrap();
        reader.read_exact(&mut [0; 4]).unwrap();
        assert_eq!(reader.source.loads, 3, "cached chunk refetched");
        let mut tampered = RangeReader::new(
            descriptor,
            MemoryChunks {
                bytes,
                corrupt: Some(1),
                loads: 0,
            },
        );
        tampered.seek(SeekFrom::Start(70_000)).unwrap();
        assert!(tampered.read(&mut [0; 4]).is_err());
    }
}
