//! Vanilla client chunk-grid retention.
//!
//! Matches the vanilla Bedrock client's rule for which loaded chunk columns a
//! player keeps: a square grid clipped by a circular horizontal-distance test,
//! sized from the server-confirmed chunk radius rather than the radius the
//! client requested.

/// Chunks the client's grid retains beyond the server-confirmed radius. Its
/// view distance is the confirmed radius plus one, square-clipped one chunk
/// further, so a column survives out to Chebyshev distance `radius + 2`.
pub const CHUNK_VIEW_SLACK: i32 = 2;

/// View distance the client's chunk grid uses: the server-confirmed radius plus
/// one, floored at one, regardless of the radius the client requested.
#[must_use]
pub fn chunk_view_distance(radius: i32) -> i64 {
    (i64::from(radius) + 1).max(1)
}

/// Reports whether the client's chunk grid centered on `center` retains the
/// column at `chunk`, matching the vanilla client's circular view test on
/// horizontal chunk distance, clipped to the grid's square boundary. `chunk`
/// and `center` are `[x, z]` chunk coordinates.
///
/// The circle test uses `f32` arithmetic with a strict less-than and the
/// `1.5 + sqrt(3)` slack added left to right so its rounding matches the
/// client.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "squared chunk deltas stay far inside f32's exact-integer range"
)]
pub fn chunk_in_view(radius: i32, chunk: [i32; 2], center: [i32; 2]) -> bool {
    let view = chunk_view_distance(radius);
    let dx = (i64::from(center[0]) - i64::from(chunk[0])).abs();
    let dz = (i64::from(center[1]) - i64::from(chunk[1])).abs();
    let max_coordinate = view + 1;
    if dx > max_coordinate || dz > max_coordinate {
        return false;
    }
    let threshold = (view as f32 + 1.5) + 1.732_050_8;
    ((dx * dx + dz * dz) as f32) < threshold * threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_in_view_matches_client_grid_retention() {
        let center = [0, 0];
        // (server-confirmed radius, tested chunk, retained?), mirrored to the
        // negative quadrant for every case.
        let cases: &[(i32, [i32; 2], bool)] = &[
            (8, [10, 0], true),
            (8, [11, 0], false),
            (8, [10, 7], true),
            (8, [10, 8], false),
            (8, [10, 10], false),
            (8, [8, 8], true),
            (2, [4, 4], true),
            (2, [5, 0], false),
            (0, [2, 2], true),
        ];
        for &(radius, chunk, want) in cases {
            assert_eq!(
                chunk_in_view(radius, chunk, center),
                want,
                "chunk_in_view({radius}, {chunk:?}, {center:?})"
            );
            let mirrored = [-chunk[0], -chunk[1]];
            assert_eq!(
                chunk_in_view(radius, mirrored, center),
                want,
                "chunk_in_view({radius}, {mirrored:?}, {center:?})"
            );
        }
    }
}
