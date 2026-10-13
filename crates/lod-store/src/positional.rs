//! Positional file reads and writes that leave no shared cursor behind.

use std::{fs::File, io};

/// Fills `buffer` from `file` at `offset`; fails with `UnexpectedEof` when the file ends
/// first.
pub(crate) fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !buffer.is_empty() {
        match read_at(file, buffer, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => {
                buffer = &mut buffer[read..];
                offset += read as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Writes all of `bytes` to `file` at `offset`.
pub(crate) fn write_all_at(file: &File, mut bytes: &[u8], mut offset: u64) -> io::Result<()> {
    #[cfg(test)]
    recorder::record(offset, bytes);
    while !bytes.is_empty() {
        match write_at(file, bytes, offset) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => {
                bytes = &bytes[written..];
                offset += written as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, bytes, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(windows)]
fn write_at(file: &File, bytes: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, bytes, offset)
}

/// The positional writes one test thread makes, in order, so crash tests can replay every
/// prefix of them.
#[cfg(test)]
pub(crate) mod recorder {
    use std::cell::RefCell;

    /// One write: its offset and bytes.
    pub(crate) type Write = (u64, Vec<u8>);

    thread_local! {
        static WRITES: RefCell<Option<Vec<Write>>> = const { RefCell::new(None) };
    }

    /// Starts recording this thread's writes, discarding any earlier record.
    pub(crate) fn start() {
        WRITES.with(|writes| *writes.borrow_mut() = Some(Vec::new()));
    }

    /// Stops recording and returns the writes made since [`start`].
    pub(crate) fn finish() -> Vec<Write> {
        WRITES.with(|writes| writes.borrow_mut().take().unwrap_or_default())
    }

    /// Appends a write when this thread is recording.
    pub(super) fn record(offset: u64, bytes: &[u8]) {
        WRITES.with(|writes| {
            if let Some(writes) = writes.borrow_mut().as_mut() {
                writes.push((offset, bytes.to_vec()));
            }
        });
    }
}
