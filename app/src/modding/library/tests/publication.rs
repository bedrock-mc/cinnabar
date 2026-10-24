use super::*;

/// Denies linkat only on this disposable OS thread, as Android denies app hard links.
fn deny_hard_links() {
    let mut filter = [
        libc::sock_filter {
            code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
            jt: 0,
            jf: 0,
            k: 0,
        },
        libc::sock_filter {
            code: (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16,
            jt: 0,
            jf: 1,
            k: libc::SYS_linkat as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ERRNO | libc::EACCES as u32,
        },
        libc::sock_filter {
            code: (libc::BPF_RET | libc::BPF_K) as u16,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ALLOW,
        },
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: both operations affect only the test's dedicated thread. The filter is valid
    // for the synchronous call, and no TSYNC flag can apply it to other test threads.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), 0);
        assert_eq!(
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &program as *const libc::sock_fprog
            ),
            0
        );
    }
}

#[test]
fn publication_succeeds_when_the_os_denies_hard_links() {
    std::thread::spawn(|| {
        let directory = tempfile::tempdir().unwrap();
        let mods = directory.path().join("mods");
        let source = directory.path().join("mod.wasm");
        fs::write(&source, b"complete mod").unwrap();
        deny_hard_links();
        assert_eq!(
            fs::hard_link(&source, directory.path().join("probe"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        publish_component(&mods, "mod.wasm", b"complete mod").unwrap();
        assert_eq!(fs::read(mods.join("mod.wasm")).unwrap(), b"complete mod");
        assert!(publish_component(&mods, "mod.wasm", b"replacement").is_err());
        assert_eq!(fs::read(mods.join("mod.wasm")).unwrap(), b"complete mod");
        assert_eq!(fs::read_dir(mods).unwrap().count(), 1);
    })
    .join()
    .unwrap();
}
