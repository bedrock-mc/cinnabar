//! Installs the exact pinned entity facts beside the runtime carriers.
use super::read_bounded;
use assets::facts::entity::EntityFacts;
use std::{error::Error, path::Path};

/// Validates the selected source before atomically replacing the installed carrier.
pub(super) fn compile(source: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = read_bounded(source)?;
    EntityFacts::validate_pinned(&bytes)?;
    assets::write_blob_atomic(out, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{Command, run};
    use assets::facts::entity::SOURCE_BYTES;

    #[test]
    fn entity_facts_install_requires_a_valid_source_and_distinct_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        let out = dir.path().join("installed.bin");
        std::fs::write(&source, SOURCE_BYTES).unwrap();
        run(Command::EntityFacts {
            source: source.clone(),
            out: out.clone(),
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), SOURCE_BYTES);
        assert!(
            run(Command::EntityFacts {
                source: source.clone(),
                out: source.clone()
            })
            .is_err()
        );
        let mut bytes = SOURCE_BYTES.to_vec();
        bytes[8] ^= 1;
        std::fs::write(&source, bytes).unwrap();
        assert!(
            run(Command::EntityFacts {
                source,
                out: out.clone()
            })
            .is_err()
        );
        assert_eq!(std::fs::read(out).unwrap(), SOURCE_BYTES);
    }
}
