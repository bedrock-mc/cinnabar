//! Sticky I/O failures prevent a demuxer from turning a failed range into successful EOF.

use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(super) struct Faults(Arc<Mutex<Option<String>>>);

impl Faults {
    /// Rejects completion if any earlier read or seek failed, even when a parser swallowed it.
    pub(super) fn check(&self) -> anyhow::Result<()> {
        let fault = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("media fault lock poisoned"))?;
        if let Some(message) = fault.as_ref() {
            anyhow::bail!("media reader failed: {message}");
        }
        Ok(())
    }

    /// Records the first failure while preserving the reader's original error result.
    fn observe<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if let Err(error) = &result {
            let mut fault = self
                .0
                .lock()
                .map_err(|_| io::Error::other("media fault lock poisoned"))?;
            if fault.is_none() {
                *fault = Some(error.to_string());
            }
        }
        result
    }
}

pub(super) struct FaultReader<R> {
    reader: R,
    faults: Faults,
}

impl<R> FaultReader<R> {
    /// Returns a reader and an independent fault handle that survives ownership by a demuxer.
    pub(super) fn new(reader: R) -> (Self, Faults) {
        let faults = Faults::default();
        (
            Self {
                reader,
                faults: faults.clone(),
            },
            faults,
        )
    }
}

impl<R: Read> Read for FaultReader<R> {
    /// Preserves all range read failures, including integrity and allowance errors.
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.faults.observe(self.reader.read(bytes))
    }
}

impl<R: Seek> Seek for FaultReader<R> {
    /// Preserves failed seeks as terminal faults rather than treating them as EOF.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.faults.observe(self.reader.seek(position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct BoundaryReader {
        bytes: io::Cursor<Vec<u8>>,
        fail: Arc<AtomicBool>,
    }

    impl Read for BoundaryReader {
        /// Simulates an authenticated first element followed by a failing range fetch.
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if !output.is_empty()
                && self.fail.load(Ordering::Acquire)
                && self.bytes.position() == self.bytes.get_ref().len() as u64
            {
                return Err(io::Error::other("media range hash mismatch"));
            }
            self.bytes.read(output)
        }
    }

    impl Seek for BoundaryReader {
        /// Delegates valid fixture seeks to the in-memory object.
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(position)
        }
    }

    #[test]
    fn swallowed_range_failure_at_element_boundary_is_not_successful_eof() {
        let (mut reader, faults) = FaultReader::new(BoundaryReader {
            bytes: io::Cursor::new(vec![0xec, 0x81, 0]),
            fail: Arc::new(AtomicBool::new(true)),
        });
        reader.read_exact(&mut [0; 3]).unwrap();
        faults.check().unwrap();
        let parser_has_next = reader.read_exact(&mut [0]).is_ok();
        assert!(!parser_has_next);
        assert!(
            faults
                .check()
                .unwrap_err()
                .to_string()
                .contains("hash mismatch")
        );
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut [0; 3]).unwrap();
        assert!(faults.check().is_err());
    }

    #[test]
    fn ordinary_eof_is_not_a_reader_fault() {
        let (mut reader, faults) = FaultReader::new(io::Cursor::new(Vec::<u8>::new()));
        assert_eq!(reader.read(&mut [0]).unwrap(), 0);
        faults.check().unwrap();
    }

    #[cfg(feature = "developer-media")]
    /// Encodes a small EBML element for the independent demuxer I/O fixture.
    fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 127);
        [id, &[0x80 | payload.len() as u8], payload].concat()
    }

    #[cfg(feature = "developer-media")]
    #[test]
    fn pinned_demuxer_swallowing_a_range_fault_cannot_report_successful_end() {
        let header = element(
            &[0x1a, 0x45, 0xdf, 0xa3],
            &[
                element(&[0x42, 0x82], b"webm"),
                element(&[0x42, 0x87], &[1]),
                element(&[0x42, 0x85], &[1]),
            ]
            .concat(),
        );
        let info = element(
            &[0x15, 0x49, 0xa9, 0x66],
            &[element(&[0x4d, 0x80], b"x"), element(&[0x57, 0x41], b"x")].concat(),
        );
        let track = element(
            &[0xae],
            &[
                element(&[0xd7], &[1]),
                element(&[0x73, 0xc5], &[1]),
                element(&[0x83], &[1]),
                element(&[0x86], b"V_AV1"),
            ]
            .concat(),
        );
        let tracks = element(&[0x16, 0x54, 0xae, 0x6b], &track);
        let cluster = element(
            &[0x1f, 0x43, 0xb6, 0x75],
            &[
                element(&[0xe7], &[0]),
                element(&[0xa3], &[0x81, 0, 0, 0x80, 0]),
            ]
            .concat(),
        );
        let segment = element(&[0x18, 0x53, 0x80, 0x67], &[info, tracks, cluster].concat());
        let fail = Arc::new(AtomicBool::new(false));
        let (reader, faults) = FaultReader::new(BoundaryReader {
            bytes: io::Cursor::new([header, segment].concat()),
            fail: Arc::clone(&fail),
        });
        let mut file = matroska_demuxer::MatroskaFile::open(reader).unwrap();
        faults.check().unwrap();
        assert!(
            file.next_frame(&mut matroska_demuxer::Frame::default())
                .unwrap()
        );
        fail.store(true, Ordering::Release);
        assert!(
            !file
                .next_frame(&mut matroska_demuxer::Frame::default())
                .unwrap()
        );
        assert!(
            faults
                .check()
                .unwrap_err()
                .to_string()
                .contains("hash mismatch")
        );
    }
}
