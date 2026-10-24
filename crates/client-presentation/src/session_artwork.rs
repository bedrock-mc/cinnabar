//! Shared artwork fallback order for held, dropped and framed items.

/// One artwork source to attempt, including geometry construction and admission.
#[derive(Clone, Copy)]
pub(crate) enum ItemArtwork {
    SessionCube,
    SessionSprite,
    StartupCube,
    StartupSprite,
}

/// Tries session artwork before startup artwork, continuing after any refused attempt.
pub(crate) fn item_artwork<C, T>(
    context: &mut C,
    mut attempt: impl FnMut(&mut C, ItemArtwork) -> Option<T>,
) -> Option<T> {
    use ItemArtwork::*;
    [SessionCube, SessionSprite, StartupCube, StartupSprite]
        .into_iter()
        .find_map(|source| attempt(context, source))
}
