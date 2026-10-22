//! Browser handoff through the hosting activity, since Android has no desktop opener.

use jni::objects::JObject;

/// Asks the activity to start an `ACTION_VIEW` intent; `Ok(false)` means no browser accepted it.
pub(super) fn open_url(url: &str) -> jni::errors::Result<bool> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the activity reference for this native loop.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(context.context().cast()) };
    let url = env.new_string(url)?;
    let result = env.call_method(
        &activity,
        "openUrl",
        "(Ljava/lang/String;)Z",
        &[(&url).into()],
    );
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
    }
    result?.z()
}
