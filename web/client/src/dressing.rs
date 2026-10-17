//! Synchronous picker admission called inside the page's import gesture.

use launcher::skin_import::picked;
use wasm_bindgen::prelude::*;

/// Exposes the canonical file count before JavaScript enumerates selected file metadata.
#[wasm_bindgen]
pub fn skin_picker_file_limit() -> usize {
    picked::MAX_PICKED_FILES
}

/// Rejects file names and declared sizes before any File.arrayBuffer or Wasm byte copy.
#[wasm_bindgen]
pub fn skin_picker_admit(metadata: &str, cape: bool) -> Result<(), JsError> {
    if metadata.len() > picked::MAX_PICKED_METADATA_BYTES {
        return Err(JsError::new("The selected file metadata is too large."));
    }
    let files: Vec<(String, u64)> = serde_json::from_str(metadata)
        .map_err(|_| JsError::new("The selected file metadata is invalid."))?;
    picked::admit(
        &files
            .iter()
            .map(|(name, size)| (name.as_str(), *size))
            .collect::<Vec<_>>(),
        cape,
    )
    .map_err(|error| JsError::new(&error.to_string()))
}
