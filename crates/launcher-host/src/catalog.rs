//! One-shot account catalog process used before a control feed is attached.
use crate::{
    core_process::{auth_cache_path, core_executable},
    lifecycle::children::Spawned,
};
use launcher::{install_layout::InstallLayout, menu::view::CatalogFile};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug)]
pub struct Catalog {
    path: PathBuf,
    child: Option<Spawned>,
}

impl Catalog {
    /// Owns the catalog file and any helper launched to fill it.
    pub fn new(path: PathBuf) -> Self {
        Self { path, child: None }
    }
    /// Returns whether a helper is still expected to publish its catalog.
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }
    /// Returns the image cache alongside this catalog file.
    pub fn artwork_dir(&self) -> PathBuf {
        self.path.with_file_name("catalog-images")
    }
    /// Starts a one-shot catalog request using the install's cached sign-in.
    pub fn start(&mut self, layout: &InstallLayout) -> Result<(), String> {
        let _ = fs::remove_file(&self.path);
        let Some(auth_cache) = auth_cache_path(layout) else {
            return Err("Sign in to load Realms, Friends, and featured servers.".to_owned());
        };
        let Some(executable) = core_executable(layout) else {
            return Err(
                "bedrock-core executable was not found; server catalog unavailable.".to_owned(),
            );
        };
        let mut command = Command::new(executable);
        command
            .arg("-catalog-file")
            .arg(&self.path)
            .arg("-auth-cache")
            .arg(auth_cache)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match crate::lifecycle::children::spawn(&mut command) {
            Ok(child) => {
                self.child = Some(child);
                Ok(())
            }
            Err(_) => Err("Reopen Cinnabar to retry the account catalog.".to_owned()),
        }
    }
    /// Reads one published catalog or helper failure without blocking the frame.
    pub fn poll(&mut self) -> Result<Option<CatalogFile>, String> {
        let Some(child) = self.child.as_ref() else {
            return Ok(None);
        };
        if let Ok(bytes) = fs::read(&self.path) {
            return match serde_json::from_slice::<CatalogFile>(&bytes) {
                Ok(catalog) => {
                    if let Some(child) = self.child.take() {
                        reap(child);
                    }
                    let _ = fs::remove_file(&self.path);
                    Ok(Some(catalog))
                }
                Err(_) => Err("Social: Refresh to try again.".to_owned()),
            };
        }
        if let Ok(Some(status)) = child.try_wait() {
            self.child = None;
            if !status.success() {
                return Err("The account catalog could not be loaded.".to_owned());
            }
        }
        Ok(None)
    }
    /// Cancels the helper while retaining its tracked asynchronous reaper.
    pub fn stop(&mut self) {
        if let Some(child) = self.child.take() {
            child.kill();
            reap(child);
        }
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_file(&self.path);
    }
}

/// Waits for an exiting helper on a thread so the frame never blocks on it; it
/// stays tracked, so the exit sweep still covers it.
fn reap(child: crate::lifecycle::children::Spawned) {
    let spawned = std::thread::Builder::new()
        .name("catalog-reaper".to_owned())
        .spawn(move || {
            child.wait();
        });
    if let Err(error) = spawned {
        tracing::warn!("catalog helper left unreaped: {error}");
    }
}
