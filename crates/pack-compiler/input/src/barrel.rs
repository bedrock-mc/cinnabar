//! Shared barrel face orientation for placed and carried models.

use assets::BlockFace;

/// Maps a barrel face to its authored north-facing texture slot and native UV flip.
pub fn face_transform(face: BlockFace, direction: usize) -> Option<(BlockFace, u8)> {
    // Retain all six authored faces, including individual side-face overrides.
    const FACES: [[BlockFace; 6]; 6] = [
        [
            BlockFace::West,
            BlockFace::East,
            BlockFace::North,
            BlockFace::South,
            BlockFace::Up,
            BlockFace::Down,
        ],
        [
            BlockFace::West,
            BlockFace::East,
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
        ],
        BlockFace::ALL,
        [
            BlockFace::East,
            BlockFace::West,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::South,
            BlockFace::North,
        ],
        [
            BlockFace::North,
            BlockFace::South,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::East,
            BlockFace::West,
        ],
        [
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::West,
            BlockFace::East,
        ],
    ];
    const FLIPS: [[u8; 6]; 6] = [
        [3, 3, 0, 0, 3, 3],
        [0; 6],
        [2, 2, 3, 0, 0, 0],
        [1, 1, 0, 3, 0, 0],
        [0, 0, 2, 2, 2, 2],
        [0, 0, 1, 1, 1, 1],
    ];
    Some((
        *FACES.get(direction)?.get(face as usize)?,
        FLIPS[direction][face as usize],
    ))
}
