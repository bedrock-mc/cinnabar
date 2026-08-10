use super::*;

impl MenuRuntime {
    fn start_catalog(&mut self) {
        if self.catalog_started || !self.visible || self.connecting {
            return;
        }
        if self.should_auto_start_sign_in(auth_cache_path(&self.layout).is_some()) {
            self.start_sign_in();
            return;
        }
        if self.auth_attempted && self.auth_process.is_none() {
            return;
        }
        if matches!(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Checking | AuthState::AwaitingCode { .. })
        ) || matches!(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Failed(_) | AuthState::SignedOut)
        ) {
            return;
        }
        self.catalog_started = true;
        let _ = fs::remove_file(&self.catalog_path);
        let Some(auth_cache) = auth_cache_path(&self.layout) else {
            self.catalog_message =
                Some("Sign in to load Realms, Friends, and featured servers.".to_owned());
            return;
        };
        let Some(executable) = core_executable(&self.layout) else {
            self.catalog_message = Some(
                "bedrock-core executable was not found; server catalog unavailable.".to_owned(),
            );
            return;
        };
        let mut command = Command::new(executable);
        command
            .arg("-catalog-file")
            .arg(&self.catalog_path)
            .arg("-auth-cache")
            .arg(auth_cache)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(child) => self.catalog_process = Some(child),
            Err(error) => {
                self.catalog_message = Some(format!("Could not start account catalog: {error}"));
            }
        }
    }

    pub(super) fn poll_catalog(&mut self) {
        self.poll_sign_in();
        self.start_catalog();
        let Some(child) = self.catalog_process.as_mut() else {
            return;
        };
        if let Ok(bytes) = fs::read(&self.catalog_path) {
            match serde_json::from_slice::<CatalogFile>(&bytes) {
                Ok(catalog) => {
                    self.featured = catalog.featured;
                    self.gatherings = catalog.gatherings;
                    self.realms = catalog.realms;
                    self.friends = catalog.friends.into_iter().map(Into::into).collect();
                    self.catalog_message = catalog.errors.first().cloned();
                    let _ = child.wait();
                    self.catalog_process = None;
                    let _ = fs::remove_file(&self.catalog_path);
                }
                Err(error) => {
                    self.catalog_message = Some(format!("Could not read account catalog: {error}"))
                }
            }
            return;
        }
        if let Ok(Some(status)) = child.try_wait() {
            self.catalog_process = None;
            if !status.success() {
                self.catalog_message = Some("The account catalog could not be loaded.".to_owned());
            }
        }
    }

    pub(super) fn stop_catalog(&mut self) {
        if let Some(mut child) = self.catalog_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub(super) fn start_sign_in(&mut self) {
        self.auth_attempted = true;
        self.stop_catalog();
        self.catalog_started = false;
        self.catalog_message = None;
        if let Some(process) = self.auth_process.as_mut()
            && !process.cleanup_handed_off()
        {
            process.request_cancel();
            self.auth_restart_requested = true;
            return;
        }
        self.auth_process = None;
        self.auth_restart_requested = false;
        self.spawn_sign_in();
    }

    fn spawn_sign_in(&mut self) {
        let Some(executable) = core_executable(&self.layout) else {
            self.auth_process = None;
            self.message =
                Some("bedrock-core executable was not found; sign-in unavailable.".to_owned());
            return;
        };
        match AuthSupervisor::spawn(&executable, &self.layout.auth_cache()) {
            Ok(process) => self.auth_process = Some(process),
            Err(error) => {
                self.auth_process = None;
                self.message = Some(session_failure_message(&error.to_string()));
            }
        }
    }

    fn poll_sign_in(&mut self) {
        let Some(process) = self.auth_process.as_mut() else {
            return;
        };
        let was_authenticated = matches!(process.state(), AuthState::Authenticated);
        process.poll();
        if process.cleanup_handed_off() && self.auth_restart_requested {
            self.auth_process = None;
            self.auth_restart_requested = false;
            self.spawn_sign_in();
            return;
        }
        if !was_authenticated && matches!(process.state(), AuthState::Authenticated) {
            self.catalog_started = false;
            self.catalog_message = Some("Signed in. Loading account destinations…".to_owned());
        }
    }

    pub(super) fn stop_sign_in(&mut self) {
        // Cancellation is sticky. Only the explicit StartSignIn action may
        // create another helper after this point.
        self.auth_attempted = true;
        self.auth_restart_requested = false;
        if let Some(mut process) = self.auth_process.take() {
            process.request_cancel();
            self.auth_process = Some(process);
        }
    }

    fn should_auto_start_sign_in(&self, cache_exists: bool) -> bool {
        cache_exists && !self.auth_attempted && self.auth_process.is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn cached_validation_runs_once_but_cancel_is_sticky() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        assert!(menu.should_auto_start_sign_in(true));
        menu.stop_sign_in();
        assert!(!menu.should_auto_start_sign_in(true));
        assert!(!menu.should_auto_start_sign_in(false));
    }

    #[test]
    fn failed_spawn_is_not_retried_by_frame_polling() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.auth_attempted = true;
        menu.auth_process = None;
        menu.message = Some("Could not start sign-in helper.".to_owned());

        for _ in 0..100 {
            assert!(!menu.should_auto_start_sign_in(true));
            assert!(menu.auth_attempted && menu.auth_process.is_none());
        }
        assert_eq!(
            menu.message.as_deref(),
            Some("Could not start sign-in helper.")
        );
    }

    #[test]
    fn active_cancel_is_sticky_and_does_not_block_the_menu_frame() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command.stdout(Stdio::piped()).spawn().unwrap();
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

        let started = Instant::now();
        menu.stop_sign_in();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(!menu.auth_restart_requested);
        assert!(!menu.should_auto_start_sign_in(true));
        assert!(matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::SignedOut)
        ));

        for _ in 0..8 {
            assert!(!menu.auth_restart_requested);
            assert!(!menu.should_auto_start_sign_in(true));
        }

        menu.start_sign_in();
        assert!(menu.auth_restart_requested);
        assert!(menu.auth_attempted);
    }
}
