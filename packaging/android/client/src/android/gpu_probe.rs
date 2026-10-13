//! Android's private service transports the shared Rust probe policy across a process boundary.

use anyhow::{Context, Result};
use jni::{
    JNIEnv, NativeMethod,
    objects::{JClass, JObject, JString, JValue},
    sys::jstring,
};
use launcher::graphics_backend::{PROBE_TIMEOUT, Verdict};
use launcher_host::renderer_startup::probe::Output;

/// Registers the service native without assuming that a NativeActivity exists in this process.
pub(super) fn register(env: &mut JNIEnv<'_>) -> jni::errors::Result<()> {
    env.register_native_methods(
        format!(
            "{}/GpuProbeService",
            super::runtime().application_id.replace('.', "/")
        ),
        &[NativeMethod {
            name: "execute".into(),
            sig: "(Ljava/lang/String;Landroid/view/Surface;)Ljava/lang/String;".into(),
            fn_ptr: execute as *mut std::ffi::c_void,
        }],
    )
}

/// Calls Java only after activity lifecycle processing can continue on the native client thread.
pub(super) fn run(arguments: &[String]) -> Result<Output> {
    let app = bevy::android::ANDROID_APP
        .get()
        .context("Android activity unavailable")?;
    // SAFETY: The activity owns the VM and activity reference throughout the client lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread_permanently()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let args = env.new_string(serde_json::to_string(arguments)?)?;
    // Attached native threads use the system loader for FindClass; ask the activity's loader.
    let loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let class_name = env.new_string(format!("{}.GpuProbe", super::runtime().application_id))?;
    let class = JClass::from(
        env.call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )?
        .l()?,
    );
    let reply = env
        .call_static_method(
            class,
            "run",
            "(Landroid/content/Context;Ljava/lang/String;J)Ljava/lang/String;",
            &[
                JValue::Object(&activity),
                JValue::Object(&args),
                JValue::Long(PROBE_TIMEOUT.as_millis() as i64),
            ],
        )?
        .l()?;
    let reply = JString::from(reply);
    let reply: String = env.get_string(&reply)?.into();
    serde_json::from_str(&reply).context("decode Android GPU probe result")
}

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut jni::sys::JNIEnv,
        surface: jni::sys::jobject,
    ) -> *mut std::ffi::c_void;
    fn ANativeWindow_release(window: *mut std::ffi::c_void);
}

/// Runs in the disposable service process and contains Rust errors before crossing JNI.
extern "system" fn execute(
    mut env: JNIEnv<'_>,
    _class: JObject<'_>,
    arguments: JString<'_>,
    surface: JObject<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String> {
        let args: String = env.get_string(&arguments)?.into();
        let args: Vec<String> = serde_json::from_str(&args)?;
        // SAFETY: Android supplies a live Surface, retained by Java until this call returns.
        let window = std::ptr::NonNull::new(unsafe {
            ANativeWindow_fromSurface(env.get_native_interface(), surface.as_raw())
        })
        .context("Android probe surface unavailable")?;
        let result = unsafe { cinnabar::gpu_probe::execute_android(&args, window) };
        unsafe {
            ANativeWindow_release(window.as_ptr());
        }
        result
    }))
    .unwrap_or_else(|_| Err(anyhow::anyhow!("GPU probe panicked")));
    let output = match result {
        Ok(stdout) => Output {
            verdict: Verdict::Passed,
            stdout,
        },
        Err(error) => {
            eprintln!("GPU probe failed: {error:#}");
            Output {
                verdict: Verdict::Inconclusive,
                stdout: String::new(),
            }
        }
    };
    env.new_string(serde_json::to_string(&output).expect("probe result serializes"))
        .map_or(std::ptr::null_mut(), JString::into_raw)
}
