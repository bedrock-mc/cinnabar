//! The endpoint file: where a running client listens and the token it accepts.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

/// Written by the client when its control server binds, read by controllers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

impl Endpoint {
    pub fn read(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    /// Writes owner-only, through a rename so readers never see a partial file.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let staging = path.with_extension("tmp");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&staging)?;
        file.write_all(&serde_json::to_vec(self).map_err(io::Error::other)?)?;
        file.sync_all()?;
        fs::rename(&staging, path)
    }
}

/// 244 random bits from the OS generator.
pub fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Compares without an early exit so response timing does not leak a prefix match.
pub fn token_matches(expected: &str, offered: &str) -> bool {
    let (expected, offered) = (expected.as_bytes(), offered.as_bytes());
    let mut difference = u8::from(expected.len() != offered.len());
    for (index, byte) in expected.iter().enumerate() {
        difference |= byte ^ offered.get(index).copied().unwrap_or(!byte);
    }
    difference == 0 && !expected.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_compare_exactly() {
        let token = random_token();
        assert_eq!(token.len(), 64);
        assert_ne!(token, random_token());
        assert!(token_matches(&token, &token));
        assert!(!token_matches(&token, &token[..63]));
        assert!(!token_matches(&token, &format!("{token}0")));
        assert!(!token_matches(&token, ""));
        assert!(!token_matches("", ""));
    }

    #[test]
    fn endpoint_round_trips_owner_only() {
        let dir = std::env::temp_dir().join(format!("cinnabar-endpoint-{}", std::process::id()));
        let path = dir.join("endpoint.json");
        let endpoint = Endpoint {
            port: 4242,
            token: random_token(),
            pid: 7,
        };
        endpoint.write(&path).unwrap();
        assert_eq!(Endpoint::read(&path).unwrap(), endpoint);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
