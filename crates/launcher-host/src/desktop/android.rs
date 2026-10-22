//! Browser and document-picker handoff through Android intents, since Android has no desktop
//! opener or picker tools.

use jni::{
    JNIEnv,
    objects::{JClass, JObject, JObjectArray, JString},
};
use std::path::PathBuf;

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

/// Opens the Storage Access Framework picker and blocks until the user chooses or cancels.
///
/// Returns private copies of the chosen documents, an empty list on cancellation, or an error
/// when no activity or document provider could show the picker or a copy failed.
pub(super) fn pick_documents(multiple: bool) -> Result<Vec<PathBuf>, String> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    let result = call_picker(&mut env, &application, multiple);
    match result {
        Ok(Some(paths)) => Ok(paths),
        Ok(None) => Err("no document picker is available".to_owned()),
        Err(error) => Err(pending_exception(&mut env).unwrap_or_else(|| error.to_string())),
    }
}

/// Calls `FilePicker.pick`, loading the class through the app's loader because worker threads
/// attached from native code only see system classes.
fn call_picker(
    env: &mut JNIEnv,
    application: &JObject,
    multiple: bool,
) -> jni::errors::Result<Option<Vec<PathBuf>>> {
    let package = env
        .call_method(application, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    let package: String = env.get_string(&JString::from(package))?.into();
    let loader = env
        .call_method(
            application,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let name = env.new_string(format!("{package}.FilePicker"))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    let paths = env
        .call_static_method(
            JClass::from(class),
            "pick",
            "(Z)[Ljava/lang/String;",
            &[multiple.into()],
        )?
        .l()?;
    if paths.is_null() {
        return Ok(None);
    }
    let paths = JObjectArray::from(paths);
    let count = env.get_array_length(&paths)?;
    let mut picked = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        let path = JString::from(env.get_object_array_element(&paths, index)?);
        let path: String = env.get_string(&path)?.into();
        picked.push(PathBuf::from(path));
    }
    Ok(Some(picked))
}

/// Clears a pending Java exception and returns its message for the import status line.
fn pending_exception(env: &mut JNIEnv) -> Option<String> {
    let throwable = env.exception_occurred().ok()?;
    if throwable.is_null() {
        return None;
    }
    env.exception_clear().ok()?;
    let message = env
        .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
        .and_then(|value| value.l())
        .ok()?;
    let message: String = env.get_string(&JString::from(message)).ok()?.into();
    Some(message)
}
