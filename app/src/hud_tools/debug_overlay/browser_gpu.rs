//! The GPU the browser names. WebGPU leaves the adapter name empty for ordinary pages, so the
//! debug screen reads WebGL's renderer string instead.

use std::sync::OnceLock;

use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGlRenderingContext, WebglLoseContext};

/// `UNMASKED_RENDERER_WEBGL` from the `WEBGL_debug_renderer_info` extension.
const UNMASKED_RENDERER: u32 = 0x9246;

/// The browser's GPU name, read once from a throwaway WebGL context; `None` when it reveals none.
pub(super) fn name() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| read().map(|renderer| tidy(&renderer)))
        .as_deref()
}

/// Reads the renderer string, preferring the unmasked one, then releases the context.
fn read() -> Option<String> {
    let canvas = web_sys::window()?
        .document()?
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    let gl = canvas
        .get_context("webgl")
        .ok()??
        .dyn_into::<WebGlRenderingContext>()
        .ok()?;
    let unmasked = gl
        .get_extension("WEBGL_debug_renderer_info")
        .ok()
        .flatten()
        .and_then(|_| gl.get_parameter(UNMASKED_RENDERER).ok())
        .and_then(|value| value.as_string());
    // Browsers that retired the extension report the unmasked name as the plain renderer.
    let renderer = unmasked.or_else(|| {
        gl.get_parameter(WebGlRenderingContext::RENDERER)
            .ok()
            .and_then(|value| value.as_string())
    });
    if let Some(lose) = gl.get_extension("WEBGL_lose_context").ok().flatten() {
        lose.unchecked_into::<WebglLoseContext>().lose_context();
    }
    renderer.filter(|renderer| !renderer.trim().is_empty())
}

/// Keeps the device from ANGLE's `ANGLE (vendor, device, backend)` form, without its PCI ID,
/// shader models, Metal prefix or Vulkan version; any other string passes through trimmed.
fn tidy(renderer: &str) -> String {
    let renderer = renderer.trim();
    renderer
        .strip_prefix("ANGLE (")
        .and_then(|inner| inner.strip_suffix(')'))
        .and_then(|inner| {
            let (_vendor, rest) = inner.split_once(", ")?;
            Some(
                rest.rsplit_once(", ")
                    .map_or(rest, |(device, _backend)| device),
            )
        })
        .map(angle_device)
        .filter(|device| !device.is_empty())
        .unwrap_or_else(|| renderer.to_owned())
}

/// The device inside ANGLE's device field, such as `Vulkan 1.3 (NVIDIA GeForce RTX 3060
/// (0x00002503))` or `NVIDIA GeForce RTX 3060 (0x00002503) Direct3D11 vs_5_0 ps_5_0`.
fn angle_device(device: &str) -> String {
    let device = device
        .strip_prefix("ANGLE Metal Renderer: ")
        .unwrap_or(device);
    let end = [" (0x", " Direct3D"]
        .iter()
        .filter_map(|marker| device.find(marker))
        .min()
        .unwrap_or(device.len());
    let device = &device[..end];
    let device = device
        .strip_prefix("Vulkan ")
        .and_then(|versioned| versioned.split_once(" ("))
        .map_or(device, |(_version, inner)| inner);
    // Cutting the PCI ID can leave the brackets around it unbalanced.
    let mut device = device.trim().to_owned();
    let opened = device.matches('(').count();
    let closed = device.matches(')').count();
    if closed > opened {
        for _ in opened..closed {
            if device.ends_with(')') {
                device.pop();
            }
        }
    } else {
        device.extend(std::iter::repeat_n(')', opened - closed));
    }
    device.trim().to_owned()
}
