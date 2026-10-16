use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Removes only the unique fixture directory successfully created by this test.
struct Scratch(PathBuf);

impl Scratch {
    /// Claims a fresh path without changing the process environment or desktop registrations.
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "cinnabar-invite-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create invite fixture: {error}"),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unchanged_desktop_file_retries_a_failed_association() {
    for failure in [io::ErrorKind::NotFound, io::ErrorKind::Other] {
        let data = Scratch::new();
        let application_id = 42;
        let command = Path::new("/original-fixture/cinnabar");
        let scheme = crate::launch::scheme(application_id);
        let file = format!("{scheme}.desktop");
        let path = data.0.join("applications").join(&file);
        let mut attempts = 0;
        let first = register_at(application_id, command, &data.0, |actual_file, mime| {
            attempts += 1;
            assert_eq!(actual_file, file);
            assert_eq!(mime, format!("x-scheme-handler/{scheme}"));
            Err(io::Error::new(failure, "injected association failure"))
        });
        assert_eq!(first.unwrap_err().kind(), failure);
        let entry = std::fs::read(&path).expect("desktop entry was published before association");
        register_at(application_id, command, &data.0, |actual_file, mime| {
            attempts += 1;
            assert_eq!(actual_file, file);
            assert_eq!(mime, format!("x-scheme-handler/{scheme}"));
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), entry);
        assert_eq!(
            attempts, 2,
            "an unchanged entry must not hide an earlier failure"
        );
    }
}

#[test]
fn desktop_publication_failure_does_not_run_the_association() {
    let data = Scratch::new();
    std::fs::write(data.0.join("applications"), b"blocks directory creation").unwrap();
    let mut ran = false;
    assert!(
        register_at(42, Path::new("/fixture/cinnabar"), &data.0, |_, _| {
            ran = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!ran);
}
