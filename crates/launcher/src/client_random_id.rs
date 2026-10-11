//! Persistence of the user namespace's opaque login ID.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

use protocol::ClientRandomId;

const FILE_NAME: &str = "clientId.txt";
const MAX_STORED_BYTES: u64 = 64;

/// Reuses a nonzero signed ID or saves a new full-width OS-random value before returning it.
pub(crate) fn load(root: &Path) -> io::Result<ClientRandomId> {
    load_with_entropy(root, |bytes| {
        getrandom::fill(bytes).map_err(|error| io::Error::other(error.to_string()))
    })
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "login client ID `{}`: {error}",
                root.join(FILE_NAME).display()
            ),
        )
    })
}

/// Initializes missing, zero or corrupt storage; I/O and entropy failures remain errors.
fn load_with_entropy(
    root: &Path,
    mut fill: impl FnMut(&mut [u8; 8]) -> io::Result<()>,
) -> io::Result<ClientRandomId> {
    let path = root.join(FILE_NAME);
    match File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_STORED_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 <= MAX_STORED_BYTES
                && let Ok(text) = std::str::from_utf8(&bytes)
                && let Ok(value) = text.trim().parse::<i64>()
                && let Some(id) = ClientRandomId::new(value)
            {
                return Ok(id);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let id = loop {
        let mut bytes = [0; 8];
        fill(&mut bytes)?;
        if let Some(id) = ClientRandomId::new(i64::from_le_bytes(bytes)) {
            break id;
        }
    };
    fs::create_dir_all(root)?;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    writeln!(temporary, "{}", id.get())?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path).map_err(|error| error.error)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_layout::{InstallEnvironment, InstallLayout, Platform};

    /// Resolves a fresh launcher owner with the same user storage namespace.
    fn layout(root: &Path) -> InstallLayout {
        InstallLayout::resolve(
            Platform::Linux,
            &InstallEnvironment {
                executable: "/opt/cinnabar/bin/bedrock-client".into(),
                user_root: Some(root.to_owned()),
                home: None,
                local_app_data: None,
                xdg_config_home: None,
                xdg_data_home: None,
                xdg_runtime_dir: None,
            },
        )
        .unwrap()
    }

    /// Returns deterministic entropy with exactly the supplied signed bits.
    fn entropy(value: i64) -> impl FnMut(&mut [u8; 8]) -> io::Result<()> {
        move |bytes| {
            *bytes = value.to_le_bytes();
            Ok(())
        }
    }

    #[test]
    fn storage_survives_joins_and_reopened_launcher_owners() {
        let directory = tempfile::tempdir().unwrap();
        let first = layout(directory.path());
        let id = first.load_client_random_id().unwrap();
        assert_eq!(first.load_client_random_id().unwrap(), id);
        assert_eq!(
            layout(directory.path()).load_client_random_id().unwrap(),
            id
        );
        assert_ne!(id.get(), 0);
    }

    #[test]
    fn independent_user_roots_keep_their_own_ids() {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a");
        let b = directory.path().join("b");
        assert_eq!(load_with_entropy(&a, entropy(-11)).unwrap().get(), -11);
        assert_eq!(load_with_entropy(&b, entropy(22)).unwrap().get(), 22);
        assert_eq!(load_with_entropy(&a, entropy(33)).unwrap().get(), -11);
        assert_eq!(load_with_entropy(&b, entropy(33)).unwrap().get(), 22);
    }

    #[test]
    fn high_bit_entropy_and_signed_endpoints_persist_without_masking() {
        let directory = tempfile::tempdir().unwrap();
        for value in [i64::MIN, i64::MAX, -1, 1] {
            let root = directory.path().join(value.to_string());
            let id = load_with_entropy(&root, entropy(value)).unwrap();
            assert_eq!(id.get(), value);
            assert_eq!(
                load_with_entropy(&root, |_| panic!("stored ID needs no entropy")).unwrap(),
                id
            );
            assert_eq!(
                fs::read_to_string(root.join(FILE_NAME)).unwrap().trim(),
                value.to_string()
            );
        }
    }

    #[test]
    fn corrupt_and_zero_storage_are_replaced_and_random_zero_is_retried() {
        let directory = tempfile::tempdir().unwrap();
        for stored in [
            "0",
            "garbage",
            "-4tail",
            "9223372036854775808",
            "",
            &"1".repeat(65),
        ] {
            fs::write(directory.path().join(FILE_NAME), stored).unwrap();
            let mut values = [0_i64, -19].into_iter();
            let id = load_with_entropy(directory.path(), |bytes| {
                *bytes = values.next().expect("only zero is retried").to_le_bytes();
                Ok(())
            })
            .unwrap();
            assert_eq!(id.get(), -19);
            assert_eq!(load(directory.path()).unwrap(), id);
        }
    }

    #[test]
    fn unreadable_storage_entropy_and_write_failures_return_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        fs::create_dir(&path).unwrap();
        assert!(load_with_entropy(directory.path(), entropy(7)).is_err());
        fs::remove_dir(&path).unwrap();
        assert!(
            load_with_entropy(directory.path(), |_| Err(io::Error::other(
                "entropy unavailable"
            )))
            .is_err()
        );
        assert!(!path.exists());
        fs::write(&path, "0").unwrap();
        let result = load_with_entropy(directory.path(), |bytes| {
            // Force the actual atomic replacement to fail after a successful storage read.
            fs::remove_file(&path)?;
            fs::create_dir(&path)?;
            *bytes = (-7_i64).to_le_bytes();
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            1,
            "temporary file is removed"
        );
        let blocked_root = directory.path().join("not-a-directory");
        fs::write(&blocked_root, "file").unwrap();
        assert!(load_with_entropy(&blocked_root, entropy(7)).is_err());
    }
}
