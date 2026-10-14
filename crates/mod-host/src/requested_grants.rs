//! Optional permission declarations in the component's own bounded metadata section.

use crate::{MAX_COMPONENT_SOURCE_BYTES, ModGrants, local_files};
use anyhow::{Result, ensure};
use std::path::Path;
use wasmparser::{Parser, Payload};

/// The optional top-level component custom section containing a JSON grant request.
pub const REQUESTED_GRANTS_SECTION: &str = "cinnabar:grants";

/// Reads a component's declared requests without instantiating or granting the component.
/// Absence means undeclared, rather than a request for every supported capability.
pub fn requested_grants(path: &Path) -> Result<Option<ModGrants>> {
    let bytes = local_files::read(path, MAX_COMPONENT_SOURCE_BYTES)?;
    // Developer text fixtures do not contain binary custom sections.
    if !bytes.starts_with(b"\0asm") {
        return Ok(None);
    }
    let mut depth = 0usize;
    let mut requested = None;
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) => depth = depth.saturating_sub(1),
            Payload::CustomSection(section)
                if depth == 0 && section.name() == REQUESTED_GRANTS_SECTION =>
            {
                ensure!(requested.is_none(), "duplicate requested grants section");
                ensure!(
                    section.data().len() <= 16 * 1024,
                    "requested grants exceed byte limit"
                );
                let grants: ModGrants = serde_json::from_slice(section.data())?;
                grants.validate()?;
                requested = Some(grants);
            }
            _ => {}
        }
    }
    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Appends a short top-level custom section to an otherwise empty component.
    fn component(json: &str) -> Vec<u8> {
        let mut bytes = wat::parse_str("(component)").unwrap();
        let mut section = vec![REQUESTED_GRANTS_SECTION.len() as u8];
        section.extend_from_slice(REQUESTED_GRANTS_SECTION.as_bytes());
        section.extend_from_slice(json.as_bytes());
        assert!(section.len() < 128);
        bytes.extend_from_slice(&[0, section.len() as u8]);
        bytes.extend(section);
        bytes
    }
    #[test]
    fn requests_are_optional_validated_and_never_become_grants_automatically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mod.wasm");
        std::fs::write(&path, wat::parse_str("(component)").unwrap()).unwrap();
        assert!(requested_grants(&path).unwrap().is_none());
        std::fs::write(
            &path,
            component(r#"{"camera":true,"commands":["ability"]}"#),
        )
        .unwrap();
        let request = requested_grants(&path).unwrap().unwrap();
        assert!(request.camera);
        assert_eq!(request.commands, ["ability"]);
        assert!(!ModGrants::default().camera);
        for invalid in [r#"{"root":true}"#, r#"{"commands":["/ability"]}"#] {
            std::fs::write(&path, component(invalid)).unwrap();
            assert!(requested_grants(&path).is_err());
        }
    }
}
