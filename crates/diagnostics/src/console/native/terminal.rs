//! The launcher's stderr destination after fd 2 becomes the capture pipe.
use std::{fs::File, io::Write};

/// Wraps the duplicated destination. On Windows a console gets UTF-16 writes, as
/// `std::io::stderr` gives it, so non-ASCII text does not depend on the console code page.
pub(super) fn writer(file: File) -> Box<dyn Write + Send> {
    #[cfg(windows)]
    if windows::is_console(&file) {
        return Box::new(windows::ConsoleWriter::new(file));
    }
    Box::new(file)
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{io, os::windows::io::AsRawHandle};
    use windows_sys::Win32::System::Console::{GetConsoleMode, WriteConsoleW};

    /// Whether `file` is a console screen buffer rather than a file or pipe.
    pub(super) fn is_console(file: &File) -> bool {
        let mut mode = 0;
        // SAFETY: The file owns a valid handle, and mode is a valid output pointer.
        unsafe { GetConsoleMode(file.as_raw_handle(), &mut mode) != 0 }
    }

    /// Converts UTF-8 writes to UTF-16 console output, holding back a sequence split across
    /// writes until its remaining bytes arrive.
    pub(super) struct ConsoleWriter {
        file: File,
        partial: Vec<u8>,
    }

    impl ConsoleWriter {
        pub(super) fn new(file: File) -> Self {
            Self {
                file,
                partial: Vec::new(),
            }
        }

        fn write_wide(&self, mut wide: &[u16]) -> io::Result<()> {
            while !wide.is_empty() {
                let mut written = 0u32;
                let count = u32::try_from(wide.len()).unwrap_or(u32::MAX);
                // SAFETY: The buffer holds `count` UTF-16 units, and written is a valid output
                // pointer; the reserved argument must be null.
                let ok = unsafe {
                    WriteConsoleW(
                        self.file.as_raw_handle(),
                        wide.as_ptr(),
                        count,
                        &mut written,
                        std::ptr::null(),
                    )
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                if written == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                wide = &wide[written as usize..];
            }
            Ok(())
        }
    }

    impl Write for ConsoleWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.partial.extend_from_slice(bytes);
            let complete = match std::str::from_utf8(&self.partial) {
                Ok(_) => self.partial.len(),
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                Err(_) => self.partial.len(),
            };
            let tail = self.partial.split_off(complete);
            let wide: Vec<u16> = String::from_utf8_lossy(&self.partial)
                .encode_utf16()
                .collect();
            self.partial = tail;
            self.write_wide(&wide)?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
