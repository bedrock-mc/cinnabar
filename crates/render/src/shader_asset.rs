//! Embedded shader assets shared by native and optional renderer caches.

/// Builds an embedded shader and its stable ID from one path, using Bevy's internal-asset paths.
macro_rules! internal_shader_asset {
    ($handle:expr, $path:literal, $loader:expr) => {
        (
            $handle.id(),
            ($loader)(
                include_str!($path),
                std::path::Path::new(file!())
                    .parent()
                    .expect("Rust source has a parent")
                    .join($path)
                    .to_string_lossy(),
            ),
        )
    };
}
pub(crate) use internal_shader_asset;
