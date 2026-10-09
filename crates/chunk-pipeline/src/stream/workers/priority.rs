/// Lowers lighting workers relative to interactive frame and network work.
#[cfg(target_os = "macos")]
pub(super) fn lower() -> std::io::Result<()> {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut relative = 0;
    // Keep the inherited QoS class and lower only relative scheduling priority.
    let status =
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status));
    }
    if matches!(class, libc::qos_class_t::QOS_CLASS_UNSPECIFIED) {
        class = libc::qos_class_t::QOS_CLASS_DEFAULT;
    }
    // A negative relative priority lowers this worker without elevating any inherited setting.
    let status = unsafe { libc::pthread_set_qos_class_self_np(class, relative.min(-1)) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status))
    }
}

/// Linux niceness applies to the calling worker's task ID, not the whole process.
#[cfg(target_os = "linux")]
pub(super) fn lower() -> std::io::Result<()> {
    // These calls address the current task; an inherited lower priority stays lower.
    let task = unsafe { libc::gettid() as libc::id_t };
    let current = unsafe { libc::getpriority(libc::PRIO_PROCESS, task) };
    let status = unsafe { libc::setpriority(libc::PRIO_PROCESS, task, current.max(1)) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Windows keeps background lighting below the normal frame/network thread priority.
#[cfg(windows)]
pub(super) fn lower() -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // The pseudo-handle is valid for the current worker and must not be closed.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Other targets retain queue isolation and bounded worker counts without an OS priority hint.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(super) fn lower() -> std::io::Result<()> {
    Ok(())
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, windows))]
pub(super) fn is_lowered() -> bool {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_NORMAL,
    };
    // The pseudo-handle is valid for the current thread and must not be closed.
    unsafe { GetThreadPriority(GetCurrentThread()) < THREAD_PRIORITY_NORMAL }
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, target_os = "linux"))]
pub(super) fn is_lowered() -> bool {
    // These calls address the current task only.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) > 0 }
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, target_os = "macos"))]
pub(super) fn is_lowered() -> bool {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut relative = 0;
    // The output pointers stay valid throughout the call on this thread.
    let status =
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
    status == 0 && relative < 0
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// Reads back the actual priority on an isolated thread so the test runner stays unchanged.
    #[test]
    fn lighting_priority_is_applied_to_the_worker() {
        std::thread::spawn(|| {
            let mut inherited = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut inherited_relative = 0;
            // These outputs remain live on this test worker throughout the call.
            assert_eq!(
                unsafe {
                    libc::pthread_get_qos_class_np(
                        libc::pthread_self(),
                        &mut inherited,
                        &mut inherited_relative,
                    )
                },
                0
            );
            super::lower().unwrap();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // The output pointers stay valid throughout the call on this worker.
            let status = unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
            };
            assert_eq!(status, 0);
            let expected = if matches!(inherited, libc::qos_class_t::QOS_CLASS_UNSPECIFIED) {
                libc::qos_class_t::QOS_CLASS_DEFAULT
            } else {
                inherited
            };
            assert_eq!(class as u32, expected as u32);
            assert!(relative <= inherited_relative);
            assert!(relative < 0);
        })
        .join()
        .unwrap();
    }
}
