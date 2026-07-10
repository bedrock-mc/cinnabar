pub const SAFE_SERVER_HEIGHT: f32 = 80.0;
const BEDROCK_POSITION_SENTINEL_Y: f32 = 32_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedServerPosition {
    pub position: [f32; 3],
    pub surface_anchor: Option<[i32; 2]>,
}

#[must_use]
pub fn resolve_server_position(
    server: [f32; 3],
    current: [f32; 3],
    existing_anchor: Option<[i32; 2]>,
) -> ResolvedServerPosition {
    if server.iter().all(|component| component.is_finite())
        && server[1].abs() < BEDROCK_POSITION_SENTINEL_Y
    {
        return ResolvedServerPosition {
            position: server,
            surface_anchor: None,
        };
    }

    let horizontal_is_finite = server[0].is_finite() && server[2].is_finite();
    let (x, z, anchor) = if horizontal_is_finite {
        (
            server[0],
            server[2],
            [floor_to_i32(server[0]), floor_to_i32(server[2])],
        )
    } else if let Some(anchor) = existing_anchor {
        (
            finite_or(current[0], anchor[0] as f32 + 0.5),
            finite_or(current[2], anchor[1] as f32 + 0.5),
            anchor,
        )
    } else {
        let x = finite_or(current[0], 0.5);
        let z = finite_or(current[2], 0.5);
        (x, z, [floor_to_i32(x), floor_to_i32(z)])
    };
    ResolvedServerPosition {
        position: [x, SAFE_SERVER_HEIGHT, z],
        surface_anchor: Some(anchor),
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn floor_to_i32(value: f32) -> i32 {
    if value <= i32::MIN as f32 {
        i32::MIN
    } else if value >= i32::MAX as f32 {
        i32::MAX
    } else {
        value.floor() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::{SAFE_SERVER_HEIGHT, resolve_server_position};

    #[test]
    fn move_player_sentinel_keeps_recovery_active_at_the_new_finite_horizontal_anchor() {
        let resolved = resolve_server_position(
            [20.25, 32_769.62, 30.75],
            [10.25, 70.0, 11.75],
            Some([10, 11]),
        );

        assert_eq!(resolved.position, [20.25, SAFE_SERVER_HEIGHT, 30.75]);
        assert_eq!(resolved.surface_anchor, Some([20, 30]));
    }

    #[test]
    fn change_dimension_non_finite_sentinel_retains_the_existing_recovery_anchor() {
        let resolved = resolve_server_position(
            [f32::NAN, 32_000.0, f32::INFINITY],
            [7.25, 70.0, -8.75],
            Some([7, -9]),
        );

        assert_eq!(resolved.position, [7.25, SAFE_SERVER_HEIGHT, -8.75]);
        assert_eq!(resolved.surface_anchor, Some([7, -9]));
    }

    #[test]
    fn invalid_bootstrap_creates_a_surface_anchor_and_valid_update_clears_it() {
        let bootstrap = resolve_server_position(
            [-104.25, f32::NEG_INFINITY, 61.25],
            [0.0, SAFE_SERVER_HEIGHT, 0.0],
            None,
        );
        assert_eq!(bootstrap.position, [-104.25, SAFE_SERVER_HEIGHT, 61.25]);
        assert_eq!(bootstrap.surface_anchor, Some([-105, 61]));

        let valid = resolve_server_position(
            [-100.0, 115.75, 60.0],
            bootstrap.position,
            bootstrap.surface_anchor,
        );
        assert_eq!(valid.position, [-100.0, 115.75, 60.0]);
        assert_eq!(valid.surface_anchor, None);
    }
}
