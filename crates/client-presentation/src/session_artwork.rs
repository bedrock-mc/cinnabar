//! Shared session artwork precedence for held, dropped and framed items.

/// An admitted session sheet or sprite, ahead of any startup artwork.
pub(crate) enum SessionArtwork<B, S> {
    Block(B),
    Sprite(S),
}

/// Selects a compatible session sheet, then a session icon; only `None` permits startup art.
pub(crate) fn session_artwork<B, S>(
    block: Option<B>,
    sprite: impl FnOnce() -> Option<S>,
) -> Option<SessionArtwork<B, S>> {
    block
        .map(SessionArtwork::Block)
        .or_else(|| sprite().map(SessionArtwork::Sprite))
}
