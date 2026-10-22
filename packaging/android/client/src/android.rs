//! The NativeActivity entry that runs the client, and the setup activity's natives.

mod bootstrap;

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use bevy::android::{ANDROID_APP, android_activity::AndroidApp};
use jni::{
    JavaVM,
    objects::{JObject, JString},
};
use launcher::install_layout::{InstallLayout, configure_android};

/// Names shared with `packaging/android/build-apk.sh`.
#[derive(serde::Deserialize)]
struct Runtime {
    application_id: String,
    core_library: String,
    compiler_library: String,
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
    InstallLayout::android(files_dir, native_library_dir.join(&runtime().core_library))
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
            1
        }
    };
    // Bevy's activity handle and winit's event loop are once per process, so a later launch
    // needs a fresh process; the activity's onDestroy also ends it.
    std::process::exit(code);
}

fn run_client() -> Result<()> {
    let app = ANDROID_APP
        .get()
        .context("Android activity is unavailable")?;
    let files_dir = app
        .internal_data_path()
        .context("Android reported no private files directory")?;
    configure_android(layout(&files_dir, &native_library_dir(app)?))?;
    cinnabar::startup_feedback::observe(startup_progress);
    if cinnabar::lifecycle::before_run(false)? {
        cinnabar::run(cinnabar::args::ClientArgs {
            force_vsync: true,
            ..Default::default()
        })?;
    }
    Ok(())
}

/// `ApplicationInfo.nativeLibraryDir`, which changes with every APK install.
fn native_library_dir(app: &AndroidApp) -> Result<PathBuf> {
    // The activity owns both pointers for the life of this native thread.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread_permanently()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let info = env
        .call_method(
            &activity,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            &[],
        )?
        .l()?;
    let directory = JString::from(
        env.get_field(&info, "nativeLibraryDir", "Ljava/lang/String;")?
            .l()?,
    );
    let directory: String = env.get_string(&directory)?.into();
    Ok(PathBuf::from(directory))
}

/// Posts work or content readiness without blocking the activity's UI thread.
fn startup_progress(message: launcher::startup::Message) {
    let report = (|| -> Result<()> {
        let app = ANDROID_APP
            .get()
            .context("Android activity is unavailable")?;
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
        let mut env = vm.attach_current_thread()?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let text = env.new_string(serde_json::to_string(&message)?)?;
        env.call_method(
            &activity,
            "startupProgress",
            "(Ljava/lang/String;)V",
            &[(&text).into()],
        )?;
        Ok(())
    })();
    if let Err(error) = report {
        bevy::log::warn!("startup feedback unavailable: {error:#}");
    }
}
