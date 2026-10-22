//! Camera-relative mesh admission; block data and lighting keep their ordinary lifetime.

use super::*;

#[derive(Default)]
pub(super) struct VerticalDetail {
    radius: Option<u32>,
    center_y: i32,
    /// Sections whose geometry was suppressed, including work not yet acknowledged.
    pub(super) suppressed: ColumnSubChunkSet,
    cursor: Option<SubChunkKey>,
    scanning: bool,
}

impl VerticalDetail {
    /// Includes the camera section and `radius` complete sections in each direction.
    pub(super) fn includes(&self, y: i32) -> bool {
        self.radius
            .is_none_or(|radius| self.center_y.abs_diff(y) <= radius)
    }
}

impl WorldStream {
    /// Sets the vertical mesh radius in sections; zero preserves unlimited height.
    /// A poll reconciles existing meshes under the ordinary maintenance budget.
    pub fn set_vertical_render_distance(&mut self, chunks: u32) {
        let radius =
            (chunks != render_api::terrain_distance::VERTICAL_DISTANCE_UNLIMITED).then_some(chunks);
        if self.vertical_detail.radius != radius {
            self.vertical_detail.radius = radius;
            self.vertical_detail.cursor = None;
            self.vertical_detail.scanning = true;
        }
    }

    /// Reports whether the next service slice has vertical admission work to perform.
    pub(super) fn has_pending_vertical_detail(&self) -> bool {
        self.vertical_detail.scanning
    }

    /// Combines horizontal detail membership with the current vertical radius.
    pub(super) fn wants_section_mesh(&self, key: SubChunkKey) -> bool {
        self.wants_mesh(key.chunk()) && self.vertical_detail.includes(key.y)
    }

    /// Reconciles one section with the current band, publishing a withdrawal only once.
    pub(super) fn reconcile_vertical_section(&mut self, key: SubChunkKey, now: Instant) {
        if self.vertical_detail.includes(key.y) {
            if self.vertical_detail.suppressed.remove(&key) {
                self.mark_dirty_exact(key, now);
            }
        } else if self.vertical_detail.suppressed.insert(key) {
            self.mark_dirty_exact_ungated(key, now, false);
        }
    }

    /// Resumes after each section; target changes restart discovery and reconcile against
    /// suppressed membership, so an interrupted scan cannot forget a previous withdrawal.
    pub(super) fn reevaluate_vertical_detail(&mut self, camera_position: [f32; 3]) {
        if self.vertical_detail.radius.is_none() && !self.vertical_detail.scanning {
            return;
        }
        if camera_position.iter().all(|value| value.is_finite()) {
            let y = floor_to_i32(camera_position[1]).div_euclid(world::SUB_CHUNK_SIDE as i32);
            if self.vertical_detail.center_y != y {
                self.vertical_detail.center_y = y;
                if self.vertical_detail.radius.is_some() {
                    self.vertical_detail.cursor = None;
                    self.vertical_detail.scanning = true;
                }
            }
        }
        if !self.vertical_detail.scanning {
            return;
        }
        let mut items = 0;
        while !self.maintenance_should_yield(items) {
            let Some(key) = self.resident.next_key(self.vertical_detail.cursor) else {
                self.vertical_detail.scanning = false;
                self.vertical_detail.cursor = None;
                break;
            };
            let started = Instant::now();
            self.vertical_detail.cursor = Some(key);
            self.reconcile_vertical_section(key, started);
            self.record_service_item(started);
            self.maintenance_items += 1;
            items += 1;
        }
    }
}
