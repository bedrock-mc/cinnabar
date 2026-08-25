//! Pure bounded evidence formatting for failed spawn-anchor probes.
//!
//! Live third-party evidence (2026-08-25, `play.lbsg.net`) showed two
//! embedded-anchor holds each failing open at exactly 200 ticks before the
//! transmitted inputless slide was rejected as "movement cheats" — and the
//! captured stream could not answer WHICH blocks seal the pocket. This
//! module renders one single-line stdout marker per failed probe attempt so
//! the next authorized live session can adjudicate the disagreement between
//! local collision truth and the server's lobby geometry.
//!
//! Everything here is pure: no world access, no state, no clock. Callers pay
//! allocation only on an actual failed attempt with instrumentation enabled;
//! the disabled path returns before touching the collider slice, so per-tick
//! allocation is unchanged and probe outcomes are byte-identical either way.
//!
//! Collider attribution is geometric: the production `sim::PaletteWorld`
//! emits one translated shape instance per (block, layer), so a collider
//! lying fully inside one unit cell is reported with that cell as its
//! contributing `block` coordinate, while a collider spanning multiple cells
//! cannot be attributed to a single block and reports `merged:true` with its
//! AABB alone. The current `CollisionWorld` trait surface returns bare AABBs
//! and cannot supply wire runtime IDs; no `runtime_id` field is emitted
//! until the trait grows provenance (recorded open limitation, not a silent
//! omission).

use sim::{Aabb, Vec3};

/// Stdout line prefix, family style of `MOVEMENT_TX_GATE=`/`TELEPORT_ACK=`.
/// The registered opt-in environment literal is
/// `crate::acceptance::markers::ANCHOR_PROBE`.
pub(super) const MARKER_PREFIX: &str = "ANCHOR_PROBE=";

const SCHEMA_TAG: &str = "rust-mcbe-anchor-probe-v1";

/// PROVISIONAL hard cap on how many sealing colliders one failure marker
/// may name, chosen so a worst-case well-formed line stays far below the
/// byte cap.
pub(super) const MAX_SEALING_COLLIDERS: usize = 8;

/// PROVISIONAL hard byte cap for one complete marker line. When evidence
/// does not fit, the collider list truncates (recording `truncated:true`
/// plus the full `total_sealing_count`) instead of emitting oversized or
/// invalid output.
pub(super) const MARKER_BYTE_CAP: usize = 2048;

/// Tolerance for unit-cell containment checks against translated registry
/// shapes, whose face coordinates are integer-offset dyadics.
const CELL_TOLERANCE: f64 = 1.0e-9;

const CLOSING_BYTES: usize = 2;

/// Quantizes one coordinate to 1/64-block precision. Values whose scaled
/// form leaves the finite range fall back to the raw coordinate so output
/// stays a valid finite JSON number.
fn quantize_to_1_64(value: f64) -> f64 {
    let quantized = (value * 64.0).round() / 64.0;
    if quantized.is_finite() {
        quantized
    } else {
        value
    }
}

/// Marks one rendered float so every JSON consumer parses it on the float
/// path: strict parsers reject bare integers beyond the u64/i64 range, so
/// an exponent-free integral rendering gains an explicit `.0`.
fn mark_float(rendered: String) -> String {
    if rendered.contains(['.', 'e', 'E']) {
        rendered
    } else {
        rendered + ".0"
    }
}

/// The unit block coordinate for one axis, when the collider's extent on
/// that axis stays inside the single block cell above its lower face.
///
/// Registry shapes are validated inside a local halo wider than their own
/// block, so containment in one cell is the strongest attribution available
/// from bare AABBs; anything spanning cells reports as merged instead.
fn axis_cell(low: f64, high: f64) -> Option<i32> {
    if !low.is_finite() {
        return None;
    }
    let floored = low.floor();
    if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&floored) {
        return None;
    }
    let block = floored as i32;
    if high > f64::from(block) + 1.0 + CELL_TOLERANCE {
        return None;
    }
    Some(block)
}

/// The unit block cell fully containing `collider`, when provably unique.
fn containing_cell(collider: Aabb) -> Option<[i32; 3]> {
    Some([
        axis_cell(collider.min.x, collider.max.x)?,
        axis_cell(collider.min.y, collider.max.y)?,
        axis_cell(collider.min.z, collider.max.z)?,
    ])
}

/// Sealing candidates must be finite and genuinely overlap the anchored
/// player box under the same strict-contact rules the simulator uses.
fn is_reportable(collider: Aabb, player: Aabb) -> bool {
    collider.min.is_finite() && collider.max.is_finite() && collider.intersects(player)
}

/// Renders one collider descriptor with 1/64-quantized bounds.
fn render_collider(collider: Aabb) -> String {
    let mut out = String::with_capacity(96);
    out.push_str("{\"min\":[");
    for (axis, value) in [collider.min.x, collider.min.y, collider.min.z]
        .into_iter()
        .enumerate()
    {
        if axis > 0 {
            out.push(',');
        }
        out.push_str(&mark_float(format!("{}", quantize_to_1_64(value))));
    }
    out.push_str("],\"max\":[");
    for (axis, value) in [collider.max.x, collider.max.y, collider.max.z]
        .into_iter()
        .enumerate()
    {
        if axis > 0 {
            out.push(',');
        }
        out.push_str(&mark_float(format!("{}", quantize_to_1_64(value))));
    }
    match containing_cell(collider) {
        Some(cell) => {
            out.push_str("],\"block\":[");
            for (axis, block) in cell.iter().enumerate() {
                if axis > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{block}"));
            }
            out.push_str("]}");
        }
        None => out.push_str("],\"merged\":true}"),
    }
    out
}

