use super::*;

/// Builds native preview geometry on its existing worker thread.
pub(super) fn spawn(
    skin: render_api::StandardSkin,
    sender: crossbeam_channel::Sender<Option<PreparedModel>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("menu-skin-model".into())
        .spawn(move || {
            let prepared = model_vertices(&skin).map(|vertices| PreparedModel {
                bounds: skin
                    .geometry
                    .as_ref()
                    .map(|_| super::super::fitting::OrbitBounds::new(&vertices)),
                source: skin.geometry,
                vertices,
            });
            let prepared = if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                None
            } else {
                prepared
            };
            let _ = sender.send(prepared);
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}
