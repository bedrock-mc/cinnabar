//! Absolute monotonic sleeps that wake as close to a deadline as each OS allows.
//!
//! macOS waits on `mach_wait_until`, Linux on `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)`,
//! and Windows on a high-resolution waitable timer, falling back to `std::thread::sleep`. The
//! caller spins the last stretch, so a late wake costs latency only, never correctness.

use std::time::Instant;

/// Sleeps until about `deadline`; returns early only on spurious OS wakes.
pub(super) fn sleep_until(deadline: Instant) {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return;
    };
    if remaining.is_zero() {
        return;
    }
    os::sleep_for(remaining);
}

#[cfg(target_os = "macos")]
mod os {
    use std::{sync::OnceLock, time::Duration};

    #[repr(C)]
    struct TimebaseInfo {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
        fn mach_wait_until(deadline: u64) -> i32;
    }

    /// Nanoseconds per tick as `numer / denom`; Apple silicon is 125/3.
    fn timebase() -> (u128, u128) {
        static TIMEBASE: OnceLock<(u128, u128)> = OnceLock::new();
        *TIMEBASE.get_or_init(|| {
            let mut info = TimebaseInfo { numer: 0, denom: 0 };
            // SAFETY: `info` is a valid, writable timebase record.
            let status = unsafe { mach_timebase_info(&raw mut info) };
            if status != 0 || info.numer == 0 || info.denom == 0 {
                (1, 1)
            } else {
                (u128::from(info.numer), u128::from(info.denom))
            }
        })
    }

    pub(super) fn sleep_for(remaining: Duration) {
        let (numer, denom) = timebase();
        let ticks = u64::try_from(remaining.as_nanos() * denom / numer).unwrap_or(u64::MAX);
        // SAFETY: both calls only read the monotonic clock and block the calling thread.
        unsafe {
            let deadline = mach_absolute_time().saturating_add(ticks);
            mach_wait_until(deadline);
        }
    }
}

#[cfg(target_os = "linux")]
mod os {
    use std::time::Duration;

    pub(super) fn sleep_for(remaining: Duration) {
        // SAFETY: an all-zero timespec is valid on every libc target.
        let mut now: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: `now` is a valid, writable timespec.
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut now) } != 0 {
            return std::thread::sleep(remaining);
        }
        let nanos = i128::from(now.tv_nsec) + remaining.as_nanos() as i128;
        let mut deadline = now;
        deadline.tv_sec = now
            .tv_sec
            .saturating_add((nanos / 1_000_000_000) as libc::time_t);
        deadline.tv_nsec = (nanos % 1_000_000_000) as libc::c_long;
        loop {
            // SAFETY: `deadline` is a valid absolute CLOCK_MONOTONIC time and no remainder is
            // requested, so an interrupted sleep simply retries against the same deadline.
            let status = unsafe {
                libc::clock_nanosleep(
                    libc::CLOCK_MONOTONIC,
                    libc::TIMER_ABSTIME,
                    &raw const deadline,
                    std::ptr::null_mut(),
                )
            };
            if status != libc::EINTR {
                return;
            }
        }
    }
}

#[cfg(windows)]
mod os {
    use std::{ptr, time::Duration};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE,
            SetWaitableTimer, TIMER_ALL_ACCESS, WaitForSingleObject,
        },
    };

    /// One high-resolution timer per waiting thread, closed when the thread exits.
    struct Timer(HANDLE);

    impl Drop for Timer {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CreateWaitableTimerExW` and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    thread_local! {
        static TIMER: Option<Timer> = {
            // SAFETY: no name or security attributes; the flags request a relative-capable
            // high-resolution timer, which older Windows versions reject with a null handle.
            let handle = unsafe {
                CreateWaitableTimerExW(
                    ptr::null(),
                    ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
            (!handle.is_null()).then_some(Timer(handle))
        };
    }

    pub(super) fn sleep_for(remaining: Duration) {
        let waited = TIMER.with(|timer| {
            let Some(timer) = timer else {
                return false;
            };
            // Negative due times are relative, in 100 ns units.
            let due = -i64::try_from(remaining.as_nanos().div_ceil(100)).unwrap_or(i64::MAX);
            // SAFETY: the timer handle is live for this thread, and no completion routine runs.
            unsafe {
                SetWaitableTimer(timer.0, &raw const due, 0, None, ptr::null(), 0) != 0
                    && WaitForSingleObject(timer.0, INFINITE) == WAIT_OBJECT_0
            }
        });
        if !waited {
            std::thread::sleep(remaining);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod os {
    pub(super) fn sleep_for(remaining: std::time::Duration) {
        std::thread::sleep(remaining);
    }
}
