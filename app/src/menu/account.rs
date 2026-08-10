use super::*;

impl MenuRuntime {
    fn start_catalog(&mut self) {
        if self.catalog_started || !self.visible || self.connecting {
            return;
        }
        if auth_cache_path().is_some() && self.auth_process.is_none() {
            self.start_sign_in();
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
        let Some(auth_cache) = auth_cache_path() else {
            self.catalog_message =
                Some("Sign in to load Realms, Friends, and featured servers.".to_owned());
            return;
        };
        let Some(executable) = core_executable() else {
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
        self.stop_sign_in();
        self.stop_catalog();
        self.catalog_started = false;
        self.catalog_message = None;
        let Some(executable) = core_executable() else {
            self.auth_process = None;
            self.message =
                Some("bedrock-core executable was not found; sign-in unavailable.".to_owned());
            return;
        };
        match AuthSupervisor::spawn(&executable, &configured_auth_cache_path()) {
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
        if !was_authenticated && matches!(process.state(), AuthState::Authenticated) {
            self.catalog_started = false;
            self.catalog_message = Some("Signed in. Loading account destinations…".to_owned());
        }
    }

    pub(super) fn stop_sign_in(&mut self) {
        if let Some(mut process) = self.auth_process.take() {
            process.cancel();
        }
    }
}
