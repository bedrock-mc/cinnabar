use super::NativeFocus;
use bevy::{
    prelude::{
        App, IntoScheduleConfigs, NonSendMut, PreUpdate, Query, Res, ResMut, Resource, With, error,
        warn,
    },
    window::{PrimaryWindow, RawHandleWrapper},
};
use raw_window_handle::RawWindowHandle;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetActiveWindow, GetCapture, GetFocus, ReleaseCapture},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            ClipCursor, IsIconic, SIZE_MINIMIZED, WM_KILLFOCUS, WM_NCACTIVATE, WM_NCDESTROY,
            WM_SETFOCUS, WM_SIZE,
        },
    },
};

#[derive(Resource, Default)]
pub(crate) struct NativeCaptureReady(pub bool);

#[derive(Default)]
struct NativeBinding(Option<(HWND, RawHandleWrapper)>);

impl Drop for NativeBinding {
    /// Removes the callback while the retained native window is still alive.
    fn drop(&mut self) {
        if let Some((window, _)) = &self.0 {
            unsafe {
                RemoveWindowSubclass(*window, Some(focus_proc), focus_proc as *const () as usize);
            }
        }
    }
}

/// Registers the callback on the event-loop thread before any game capture.
pub(in crate::camera::focus) fn install(app: &mut App) {
    app.init_resource::<NativeCaptureReady>()
        .init_non_send_resource::<NativeBinding>()
        .add_systems(PreUpdate, bind.before(super::super::track_focus));
}

/// Retains the native window and fails closed if its focus hook cannot be installed.
fn bind(
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    driven: Option<Res<super::super::super::DrivenInput>>,
    mut binding: NonSendMut<NativeBinding>,
    mut ready: ResMut<NativeCaptureReady>,
) {
    if driven.is_some() || binding.0.is_some() {
        return;
    }
    let Ok(raw) = windows.single() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = raw.get_window_handle() else {
        return;
    };
    let window = handle.hwnd.get() as HWND;
    let focus = unsafe {
        NativeFocus::new(
            GetActiveWindow() == window,
            GetFocus() == window,
            IsIconic(window) != 0,
        )
    };
    ready.0 = unsafe {
        SetWindowSubclass(
            window,
            Some(focus_proc),
            focus_proc as *const () as usize,
            focus.0,
        ) != 0
    };
    if !ready.0 {
        error!(
            "could not install native cursor focus handler: {}",
            std::io::Error::last_os_error()
        );
    }
    binding.0 = Some((window, raw.clone()));
}

/// Releases desktop ownership once, inside the first native input-loss notification.
unsafe extern "system" fn focus_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let change = match message {
        WM_NCACTIVATE => Some((1, wparam != 0)),
        WM_SETFOCUS => Some((2, true)),
        WM_KILLFOCUS => Some((2, false)),
        WM_SIZE => Some((4, wparam == SIZE_MINIMIZED as usize)),
        _ => None,
    };
    if let Some((bit, enabled)) = change {
        let mut focus = NativeFocus(data);
        let release = focus.change(bit, enabled);
        // Store the transition before ReleaseCapture can synchronously send another message.
        unsafe {
            SetWindowSubclass(window, Some(focus_proc), id, focus.0);
        }
        if release {
            if unsafe { ClipCursor(std::ptr::null()) } == 0 {
                warn!(
                    "could not release cursor clip: {}",
                    std::io::Error::last_os_error()
                );
            }
            if unsafe { GetCapture() } == window && unsafe { ReleaseCapture() } == 0 {
                warn!(
                    "could not release mouse capture: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(window, Some(focus_proc), id);
        }
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}
