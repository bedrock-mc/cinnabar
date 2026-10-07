use super::wrap_degrees;

const BODY_FOLLOW: f32 = 0.3;
const HEAD_LIMIT: f32 = 75.0;
pub(super) const HEAD_SOFT_LIMIT_SQUARED: f32 = 2500.0;
pub(super) const HEAD_SOFT_PULL: f32 = 0.2;
const FACING_DISTANCE_SQUARED: f32 = 0.002_500_000_2;

/// One Java body step uses this completed tick's displacement, look and attack progress.
pub(in crate::actor_animation) fn advance(
    heading: &mut [f32; 2],
    [dx, _, dz]: [f32; 3],
    yaw: f32,
    swing: f32,
) {
    heading[0] = heading[1];
    let mut body = heading[1];
    let mut facing = body;
    if dx * dx + dz * dz > FACING_DISTANCE_SQUARED {
        facing = (f64::from(dz).atan2(f64::from(dx)) as f32).to_degrees() - 90.0;
    }
    if swing > 0.0 {
        facing = yaw;
    }
    body += wrap_degrees(facing - body) * BODY_FOLLOW;
    let lag = wrap_degrees(yaw - body).clamp(-HEAD_LIMIT, HEAD_LIMIT);
    body = yaw - lag;
    if lag * lag > HEAD_SOFT_LIMIT_SQUARED {
        body += lag * HEAD_SOFT_PULL;
    }
    heading[1] = body;
}
