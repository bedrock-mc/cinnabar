/** Opens only freshly hit-tested sign-in actions inside a trusted browser gesture. */
export function signInGestures(canvas, actionAt, focusedAction, link, report, view = window) {
  let disposed = false;
  /** Validates the current prompt URL again at the browser boundary. */
  const open = () => {
    const uri = link();
    if (!uri) return;
    let url;
    try { url = new URL(uri); } catch { report("The sign-in link is invalid."); return; }
    if (url.protocol !== "https:" || !url.hostname || url.username || url.password) {
      report("The sign-in link is invalid."); return;
    }
    view.open(url.href, "_blank", "noopener,noreferrer");
  };
  /** Uses this event's coordinates rather than a previous frame's hover state. */
  const pointer = (event) => {
    if (!disposed && event.isTrusted && event.isPrimary && event.button === 0 &&
        actionAt(event.clientX, event.clientY) === "open_sign_in_link") open();
  };
  /** Lets focused menu links be activated without relying on pointer hover. */
  const keyboard = (event) => {
    if (!disposed && event.isTrusted && !event.repeat && ["Enter", " "].includes(event.key) &&
        focusedAction() === "open_sign_in_link") { event.preventDefault(); open(); }
  };
  canvas.addEventListener("pointerdown", pointer);
  canvas.addEventListener("keydown", keyboard);
  return () => {
    disposed = true;
    canvas.removeEventListener("pointerdown", pointer);
    canvas.removeEventListener("keydown", keyboard);
  };
}

/** Owns one bounded preference key and makes read, quota and security failures visible. */
export class BrowserPreferences {
  /** Receives the Rust schema validator and ceiling without duplicating either policy. */
  constructor(validate, maximum, report, storage = () => window.localStorage) {
    this.validate = validate; this.maximum = maximum; this.report = report; this.storage = storage;
    this.key = "cinnabar.menu.v1";
    this.writable = true;
  }
  /** Reads a checked document without converting a failed read into a successful empty save. */
  load() {
    try {
      const json = this.storage().getItem(this.key);
      return json === null ? null : this.checked(json);
    } catch { this.writable = false; this.report("Browser preferences could not be read. Your saved data was kept."); return null; }
  }
  /** Returns whether this exact document became durable; never reports success on quota failure. */
  save(json) {
    try {
      if (!this.writable) throw new Error("Previous read failed");
      this.storage().setItem(this.key, this.checked(json)); return true;
    }
    catch { this.report("Browser preferences could not be saved. Changes last for this tab only."); return false; }
  }
  /** Bounds text before encoding or parsing and delegates schema policy to its Rust owner. */
  checked(json) {
    if (typeof json !== "string" || json.length > this.maximum || new TextEncoder().encode(json).length > this.maximum) throw new Error("Preferences too large");
    return this.validate(json);
  }
}
