//! Priority belongs to the hardware callback thread, including all sources it pulls.

use rodio::Source;
#[cfg(any(windows, test))]
use std::thread::ThreadId;
use std::time::Duration;

pub(super) struct CallbackSource<S> {
    inner: S,
    #[cfg(any(windows, test))]
    submitted_on: ThreadId,
}

impl<S> CallbackSource<S> {
    /// Wraps hardware output without changing the submitting thread's priority.
    pub(super) fn new(inner: S) -> Self {
        Self {
            inner,
            #[cfg(any(windows, test))]
            submitted_on: std::thread::current().id(),
        }
    }
}

impl<S: Source<Item = f32>> Iterator for CallbackSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        #[cfg(any(windows, test))]
        ensure_callback_priority(self.submitted_on);
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source<Item = f32>> Source for CallbackSource<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)
    }
}

#[cfg(any(windows, test))]
thread_local! {
    static REGISTRATION: std::cell::OnceCell<platform::Registration> = const { std::cell::OnceCell::new() };
}

#[cfg(test)]
thread_local! {
    static ATTEMPTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Attempts priority once per pulling thread, skipping rodio's submission-time resampling.
#[cfg(any(windows, test))]
fn ensure_callback_priority(submitted_on: ThreadId) {
    REGISTRATION.with(|registration| {
        if registration.get().is_none() && std::thread::current().id() != submitted_on {
            registration.get_or_init(|| {
                #[cfg(test)]
                ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
                platform::register()
            });
        }
    });
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::{
        Foundation::{GetLastError, HANDLE},
        System::Threading::{
            AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, GetCurrentThread,
            SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
        },
    };

    pub(super) struct Registration(HANDLE);

    /// Keeps MMCSS active until this callback thread exits; falls back to a real pseudo-handle.
    pub(super) fn register() -> Registration {
        let mut task_index = 0;
        // The task name is static and terminated; the index is initialized for its first use.
        let handle = unsafe {
            AvSetMmThreadCharacteristicsW(windows_sys::core::w!("Pro Audio"), &mut task_index)
        };
        if handle.is_null() {
            let mmcss_error = unsafe { GetLastError() };
            let fallback =
                unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };
            let fallback_error = if fallback == 0 {
                unsafe { GetLastError() }
            } else {
                0
            };
            eprintln!(
                "audio MMCSS registration failed ({mmcss_error}); thread priority fallback error: {fallback_error}"
            );
        }
        Registration(handle)
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // Thread-local destruction runs on the thread that registered this task.
                unsafe { AvRevertMmThreadCharacteristics(self.0) };
            }
        }
    }
}

#[cfg(all(test, not(windows)))]
mod platform {
    pub(super) type Registration = ();

    /// Exercises the callback registration boundary without changing OS scheduling.
    pub(super) fn register() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_submission_stays_unboosted_and_callback_registers_once() {
        std::thread::spawn(|| {
            let (controller, mixer) =
                rodio::dynamic_mixer::mixer::<f32>(2, super::super::MIXER_RATE);
            for _ in 0..2 {
                let source = rodio::buffer::SamplesBuffer::new(1, 8000, vec![0.25; 64]);
                controller.add(CallbackSource::new(source));
            }
            assert!(REGISTRATION.with(|registration| registration.get().is_none()));
            std::thread::spawn(move || {
                assert!(REGISTRATION.with(|registration| registration.get().is_none()));
                let mut mixer = mixer;
                assert_eq!(mixer.next(), Some(0.5));
                for _ in 0..32 {
                    assert_eq!(mixer.next(), Some(0.5));
                }
                assert!(REGISTRATION.with(|registration| registration.get().is_some()));
                assert_eq!(ATTEMPTS.with(std::cell::Cell::get), 1);
                for _ in 0..16 {
                    assert_eq!(mixer.next(), Some(0.5));
                }
                assert_eq!(ATTEMPTS.with(std::cell::Cell::get), 1);
            })
            .join()
            .unwrap();
        })
        .join()
        .unwrap();
    }
}
