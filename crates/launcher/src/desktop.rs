//! The contract between the desktop launcher and the game process it starts.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Set on a game the launcher supervises; such a game may quit back to the launcher.
pub const SUPERVISED_ENV: &str = "CINNABAR_LAUNCHER";

/// Why the game exited, carried by its process exit status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameExit {
    Quit,
    /// The player chose Return to launcher after a server refused this build as outdated.
    ReturnForUpdate,
}

impl GameExit {
    /// The exit status for this reason; `ReturnForUpdate` is BSD's `EX_TEMPFAIL`.
    pub const fn code(self) -> i32 {
        match self {
            Self::Quit => 0,
            Self::ReturnForUpdate => 75,
        }
    }

    /// The typed reason behind `code`, if it is one; other codes are ordinary failures.
    pub fn from_code(code: i32) -> Option<Self> {
        [Self::Quit, Self::ReturnForUpdate]
            .into_iter()
            .find(|reason| reason.code() == code)
    }
}

/// Saved servers the game joined, newest first, read without the game's repair of a bad file.
pub fn recent_servers(server_file: &Path, limit: usize) -> Vec<crate::menu::view::SavedServer> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = std::fs::File::open(server_file).and_then(|file| {
        file.take(crate::menu::MAX_SAVED_SERVER_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    if read.is_err() || bytes.len() > crate::menu::MAX_SAVED_SERVER_FILE_BYTES {
        return Vec::new();
    }
    let mut servers: Vec<crate::menu::view::SavedServer> =
        serde_json::from_slice(&bytes).unwrap_or_default();
    servers.retain(|server| server.last_joined_unix > 0);
    servers.sort_by_key(|server| std::cmp::Reverse(server.last_joined_unix));
    servers.truncate(limit);
    servers
}

/// The featured servers the game last received from the signed-in account's services, so the
/// launcher can show them without its own account core.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeaturedSnapshot {
    pub servers: Vec<FeaturedEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeaturedEntry {
    pub name: String,
    /// `host:port`, or an experience ID behind [`crate::menu::EXPERIENCE_ADDRESS_PREFIX`].
    pub address: String,
    pub caption: String,
    /// The server's remote HTTPS logo; empty when it has none.
    pub logo_url: String,
    pub player_count: Option<i64>,
    /// The server's current headline, when it publishes one.
    #[serde(default)]
    pub motd: String,
    /// The games it advertises.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The snapshot's file beside the game's downloaded catalog `artwork` directory.
pub fn featured_file(artwork: &Path) -> PathBuf {
    artwork.with_file_name("featured-servers.json")
}

impl FeaturedSnapshot {
    /// Reads a saved snapshot; a missing or unreadable file is empty.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Replaces the saved snapshot atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let directory = path
            .parent()
            .ok_or_else(|| std::io::Error::other("featured snapshot has no directory"))?;
        std::fs::create_dir_all(directory)?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&temporary, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_servers_are_joined_saved_servers_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("servers.json");
        assert!(recent_servers(&path, 8).is_empty());
        let saved = |name: &str, joined: u64| crate::menu::view::SavedServer {
            name: name.into(),
            address: format!("{name}.example.test:19132"),
            favorite: false,
            last_joined_unix: joined,
        };
        std::fs::write(
            &path,
            serde_json::to_vec(&[saved("old", 10), saved("never", 0), saved("new", 20)]).unwrap(),
        )
        .unwrap();
        let names = |limit| {
            recent_servers(&path, limit)
                .into_iter()
                .map(|server| server.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(8), ["new", "old"]);
        assert_eq!(names(1), ["new"]);
        std::fs::write(&path, b"{").unwrap();
        assert!(recent_servers(&path, 8).is_empty());
    }

    #[test]
    fn featured_snapshots_round_trip_and_missing_files_read_empty() {
        let root = tempfile::tempdir().unwrap();
        let path = featured_file(&root.path().join("artwork"));
        assert_eq!(FeaturedSnapshot::load(&path), FeaturedSnapshot::default());
        let snapshot = FeaturedSnapshot {
            servers: vec![FeaturedEntry {
                name: "Example".into(),
                address: "play.example.test:19132".into(),
                player_count: Some(3),
                ..FeaturedEntry::default()
            }],
        };
        snapshot.save(&path).unwrap();
        assert_eq!(FeaturedSnapshot::load(&path), snapshot);
    }

    #[test]
    fn exit_reasons_round_trip_and_crashes_stay_untyped() {
        for reason in [GameExit::Quit, GameExit::ReturnForUpdate] {
            assert_eq!(GameExit::from_code(reason.code()), Some(reason));
        }
        assert_eq!(GameExit::from_code(1), None);
        assert_eq!(GameExit::from_code(101), None);
    }
}
