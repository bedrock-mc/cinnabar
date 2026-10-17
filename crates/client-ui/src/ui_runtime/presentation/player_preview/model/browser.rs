use super::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/../../web/site/dressing.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = nextDressingFrame)]
    fn next_frame() -> js_sys::Promise;
}

/// Yields before each preparation stage and discards work superseded by a newer selection.
async fn checkpoint(cancelled: &std::sync::atomic::AtomicBool) -> Option<()> {
    wasm_bindgen_futures::JsFuture::from(next_frame())
        .await
        .ok()?;
    (!cancelled.load(std::sync::atomic::Ordering::Relaxed)).then_some(())
}

/// Prepares model stages on separate browser frames without a native thread or inline mutex.
pub(super) fn spawn(
    skin: render_api::StandardSkin,
    sender: crossbeam_channel::Sender<Option<PreparedModel>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    wasm_bindgen_futures::spawn_local(async move {
        let prepared = prepare(skin, &cancelled).await;
        let _ = sender.try_send(prepared);
    });
    Ok(())
}

/// Separates JSON parsing, mesh preparation and bounded vertex conversion into task slices.
async fn prepare(
    skin: render_api::StandardSkin,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Option<PreparedModel> {
    checkpoint(cancelled).await?;
    let source = skin.geometry.clone();
    let vertices = if let Some(source) = &source {
        let geometry =
            assets::parse_skin_geometry(&source.resource_patch, &source.geometry_data).ok()??;
        checkpoint(cancelled).await?;
        let rig = render_model::skin_geometry(&geometry, render_model::EntityRigId(0)).ok()?;
        checkpoint(cancelled).await?;
        let parts: Vec<_> = (0..geometry.bones.len())
            .map(|index| part(&geometry, index))
            .collect();
        let mut vertices = Vec::with_capacity(rig.vertices.len());
        for batch in rig.vertices.chunks(1024) {
            checkpoint(cancelled).await?;
            vertices.extend(batch.iter().map(|vertex| preview_vertex(vertex, &parts)));
        }
        vertices.into()
    } else {
        model_vertices(&skin)?
    };
    checkpoint(cancelled).await?;
    Some(PreparedModel {
        bounds: source
            .as_ref()
            .map(|_| super::super::fitting::OrbitBounds::new(&vertices)),
        source,
        vertices,
    })
}
