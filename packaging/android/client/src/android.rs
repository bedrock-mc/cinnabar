//! The NativeActivity entry that runs the client, and the setup activity's natives.

mod bootstrap;

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use bevy::android::{ANDROID_APP, android_activity::AndroidApp};
use jni::{
    JavaVM, jni_sig, jni_str,
    objects::{JObject, JString},
};
use launcher::install_layout::{InstallLayout, configure_android};

/// Names shared with `packaging/android/build-apk.sh`.
#[derive(serde::Deserialize)]
struct Runtime {
    application_id: String,
    compiler_library: String,
    local_server_library: String,
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        serde_json::from_str(include_str!("../../runtime.json"))
            .expect("the tracked Android runtime manifest parses")
    })
}

/// The install layout for the app's private files and the APK's native-library directory.
fn layout(files_dir: &Path, native_library_dir: &Path) -> InstallLayout {
    InstallLayout::android(
        files_dir,
        native_library_dir,
        &runtime().local_server_library,
    )
}

#[bevy::prelude::bevy_main]
#[expect(
    clippy::print_stderr,
    reason = "startup can fail before client logging is installed, so stderr is the only sink"
)]
fn main() {
    let code = match run_client() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{} failed: {error:#}", launcher::PRODUCT_NAME);
            if let Err(display_error) = show_startup_failure(&error) {
                eprintln!("Cannot show startup failure: {display_error:#}");
            }
            1
        }
    };
    // Bevy's activity handle and winit's event loop are once per process, so a later launch
    // needs a fresh process; the activity's onDestroy also ends it.
    std::process::exit(code);
}

/// Shows the native error dialog and services activity events until Android closes the app.
fn show_startup_failure(error: &anyhow::Error) -> Result<()> {
    let app = ANDROID_APP
        .get()
        .context("Android activity is unavailable")?;
    // SAFETY: AndroidApp retains its live VM and Activity for this native thread's lifetime.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread_for_scope(|env| -> Result<()> {
        // SAFETY: AndroidApp owns the live Activity reference; this wrapper only borrows it.
        let activity_ptr = app.activity_as_ptr().cast();
        let activity = unsafe { env.as_cast_raw::<JObject>(&activity_ptr)? };
        let message = env.new_string(format!("{error:#}"))?;
        env.call_method(
            &activity,
            jni_str!("startupFailed"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[jni::objects::JValue::Object(&message)],
        )?;
        Ok(())
    })?;
    loop {
        let mut destroyed = false;
        let mut input_available = false;
        app.poll_events(None, |event| {
            use bevy::android::android_activity::{MainEvent, PollEvent};
            match event {
                PollEvent::Main(MainEvent::Destroy) => destroyed = true,
                PollEvent::Main(MainEvent::InputAvailable) => input_available = true,
                _ => {}
            }
        });
        if destroyed {
            return Ok(());
        }
        if input_available {
            let mut events = app.input_events_iter()?;
            while events.next(|_| bevy::android::android_activity::InputStatus::Unhandled) {}
        }
    }
}

fn run_client() -> Result<()> {
    let app = ANDROID_APP
        .get()
        .context("Android activity is unavailable")?;
    let files_dir = app
        .internal_data_path()
        .context("Android reported no private files directory")?;
    configure_android(layout(&files_dir, &initialize_platform(app)?))?;
    cinnabar::startup_feedback::observe(startup_progress, startup_generation);
    if cinnabar::lifecycle::before_run(false)? {
        cinnabar::run(cinnabar::args::ClientArgs {
            force_vsync: true,
            ..Default::default()
        })?;
    }
    Ok(())
}

/// Initializes platform TLS before account/network work and reads this install's native-library root.
fn initialize_platform(app: &AndroidApp) -> Result<PathBuf> {
    // SAFETY: AndroidApp retains its live VM and Activity for this native thread's lifetime.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread_for_scope(|env| -> Result<PathBuf> {
        // SAFETY: The Activity pointer is a live reference owned by AndroidApp, not this wrapper.
        let activity_ptr = app.activity_as_ptr().cast();
        let activity = unsafe { env.as_cast_raw::<JObject>(&activity_ptr)? };
        initialize_tls(env, &activity)?;
        let info = env
            .call_method(
                &activity,
                jni_str!("getApplicationInfo"),
                jni_sig!("()Landroid/content/pm/ApplicationInfo;"),
                &[],
            )?
            .l()?;
        let directory = env
            .get_field(
                &info,
                jni_str!("nativeLibraryDir"),
                jni_sig!("Ljava/lang/String;"),
            )?
            .l()?;
        let directory = env.cast_local::<JString>(directory)?;
        Ok(PathBuf::from(directory.try_to_string(env)?))
    })
}

/// The verifier retains its own global VM/context/class-loader references exactly once.
fn initialize_tls(env: &mut jni::Env<'_>, activity: &JObject<'_>) -> Result<()> {
    let context = env
        .call_method(
            activity,
            jni_str!("getApplicationContext"),
            jni_sig!("()Landroid/content/Context;"),
            &[],
        )?
        .l()?;
    rustls_platform_verifier::android::init_with_env(env, context)
        .context("initialize Android certificate verification")
}

/// Posts work or content readiness without blocking the activity's UI thread.
fn startup_progress(message: launcher::startup::Message, generation: u64) {
    let report = (|| -> Result<()> {
        let app = ANDROID_APP
            .get()
            .context("Android activity is unavailable")?;
        // SAFETY: AndroidApp retains its live VM and Activity for this native thread's lifetime.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        vm.attach_current_thread_for_scope(|env| -> Result<()> {
            // SAFETY: AndroidApp owns the live Activity reference; this wrapper only borrows it.
            let activity_ptr = app.activity_as_ptr().cast();
            let activity = unsafe { env.as_cast_raw::<JObject>(&activity_ptr)? };
            let text = env.new_string(serde_json::to_string(&message)?)?;
            env.call_method(
                &activity,
                jni_str!("startupProgress"),
                jni_sig!("(Ljava/lang/String;J)V"),
                &[
                    jni::objects::JValue::Object(&text),
                    jni::objects::JValue::Long(generation as i64),
                ],
            )?;
            Ok(())
        })
    })();
    if let Err(error) = report {
        bevy::log::warn!("startup feedback unavailable: {error:#}");
    }
}

/// Samples the activity lifetime before validating the next destination frame.
fn startup_generation() -> u64 {
    let read = (|| -> Result<u64> {
        let app = ANDROID_APP
            .get()
            .context("Android activity is unavailable")?;
        // SAFETY: AndroidApp retains its live VM and Activity for this native thread's lifetime.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        vm.attach_current_thread_for_scope(|env| -> Result<u64> {
            // SAFETY: AndroidApp owns the live Activity reference; this wrapper only borrows it.
            let activity_ptr = app.activity_as_ptr().cast();
            let activity = unsafe { env.as_cast_raw::<JObject>(&activity_ptr)? };
            Ok(env
                .call_method(
                    &activity,
                    jni_str!("startupGeneration"),
                    jni_sig!("()J"),
                    &[],
                )?
                .j()? as u64)
        })
    })();
    read.unwrap_or_default()
}
