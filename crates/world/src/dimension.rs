/// Vanilla's loading-area anchor when the transfer position lies outside dimension height.
#[must_use]
pub const fn dimension_loading_fallback_y(dimension: i32) -> i32 {
    if dimension == 2 { 50 } else { 0 }
}
