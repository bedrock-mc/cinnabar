use std::process::{Command, Stdio};

use super::*;

pub(super) fn validated_auth_cache(
    layout: &InstallLayout,
    state: Option<&AuthState>,
) -> Option<PathBuf> {
    matches!(state, Some(AuthState::Authenticated)).then(|| layout.auth_cache())
}

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
            && !process.cleanup_complete()
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
        if process.cleanup_complete() && self.auth_restart_requested {
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
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::install_layout::{InstallEnvironment, Platform};
    use crate::menu::core_process::core_command_for_address;

    fn launch_layout_with_spaces() -> InstallLayout {
        InstallLayout::resolve(
            Platform::Linux,
            &InstallEnvironment {
                executable: PathBuf::from("/opt/Cinnabar Client/bin/bedrock-client"),
                home: Some(PathBuf::from("/home/Player One")),
                local_app_data: None,
                xdg_config_home: Some(PathBuf::from("/cfg/Player One")),
                xdg_data_home: Some(PathBuf::from("/data/Player One")),
                xdg_runtime_dir: Some(PathBuf::from("/run/user/1000")),
            },
        )
        .unwrap()
    }

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

    #[test]
    fn only_validated_authentication_selects_the_cache_for_a_connection() {
        let layout = InstallLayout::discover().unwrap();
        assert_eq!(validated_auth_cache(&layout, None), None);
        assert_eq!(
            validated_auth_cache(&layout, Some(&AuthState::SignedOut)),
            None
        );
        assert_eq!(
            validated_auth_cache(&layout, Some(&AuthState::Checking)),
            None
        );
        let failed = AuthState::Failed("validation failed".to_owned());
        assert_eq!(validated_auth_cache(&layout, Some(&failed)), None);
        assert_eq!(
            validated_auth_cache(&layout, Some(&AuthState::Authenticated)),
            Some(layout.auth_cache())
        );
    }

    #[test]
    fn core_child_args_are_offline_unless_authentication_was_validated() {
        let layout = launch_layout_with_spaces();
        let offline = core_command_for_address(
            &layout,
            Path::new("bedrock-core"),
            Path::new("run with spaces"),
            "example.test:19132",
            None,
        );
        let offline_args = offline.get_args().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            offline_args,
            [
                OsString::from("-socket-dir"),
                OsString::from("run with spaces"),
                OsString::from("-upstream"),
                OsString::from("example.test:19132"),
                OsString::from("-resource-pack-cache-dir"),
                layout.resource_pack_cache_dir().into_os_string(),
            ]
        );
        assert_eq!(
            offline_args
                .iter()
                .filter(|arg| *arg == "-resource-pack-cache-dir")
                .count(),
            1
        );
        assert!(
            !offline_args
                .iter()
                .any(|arg| arg == "-resource-pack-cache-quota-bytes")
        );

        let authenticated = core_command_for_address(
            &layout,
            Path::new("bedrock-core"),
            Path::new("run with spaces"),
            "example.test:19132",
            Some(Path::new("validated token.json")),
        );
        let authenticated_args = authenticated
            .get_args()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(
            authenticated_args,
            [
                OsString::from("-socket-dir"),
                OsString::from("run with spaces"),
                OsString::from("-upstream"),
                OsString::from("example.test:19132"),
                OsString::from("-resource-pack-cache-dir"),
                layout.resource_pack_cache_dir().into_os_string(),
                OsString::from("-auth-cache"),
                OsString::from("validated token.json"),
            ]
        );
    }

    #[test]
    fn offline_connect_waits_for_cancelled_sign_in_to_reap() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/Q", "/C", "set /p hold="]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "IFS= read -r hold"]);
            command
        };
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

        menu.request_connect("offline.example:19132".to_owned());
        assert!(menu.take_pending_connect().is_none());
        assert!(matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::SignedOut)
        ));

        let deadline = Instant::now() + Duration::from_secs(5);
        while !menu
            .auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete)
            && Instant::now() < deadline
        {
            menu.poll_sign_in();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            menu.auth_process
                .as_ref()
                .is_some_and(AuthSupervisor::cleanup_complete),
            "cancelled sign-in helper was not reaped"
        );
        let pending = menu.take_pending_connect().expect("offline connection");
        assert_eq!(pending.address, "offline.example:19132");
        assert_eq!(pending.auth_cache, None);
    }

    #[test]
    fn authenticated_connect_waits_for_sign_in_reap_and_keeps_validated_cache() {
        let (child, directory) = event_child_holding(&[
            r#"{"v":1,"event":"checking_cache"}"#,
            r#"{"v":1,"event":"authenticated","method":"cached"}"#,
        ]);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

        let authenticated_deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Authenticated)
        ) && Instant::now() < authenticated_deadline
        {
            menu.poll_sign_in();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Authenticated)
        ));

        menu.request_connect("authenticated.example:19132".to_owned());
        assert!(menu.take_pending_connect().is_none());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !menu
            .auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete)
            && Instant::now() < deadline
        {
            menu.poll_sign_in();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            menu.auth_process
                .as_ref()
                .is_some_and(AuthSupervisor::cleanup_complete),
            "authenticated sign-in helper was not reaped"
        );
        let pending = menu
            .take_pending_connect()
            .expect("authenticated connection");
        assert_eq!(pending.address, "authenticated.example:19132");
        assert_eq!(pending.auth_cache, Some(menu.layout.auth_cache()));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn validation_failure_releases_the_queued_connection_offline() {
        let (child, directory) = event_child_holding(&[
            r#"{"v":1,"event":"checking_cache"}"#,
            r#"{"v":1,"event":"error","stage":"cache","message":"validation failed"}"#,
        ]);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

        let failed_deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Failed(_))
        ) && Instant::now() < failed_deadline
        {
            menu.poll_sign_in();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            menu.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Failed(_))
        ));

        menu.request_connect("offline-after-failure.example:19132".to_owned());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !menu
            .auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete)
            && Instant::now() < deadline
        {
            menu.poll_sign_in();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            menu.auth_process
                .as_ref()
                .is_some_and(AuthSupervisor::cleanup_complete),
            "failed sign-in helper was not reaped"
        );
        let pending = menu.take_pending_connect().expect("offline connection");
        assert_eq!(pending.address, "offline-after-failure.example:19132");
        assert_eq!(pending.auth_cache, None);
        fs::remove_dir_all(directory).unwrap();
    }

    fn event_child_holding(lines: &[&str]) -> (std::process::Child, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "cinnabar-account-auth-helper-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut command = if cfg!(windows) {
            let script = directory.join("events.cmd");
            let body = format!(
                "@echo off\r\n{}\r\nset /p hold=\r\n",
                lines
                    .iter()
                    .map(|line| format!("echo {line}"))
                    .collect::<Vec<_>>()
                    .join("\r\n")
            );
            fs::write(&script, body).unwrap();
            let mut command = Command::new("cmd");
            command.args(["/Q", "/D", "/C"]).arg(script);
            command
        } else {
            let script = directory.join("events.sh");
            let body = format!(
                "#!/bin/sh\n{}\nIFS= read -r hold\n",
                lines
                    .iter()
                    .map(|line| format!("printf '%s\\n' '{line}'"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            fs::write(&script, body).unwrap();
            let mut command = Command::new("sh");
            command.arg(script);
            command
        };
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        (child, directory)
    }
}
