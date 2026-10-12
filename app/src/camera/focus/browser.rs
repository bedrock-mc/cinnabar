//! A browser's pointer lock, which the page holds only while the browser allows it. Escape ends
//! it without the game asking, and some browsers grant it only inside the click or key event that
//! asks, not a frame later when the game reacts to that click.

use std::cell::Cell;

use wasm_bindgen::{JsCast, closure::Closure};

thread_local! {
    /// Whether a click on the game would capture the mouse; read inside the click's own event.
    static CAPTURE_ON_CLICK: Cell<bool> = const { Cell::new(false) };
    /// Whether a click on the pause menu's resume button would capture it.
    static RESUME_ON_CLICK: Cell<bool> = const { Cell::new(false) };
    /// Whether a click on the game requested capture since the game last looked.
    static CAPTURE_CLICKED: Cell<bool> = const { Cell::new(false) };
    /// Since when the page has held a lock the game has not asked for.
    static UNCLAIMED_SINCE: Cell<Option<f64>> = const { Cell::new(None) };
}

/// How long a lock requested inside a click may wait for the game to capture the mouse.
const CLAIM_SECONDS: f64 = 1.0;

/// Whether the page holds the browser's pointer lock.
pub(super) fn pointer_locked() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.pointer_lock_element())
        .is_some()
}

/// Exits a pointer lock the game has not claimed within [`CLAIM_SECONDS`], as when a click that
/// was expected to resume the game did not, so the cursor never stays hidden over a screen.
pub(super) fn release_unclaimed_lock(requested: bool, locked: bool, now: f64) {
    if requested || !locked {
        UNCLAIMED_SINCE.set(None);
        return;
    }
    let since = UNCLAIMED_SINCE.get().unwrap_or(now);
    UNCLAIMED_SINCE.set(Some(since));
    if now - since >= CLAIM_SECONDS
        && let Some(document) = web_sys::window().and_then(|window| window.document())
    {
        UNCLAIMED_SINCE.set(None);
        document.exit_pointer_lock();
    }
}

/// Records whether the next primary click on the canvas captures the mouse: in `gameplay`, or
/// on the pause menu's `resume` button.
pub(super) fn set_capture_on_click(gameplay: bool, resume: bool) {
    CAPTURE_ON_CLICK.set(gameplay);
    RESUME_ON_CLICK.set(resume);
}

/// Whether a click on the game requested capture since the last call, which the game then
/// consumes rather than treating it as an attack.
pub(super) fn take_capture_click() -> bool {
    CAPTURE_CLICKED.replace(false)
}

/// Requests pointer lock from within each primary-button press on the game's canvas that would
/// capture the mouse, so browsers that refuse a later request still grant it.
pub(super) fn install_click_capture() {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let listener = Closure::<dyn Fn(web_sys::MouseEvent)>::new(|event: web_sys::MouseEvent| {
        let gameplay = CAPTURE_ON_CLICK.get();
        if event.button() != 0 || !(gameplay || RESUME_ON_CLICK.get()) || pointer_locked() {
            return;
        }
        if let Some(canvas) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        {
            CAPTURE_CLICKED.set(gameplay);
            canvas.request_pointer_lock();
        }
    });
    // winit cancels the canvas's `pointerdown`, so browsers fire no `mousedown` after it; a
    // mouse's `pointerdown` still activates the page.
    let _ =
        document.add_event_listener_with_callback("pointerdown", listener.as_ref().unchecked_ref());
    // The listener lives as long as the page.
    listener.forget();
}