/// Assembles one complete marker line from precomputed parts. Field order
/// is fixed so the collider array is always the final member and greedy
/// byte accounting has a constant two-byte tail.
#[allow(clippy::too_many_arguments)]
fn marker_line(
    phase: &str,
    feet: Vec3,
    player_extents: [f32; 3],
    probe_iterations: usize,
    probe_max_displacement_blocks: f64,
    total_sealing_count: usize,
    truncated: bool,
    sealing: &[String],
) -> String {
    let mut line = String::with_capacity(MARKER_BYTE_CAP.min(1024));
    line.push_str(MARKER_PREFIX);
    line.push_str("{\"schema\":\"");
    line.push_str(SCHEMA_TAG);
    line.push_str("\",\"phase\":\"");
    line.push_str(phase);
    line.push_str("\",\"feet\":[");
    for (axis, coordinate) in [feet.x, feet.y, feet.z].into_iter().enumerate() {
        if axis > 0 {
            line.push(',');
        }
        line.push_str(&mark_float(format!("{}", coordinate as f32)));
    }
    line.push_str("],\"player_extents\":[");
    for (axis, extent) in player_extents.into_iter().enumerate() {
        if axis > 0 {
            line.push(',');
        }
        line.push_str(&mark_float(format!("{extent}")));
    }
    line.push_str(&format!(
        "],\"iterations\":{probe_iterations},\"max_displacement_blocks\":{},\"overlap_free\":false,\"total_sealing_count\":{total_sealing_count},\"truncated\":{truncated},\"sealing\":[",
        mark_float(format!("{probe_max_displacement_blocks}")),
    ));
    for (index, entry) in sealing.iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        line.push_str(entry);
    }
    line.push_str("]}");
    line
}

/// Header length for byte-budget decisions. The truncated flag is measured
/// as `false` (the longer spelling) so the estimate never undercounts.
#[allow(clippy::too_many_arguments)]
fn header_length(
    phase: &str,
    feet: Vec3,
    player_extents: [f32; 3],
    probe_iterations: usize,
    probe_max_displacement_blocks: f64,
    total_sealing_count: usize,
) -> usize {
    marker_line(
        phase,
        feet,
        player_extents,
        probe_iterations,
        probe_max_displacement_blocks,
        total_sealing_count,
        false,
        &[],
    )
    .len()
}

/// Renders the bounded failure markers for one failed probe attempt.
///
/// Returns an empty vector whenever instrumentation is disabled. Otherwise
/// returns exactly one `phase:"failed"` line, plus one additional
/// `phase:"degraded"` line with otherwise identical content when the caller
/// reports that this attempt also spends the per-epoch failure budget. The
/// byte-fit decision is made against the longer degraded phase so both
/// emitted lines respect [`MARKER_BYTE_CAP`].
pub(super) fn failure_marker_lines(
    enabled: bool,
    feet: Vec3,
    colliders: &[Aabb],
    probe_iterations: usize,
    probe_max_displacement_blocks: f64,
    epoch_spent: bool,
) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    let player = Aabb::player_at(feet);
    let total_sealing_count = colliders
        .iter()
        .copied()
        .filter(|collider| is_reportable(*collider, player))
        .count();
    // A genuine failure always has at least one finite overlapping collider:
    // `depenetrate_player` reports success immediately when none overlap, and
    // it skips non-finite boxes itself. Zero reportable colliders therefore
    // cannot describe a real embedment and emit nothing.
    if total_sealing_count == 0 {
        return Vec::new();
    }
    let mut rendered = Vec::with_capacity(total_sealing_count.min(MAX_SEALING_COLLIDERS));
    for collider in colliders.iter().copied() {
        if rendered.len() >= MAX_SEALING_COLLIDERS {
            break;
        }
        if is_reportable(collider, player) {
            rendered.push(render_collider(collider));
        }
    }

    let box_ = Aabb::player_at(feet);
    let player_extents = [
        (box_.max.x - box_.min.x) as f32,
        (box_.max.y - box_.min.y) as f32,
        (box_.max.z - box_.min.z) as f32,
    ];
    // Measure against the longer degraded phase so both lines fit.
    let mut used = header_length(
        "degraded",
        feet,
        player_extents,
        probe_iterations,
        probe_max_displacement_blocks,
        total_sealing_count,
    );
    let mut included = 0_usize;
    for entry in &rendered {
        let candidate = entry.len() + usize::from(included > 0);
        if used + candidate + CLOSING_BYTES > MARKER_BYTE_CAP {
            break;
        }
        used += candidate;
        included += 1;
    }
    let truncated = included < total_sealing_count;
    let sealing = &rendered[..included];

    let mut lines = Vec::with_capacity(usize::from(epoch_spent) + 1);
    lines.push(marker_line(
        "failed",
        feet,
        player_extents,
        probe_iterations,
        probe_max_displacement_blocks,
        total_sealing_count,
        truncated,
        sealing,
    ));
    if epoch_spent {
        lines.push(marker_line(
            "degraded",
            feet,
            player_extents,
            probe_iterations,
            probe_max_displacement_blocks,
            total_sealing_count,
            truncated,
            sealing,
        ));
    }
    lines
}
