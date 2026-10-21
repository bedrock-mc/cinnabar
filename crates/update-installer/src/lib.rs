//! Verified local downloads and one offline Rust installer process.

pub mod apply;
pub mod helper;
pub mod lease;
pub mod parent;
mod platform;
pub mod recovery;
pub mod stage;
pub mod startup;
pub mod target;

/// Lease shared by stage writers, apply helpers, and startup recovery.
pub const LOCK_FILE: &str = "update.lock";

/// Keys embedded into the client and helper; the existing list supports key rotation.
pub const TRUSTED_KEYS: &str = match option_env!("UPDATE_TRUSTED_KEYS") {
    Some(keys) => keys,
    None => "",
};

/// The packaged helper's filename on this host.
pub fn helper_filename() -> String {
    update_manifest::runtime::executable("UPDATER", std::env::consts::OS)
}

#[cfg(test)]
mod fixtures;
