use cinnabar_vita::{
    memory::{Budget, Pool},
    native::Renderer,
};
use std::io::Write;

/// Read the real frame, admit temporary pixels, and return a stable comparison hash.
pub fn frame(renderer: &Renderer, budget: &Budget, path: &str) -> Result<u64, String> {
    let [width, height] = renderer.dimensions();
    let bytes = width as usize * height as usize * 4;
    let _lease = budget
        .reserve(Pool::Main, bytes)
        .map_err(|e| format!("{e:?}"))?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(bytes).map_err(|e| e.to_string())?;
    let _padding = budget
        .reserve(Pool::Main, pixels.capacity() - bytes)
        .map_err(|e| format!("{e:?}"))?;
    pixels.resize(bytes, 0);
    renderer
        .read_pixels(&mut pixels)
        .map_err(|e| format!("{e:?}"))?;
    if !pixels.chunks_exact(4).enumerate().any(|(index, pixel)| {
        let x = index as u32 % width;
        let y = index as u32 / width;
        // A diagnostic reticle alone cannot prove terrain rendered.
        let reticle = x.abs_diff(width / 2) <= 8 && y.abs_diff(height / 2) <= 8;
        !reticle && pixel[..3] != pixels[..3]
    }) {
        return Err("uniform frame".into());
    }
    // FNV-1a is only a non-security regression comparison; artifact identity uses SHA-256.
    let hash = pixels.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    for rgba in pixels.chunks_exact_mut(4) {
        rgba.swap(0, 2);
    }
    let mut header = [0_u8; 18];
    header[2] = 2;
    header[12..14].copy_from_slice(&(width as u16).to_le_bytes());
    header[14..16].copy_from_slice(&(height as u16).to_le_bytes());
    header[16] = 32;
    header[17] = 8;
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    file.write_all(&header)
        .and_then(|_| file.write_all(&pixels))
        .map_err(|e| e.to_string())?;
    Ok(hash)
}
