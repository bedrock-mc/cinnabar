//! Browser and document-picker handoff through Android intents.

use std::path::PathBuf;

use jni::{
    Env, JavaVM, jni_sig, jni_str,
    objects::{JClass, JObject, JObjectArray, JString, JValue},
};

/// Starts a browsable `ACTION_VIEW` intent; failure keeps manual sign-in recovery visible.
pub(super) fn open_url(url: &str) -> jni::errors::Result<bool> {
    let context = ndk_context::android_context();
    // SAFETY: NativeActivity initializes ndk-context with its live VM and global Context reference.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) };
    vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<bool> {
        // SAFETY: ndk-context retains this reference while the NativeActivity is alive.
        let activity_ptr = context.context().cast();
        let activity = unsafe { env.as_cast_raw::<JObject>(&activity_ptr)? };
        let application = env
            .call_method(
                &activity,
                jni_str!("getApplicationContext"),
                jni_sig!("()Landroid/content/Context;"),
                &[],
            )?
            .l()?;
        let uri = env.new_string(url)?;
        let uri = env
            .call_static_method(
                jni_str!("android/net/Uri"),
                jni_str!("parse"),
                jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                &[JValue::Object(&uri)],
            )?
            .l()?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(
            jni_str!("android/content/Intent"),
            jni_sig!("(Ljava/lang/String;Landroid/net/Uri;)V"),
            &[JValue::Object(&action), JValue::Object(&uri)],
        )?;
        let category = env.new_string("android.intent.category.BROWSABLE")?;
        env.call_method(
            &intent,
            jni_str!("addCategory"),
            jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(&category)],
        )?;
        let flags = env
            .get_static_field(
                jni_str!("android/content/Intent"),
                jni_str!("FLAG_ACTIVITY_NEW_TASK"),
                jni_sig!("I"),
            )?
            .i()?;
        env.call_method(
            &intent,
            jni_str!("addFlags"),
            jni_sig!("(I)Landroid/content/Intent;"),
            &[JValue::Int(flags)],
        )?;
        env.call_method(
            application,
            jni_str!("startActivity"),
            jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )?;
        Ok(true)
    })
}

/// Opens the Storage Access Framework picker for `mime_type` and blocks until the user chooses
/// or cancels. Custom file types use `*/*` and the caller checks filename extensions.
///
/// Returns private copies of the chosen documents, an empty list on cancellation, `None` when no
/// activity or document provider could show the picker, or an error when a document exceeds
/// `max_bytes` or could not be copied.
pub(super) fn pick_documents(
    multiple: bool,
    max_bytes: Option<u64>,
    mime_type: &str,
) -> Result<Option<Vec<PathBuf>>, String> {
    let context = ndk_context::android_context();
    // SAFETY: NativeActivity initializes ndk-context with its live VM and global Context reference.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) };
    vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<Option<Vec<PathBuf>>> {
        // SAFETY: ndk-context retains this reference while the NativeActivity is alive.
        let application_ptr = context.context().cast();
        let application = unsafe { env.as_cast_raw::<JObject>(&application_ptr)? };
        // Java has no unsigned long; a negative limit means unlimited.
        let max_bytes = max_bytes.map_or(-1, |bytes| i64::try_from(bytes).unwrap_or(i64::MAX));
        call_picker(env, &application, multiple, max_bytes, mime_type)
    })
    .map_err(|error| match error {
        jni::errors::Error::CaughtJavaException { name, msg, .. } => {
            if msg.is_empty() {
                name
            } else {
                msg
            }
        }
        error => error.to_string(),
    })
}

/// Calls `FilePicker.pick` through the app's loader, which can find app classes on native workers.
fn call_picker(
    env: &mut Env<'_>,
    application: &JObject<'_>,
    multiple: bool,
    max_bytes: i64,
    mime_type: &str,
) -> jni::errors::Result<Option<Vec<PathBuf>>> {
    let package = env
        .call_method(
            application,
            jni_str!("getPackageName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let package = env.cast_local::<JString>(package)?.try_to_string(env)?;
    let loader = env
        .call_method(
            application,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let name = env.new_string(format!("{package}.FilePicker"))?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    let class = env.cast_local::<JClass>(class)?;
    let mime_type = env.new_string(mime_type)?;
    let paths = env
        .call_static_method(
            class,
            jni_str!("pick"),
            jni_sig!("(ZJLjava/lang/String;)[Ljava/lang/String;"),
            &[
                JValue::Bool(multiple),
                JValue::Long(max_bytes),
                JValue::Object(&mime_type),
            ],
        )?
        .l()?;
    if paths.is_null() {
        return Ok(None);
    }
    let paths = env.cast_local::<JObjectArray<JString>>(paths)?;
    let count = paths.len(env)?;
    let mut picked = Vec::with_capacity(count);
    for index in 0..count {
        let path = paths.get_element(env, index)?;
        picked.push(PathBuf::from(path.try_to_string(env)?));
    }
    Ok(Some(picked))
}
