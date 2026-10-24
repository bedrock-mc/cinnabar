//! Natives for the setup activity, which prepares carriers before the game activity starts.
//!
//! `prepare` runs on the activity's worker thread and returns null when the game can start,
//! an empty string when the user quit, or the failure to show. Each call carries its attempt
//! number, which `cancel` names.

use std::{
    cell::RefCell,
    ffi::c_void,
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError, atomic::Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use first_run::Outcome;
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{JObject, JString, JValue},
    sys::{JNI_ERR, JNI_VERSION_1_6, jint, jlong, jstring},
};

use crate::cancellation::Cancellation;

static CANCELLATION: Cancellation = Cancellation::new();
/// A relaunched activity waits for an abandoned attempt to observe its cancellation.
static SETUP: Mutex<()> = Mutex::new(());

/// Registers the setup natives on the class the runtime manifest's application id names.
///
/// # Safety
/// Android passes its live VM while loading this library.
#[unsafe(no_mangle)]
#[expect(
    clippy::print_stderr,
    reason = "the library loads before any logging exists, so stderr is the only sink"
)]
pub unsafe extern "system" fn JNI_OnLoad(vm: *mut jni::sys::JavaVM, _: *mut c_void) -> jint {
    let registered = unsafe { JavaVM::from_raw(vm) }.and_then(|vm| {
        let class = format!(
            "{}/BootstrapActivity",
            super::runtime().application_id.replace('.', "/")
        );
        let mut env = vm.get_env()?;
        env.register_native_methods(
            class,
            &[
                NativeMethod {
                    name: "prepare".into(),
                    sig: "(Ljava/lang/String;Ljava/lang/String;J)Ljava/lang/String;".into(),
                    fn_ptr: prepare as *mut c_void,
                },
                NativeMethod {
                    name: "cancel".into(),
                    sig: "(J)V".into(),
                    fn_ptr: cancel as *mut c_void,
                },
            ],
        )
    });
    match registered {
        Ok(()) => JNI_VERSION_1_6,
        Err(error) => {
            eprintln!("Android setup natives failed to register: {error}");
            JNI_ERR
        }
    }
}

extern "system" fn prepare<'local>(
    mut env: JNIEnv<'local>,
    activity: JObject<'local>,
    files_dir: JString<'local>,
    native_library_dir: JString<'local>,
    attempt: jlong,
) -> jstring {
    let _serial = SETUP.lock().unwrap_or_else(PoisonError::into_inner);
    let outcome = if CANCELLATION.begin(attempt as u64) {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            setup(&mut env, &activity, &files_dir, &native_library_dir)
        }))
        .unwrap_or_else(|_| Err(anyhow!("setup panicked")))
    } else {
        Ok(Outcome::Quit)
    };
    clear_exception(&mut env);
    let reply = match outcome {
        Ok(Outcome::Quit) => String::new(),
        Ok(Outcome::NotNeeded | Outcome::Prepared) => return std::ptr::null_mut(),
        Err(error) => format!("{error:#}"),
    };
    env.new_string(reply)
        .map_or(std::ptr::null_mut(), JString::into_raw)
}

extern "system" fn cancel(_env: JNIEnv<'_>, _activity: JObject<'_>, attempt: jlong) {
    CANCELLATION.cancel(attempt as u64);
}

