//! Regenerates the local server's embedded painting sizes from the runtime catalog.

/// Writes the gallery fixture without copying texture or atlas data.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let motives: Vec<_> = assets::painting::PAINTING_MOTIVES
        .iter()
        .map(|motive| serde_json::json!({"title": motive.title, "size": motive.size}))
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/localserver/painting_motives.json");
    std::fs::write(path, serde_json::to_vec_pretty(&motives)?)?;
    Ok(())
}
