//! Browser handoff through an Android intent, since Android has no desktop opener.

use jni::{JNIEnv, objects::JObject};

/// `Intent.FLAG_ACTIVITY_NEW_TASK`, required to start an activity from the application context.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;

/// Starts a browsable `ACTION_VIEW` intent; `Ok(false)` means no app accepted the link.
pub(super) fn open_url(url: &str) -> jni::errors::Result<bool> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
    let mut env = vm.attach_current_thread()?;
    // android-activity publishes the Application here, not the hosting activity.
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    let result = start_view_intent(&mut env, &application, url);
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
        return Ok(false);
    }
    result.map(|()| true)
}

/// Builds `new Intent(ACTION_VIEW, Uri.parse(url))` and starts it in a new task.
fn start_view_intent(env: &mut JNIEnv, context: &JObject, url: &str) -> jni::errors::Result<()> {
    let url = env.new_string(url)?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[(&url).into()],
        )?
        .l()?;
    let action = env.new_string("android.intent.action.VIEW")?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;Landroid/net/Uri;)V",
        &[(&action).into(), (&uri).into()],
    )?;
    let category = env.new_string("android.intent.category.BROWSABLE")?;
    env.call_method(
        &intent,
        "addCategory",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[(&category).into()],
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[FLAG_ACTIVITY_NEW_TASK.into()],
    )?;
    env.call_method(
        context,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[(&intent).into()],
    )?;
    Ok(())
}
