//! Marketplace declarations must match the exact signed request before any guest is instantiated.

/// Compares requested capabilities and an optional declared version against signed release metadata.
pub(super) fn declarations(
    bytes: &[u8],
    manifest: &launcher_host::marketplace::Manifest,
) -> Result<(), String> {
    let declarations = mod_host::declarations_snapshot(bytes).map_err(|error| error.to_string())?;
    let actual = declarations
        .requested
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let signed: mod_host::ModGrants = serde_json::from_value(manifest.grants.clone())
        .map_err(|error| format!("Unsupported signed mod permissions: {error}"))?;
    if actual != signed {
        return Err("The component's permission request differs from its signed release.".into());
    }
    if declarations
        .metadata
        .version
        .as_ref()
        .is_some_and(|version| version != &manifest.version)
    {
        return Err("The component's version differs from its signed release.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds original binary declarations without executing a component.
    fn component(grants: &str) -> Vec<u8> {
        let name = b"cinnabar:grants";
        let mut bytes = b"\0asm\x0d\0\x01\0".to_vec();
        bytes.extend([0, (1 + name.len() + grants.len()) as u8, name.len() as u8]);
        bytes.extend(name);
        bytes.extend(grants.as_bytes());
        bytes
    }

    #[test]
    fn signed_command_values_and_unknown_permissions_are_checked_before_admission() {
        let mut manifest = launcher_host::marketplace::Manifest {
            slug: "fixture-mod".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            bytes: 1,
            grants: serde_json::json!({"commands":["help"]}),
            approved_at: 1,
            client_revision: "a".repeat(40),
            policy: "fixture".into(),
            publication: "manual".into(),
        };
        assert!(declarations(&component(r#"{"commands":["help"]}"#), &manifest).is_ok());
        assert!(declarations(&component(r#"{"commands":["op"]}"#), &manifest).is_err());
        manifest.grants = serde_json::json!({"future_capability":true});
        assert!(declarations(&component("{}"), &manifest).is_err());
    }
}