fn setup(
    env: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
    files_dir: &JString<'_>,
    native_library_dir: &JString<'_>,
) -> Result<Outcome> {
    let files_dir = PathBuf::from(String::from(env.get_string(files_dir)?));
    let native_library_dir = PathBuf::from(String::from(env.get_string(native_library_dir)?));
    let layout = super::layout(&files_dir, &native_library_dir);
    launcher::install_layout::configure_android(layout.clone())?;
    export_platform_trust(env, activity, &layout.user_data_root)?;
    // The kit arrives as APK assets, but Android executes only native-library files.
    let compiler = assets::carriers::kit_compiler(&layout.prep_kit());
    fs::create_dir_all(
        compiler
            .parent()
            .context("kit compiler path has no parent")?,
    )?;
    let _ = fs::remove_file(&compiler);
    symlink(
        native_library_dir.join(&super::runtime().compiler_library),
        &compiler,
    )
    .context("link the asset compiler into the prep kit")?;
    let env = RefCell::new(env);
    let outcome = first_run::prepare_headless(
        &layout.with_prepared_assets(),
        CANCELLATION.flag(),
        |title, body| {
            call_activity(
                &mut env.borrow_mut(),
                activity,
                ("requestConsent", "(Ljava/lang/String;Ljava/lang/String;)Z"),
                &[title, body],
            )
            .unwrap_or(false)
        },
        |line| {
            let _ = call_activity(
                &mut env.borrow_mut(),
                activity,
                ("showProgress", "(Ljava/lang/String;)V"),
                &[line],
            );
        },
    )?;
    // The game activity's UI thread waits for the game thread until its event loop starts, so
    // the carriers load here, behind the progress dialog, to keep that wait short.
    if outcome != Outcome::Quit && !CANCELLATION.flag().load(Ordering::Acquire) {
        cinnabar::lifecycle::preload(outcome == Outcome::Prepared);
    }
    Ok(outcome)
}

/// Refreshes the server's platform trust, refusing setup if stale trust cannot be invalidated.
fn export_platform_trust(env: &mut JNIEnv<'_>, activity: &JObject<'_>, root: &Path) -> Result<()> {
    let pem = platform_trust_pem(env, activity);
    let available = launcher_host::local_worlds::refresh_android_server_trust(
        root,
        pem.as_deref().ok().map(str::as_bytes),
    )
    .context("Cannot refresh or clear local server certificate trust")?;
    if !available {
        log_warning(
            env,
            "Platform trust export failed; local worlds may lose online services",
        );
    }
    Ok(())
}

/// Returns the activity's `platformTrustPem` export, clearing any exception it threw; a thrown,
/// null or empty export is an error.
fn platform_trust_pem(env: &mut JNIEnv<'_>, activity: &JObject<'_>) -> Result<String> {
    let result = env.with_local_frame(4, |env| -> Result<String> {
        let pem = env
            .call_method(activity, "platformTrustPem", "()Ljava/lang/String;", &[])?
            .l()?;
        if pem.is_null() {
            bail!("platform trust export returned null");
        }
        Ok(String::from(env.get_string(&JString::from(pem))?))
    });
    if result.is_err() {
        clear_exception(env);
    }
    let pem = result?;
    if pem.is_empty() {
        bail!("platform trust export is empty");
    }
    Ok(pem)
}

/// Logs `message` at warning priority under the product's tag, discarding any logging failure.
fn log_warning(env: &mut JNIEnv<'_>, message: &str) {
    let logged = env.with_local_frame(4, |env| -> jni::errors::Result<()> {
        let tag = env.new_string(launcher::PRODUCT_NAME)?;
        let message = env.new_string(message)?;
        env.call_static_method(
            "android/util/Log",
            "w",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[JValue::Object(&tag), JValue::Object(&message)],
        )?;
        Ok(())
    });
    if logged.is_err() {
        clear_exception(env);
    }
}

/// Calls an activity method with string arguments in its own local frame; a `Z` method returns
/// its result and any other returns false.
fn call_activity(
    env: &mut JNIEnv<'_>,
    activity: &JObject<'_>,
    (name, signature): (&str, &str),
    args: &[&str],
) -> Result<bool> {
    let result = env.with_local_frame(4, |env| -> Result<bool> {
        let strings = args
            .iter()
            .map(|arg| env.new_string(*arg).map(JObject::from))
            .collect::<jni::errors::Result<Vec<_>>>()?;
        let values: Vec<JValue<'_, '_>> = strings.iter().map(JValue::Object).collect();
        let value = env.call_method(activity, name, signature, &values)?;
        Ok(if signature.ends_with('Z') {
            value.z()?
        } else {
            false
        })
    });
    if result.is_err() {
        clear_exception(env);
    }
    result
}

/// A pending Java exception would make every later JNI call on this thread fail.
fn clear_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}
