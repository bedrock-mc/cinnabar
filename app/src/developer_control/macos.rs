//! Native application policy for invisible developer-control sessions.

use anyhow::{Context, Result, ensure};
use bevy::{ecs::system::NonSendMarker, prelude::*};
use objc2::{
    ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::NSObjectProtocol,
};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu};

define_class!(
    // SAFETY: NSApplication allows subclassing; this class adds no state or destruction hooks.
    #[unsafe(super = NSApplication)]
    #[thread_kind = MainThreadOnly]
    struct HiddenApplication;

    impl HiddenApplication {
        /// Backend launch callbacks cannot promote a hidden process into a foreground app.
        #[unsafe(method(setActivationPolicy:))]
        fn set_activation_policy(&self, _requested: NSApplicationActivationPolicy) -> bool {
            // SAFETY: The superclass selector accepts this policy and returns an Objective-C BOOL.
            unsafe { msg_send![super(self), setActivationPolicy: NSApplicationActivationPolicy::Prohibited] }
        }

        /// Hidden rendering never takes focus from the user's current application.
        #[unsafe(method(activateIgnoringOtherApps:))]
        fn activate_ignoring_other_apps(&self, _ignore_other_apps: bool) {}

        /// A hidden developer process never publishes a default application menu.
        #[unsafe(method(setMainMenu:))]
        fn set_main_menu(&self, _menu: Option<&NSMenu>) {}

        /// Suppress the services menu alongside the main application menu.
        #[unsafe(method(setServicesMenu:))]
        fn set_services_menu(&self, _menu: Option<&NSMenu>) {}
    }
);

/// Own the application singleton before the window backend installs its launch callbacks.
pub(super) fn prepare_hidden_application() -> Result<()> {
    let _main_thread =
        MainThreadMarker::new().context("hidden rendering requires the main thread")?;
    // SAFETY: This inherited selector constructs the singleton on the main thread and returns
    // an NSApplication. Check its dynamic type before relying on our activation overrides.
    let app: Retained<NSApplication> =
        unsafe { msg_send![HiddenApplication::class(), sharedApplication] };
    ensure!(
        app.isKindOfClass(HiddenApplication::class()),
        "hidden rendering must configure macOS before the native application is created"
    );
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    ensure!(
        app.activationPolicy() == NSApplicationActivationPolicy::Prohibited,
        "macOS did not apply the hidden rendering activation policy"
    );
    Ok(())
}

/// Checks the native state once the window backend has completed its launch callbacks.
pub(super) fn verify_after_launch(app: &mut App) {
    app.add_systems(Startup, verify_hidden_application);
}

/// Emits a native hidden-mode witness and stops if launch callbacks changed the policy.
fn verify_hidden_application(_main_thread: NonSendMarker, mut exit: MessageWriter<AppExit>) {
    let main_thread =
        MainThreadMarker::new().expect("native activation checks run on the main thread");
    let app = NSApplication::sharedApplication(main_thread);
    let prohibited = app.activationPolicy() == NSApplicationActivationPolicy::Prohibited;
    let active = app.isActive();
    let main_menu = app.mainMenu().is_some();
    let services_menu = app.servicesMenu().is_some();
    eprintln!(
        "DEVELOPER_CONTROL_NATIVE prohibited={prohibited} active={active} main_menu={main_menu} services_menu={services_menu}"
    );
    if !prohibited || active || main_menu || services_menu {
        eprintln!(
            "developer control stopping: macOS hidden application policy was not preserved after launch"
        );
        exit.write(AppExit::error());
    }
}
