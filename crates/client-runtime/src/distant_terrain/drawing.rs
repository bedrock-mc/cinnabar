//! The frame's side of drawing distant terrain: hands the renderer its settings, keeps
//! selection running on the stream's mesher, feeds it the view, and passes one draw update,
//! the colour tables and the resident chunk columns at a time through the
//! [`DistantTerrainLink`].

use std::{sync::Arc, time::Duration};

use chunk_pipeline::{DistantTerrainColours, DistantTerrainView, WorldStream};
use lod::{
    mesh::output::CLUSTER_QUADS,
    select::{MAX_DISTANCE_CHUNKS, focal_px, policy::RESIDENT_WALK_INTERVAL},
};
use render_api::DistantTerrainTier;
use render_scene::distant_terrain::{
    DistantTerrainColourTables, DistantTerrainLink, DistantTerrainPass,
    DistantTerrainRenderSettings, DistantTerrainRenderStats, RESIDENT_RECENTRE_CHUNKS,
    ResidentColumnMask,
};

const _: () = assert!(
    DistantTerrainTier::DESKTOP.max_distance_chunks <= MAX_DISTANCE_CHUNKS,
    "selection walks every distance the settings offer"
);

/// The camera distant terrain is selected for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawCamera {
    /// Eye position in blocks.
    pub eye: [f64; 3],
    /// Height of the drawn viewport in pixels.
    pub viewport_height_px: f64,
    /// The projection's vertical field of view.
    pub vertical_fov_radians: f64,
}

/// What one frame hands the driver.
pub struct DrawFrame<'a> {
    /// The session's stream, when it is on this thread.
    pub stream: Option<&'a mut WorldStream>,
    /// The Video setting's choices; `None` while distant terrain is off.
    pub settings: Option<DistantTerrainRenderSettings>,
    pub camera: Option<DrawCamera>,
    /// Monotonic time since an origin the caller keeps fixed; paces selection walks.
    pub uptime: Duration,
    /// Seconds the last frame took; paces uploads.
    pub frame_seconds: f32,
}

/// The [`diagnostics::markers::LOD_RENDER`] line of `stats`: each counter as `name=value`,
/// the size histograms as comma-separated bins, then each pass's GPU milliseconds.
#[must_use]
pub fn render_marker_line(stats: &DistantTerrainRenderStats) -> String {
    use std::fmt::Write as _;
    let bins = |bins: &[u32]| {
        bins.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut line = format!(
        "{marker} supported={} ready={} max_distance_chunks={} hiz={} sections={} clusters={} pool_used_bytes={} pool_bytes={} uploaded_bytes={} upload_budget={} committed_updates={} resets={} visible_clusters={} pass_b_clusters={} pass_b_share={:.3} translucent_clusters={} list_overflow={} dropped_sections={} section_size_bins={} cluster_size_bins={} degenerate_share={:.3}",
        stats.supported.unwrap_or(false),
        stats.ready,
        stats.tier.map_or(0, |tier| tier.max_distance_chunks),
        stats.hiz,
        stats.sections,
        stats.clusters,
        stats.pool_used_bytes,
        stats.pool_bytes,
        stats.uploaded_bytes,
        stats.upload_budget,
        stats.committed_updates,
        stats.resets_requested,
        stats.visible_clusters,
        stats.pass_b_clusters,
        stats.pass_b_share(),
        stats.translucent_clusters,
        stats.list_overflow,
        stats.dropped_sections,
        bins(&stats.section_size_bins),
        bins(&stats.cluster_size_bins),
        stats.degenerate_share(CLUSTER_QUADS as u32),
        marker = diagnostics::markers::LOD_RENDER,
    );
    for (pass, ms) in DistantTerrainPass::ALL.iter().zip(stats.gpu_ms) {
        let _ = write!(line, " gpu_{}_ms={ms:.3}", pass.name());
    }
    line
}

/// See the module documentation.
pub struct DistantTerrainDrawing {
    link: DistantTerrainLink,
    /// Selection runs on the stream for the renderer's current contents.
    selecting: bool,
    /// The renderer's reset count last answered.
    resets_seen: u64,
    colours: Option<DistantTerrainColours>,
    /// Resident column changes, the centre and the uptime of the last mask sent.
    resident: Option<(u64, [i32; 2], Duration)>,
    /// The drawing distance in chunks selection runs for, within the device's tier.
    distance_chunks: Option<u32>,
    /// The renderer was last handed settings, not `None`.
    handed: bool,
}

impl DistantTerrainDrawing {
    /// A driver handing over through `link`.
    #[must_use]
    pub fn new(link: DistantTerrainLink) -> Self {
        Self {
            link,
            selecting: false,
            resets_seen: 0,
            colours: None,
            resident: None,
            distance_chunks: None,
            handed: false,
        }
    }

    /// True when the device cannot draw distant terrain; unknown counts as able.
    #[must_use]
    pub fn unsupported(&self) -> bool {
        self.link.stats().supported == Some(false)
    }

    /// True while distant terrain is drawn: the setting is on, the device can draw it and
    /// selection runs.
    #[must_use]
    pub fn drawing(&self) -> bool {
        self.selecting
    }

    /// True once nothing runs and the renderer was told to draw nothing: while the setting
    /// stays off, frames need not call [`Self::update`].
    #[must_use]
    pub fn idle(&self) -> bool {
        !self.selecting && !self.handed
    }

    /// The distance in chunks distant terrain is drawn to: `Some` only once the renderer
    /// is ready and draws sections, so fog moves out no earlier than the terrain it reveals.
    #[must_use]
    pub fn drawn_distance_chunks(&self) -> Option<u32> {
        let stats = self.link.stats();
        self.distance_chunks
            .filter(|_| self.selecting && stats.ready && stats.sections != 0)
    }

    /// Runs one frame; see the module documentation.
    pub fn update(&mut self, frame: DrawFrame<'_>) {
        self.link.set_frame_seconds(frame.frame_seconds);
        let stats = self.link.stats();
        let settings = frame.settings.filter(|_| stats.supported != Some(false));
        self.link.set_settings(settings);
        self.handed = settings.is_some();
        let Some(stream) = frame.stream else {
            self.stop(None);
            return;
        };
        let Some(settings) = settings else {
            self.stop(Some(stream));
            return;
        };
        // A renderer on a low-end adapter of a desktop build caps the distance further.
        let distance_chunks = stats
            .tier
            .unwrap_or(DistantTerrainTier::CURRENT)
            .clamp_distance(settings.distance_chunks);
        let meshing = stream
            .distant_terrain_mesh_stats()
            .is_some_and(|stats| !stats.stopped);
        if !meshing {
            self.stop(Some(stream));
            return;
        }
        let selection = stream.distant_terrain_selection_stats();
        let restart = !self.selecting
            || selection.is_none_or(|stats| stats.stopped)
            || stats.resets_requested != self.resets_seen;
        if restart {
            self.resets_seen = stats.resets_requested;
            self.link.reset();
            self.resident = None;
            self.colours = None;
            self.selecting = match stream.enable_distant_terrain_selection() {
                Ok(()) => true,
                Err(error) => {
                    bevy::log::warn!(%error, "distant terrain selection unavailable");
                    false
                }
            };
            if !self.selecting {
                return;
            }
        }
        if let Some(camera) = frame.camera {
            stream.set_distant_terrain_view(
                DistantTerrainView {
                    camera: camera.eye,
                    focal_px: focal_px(camera.viewport_height_px, camera.vertical_fov_radians),
                    quality: settings.quality,
                    distance_chunks,
                },
                frame.uptime,
            );
            self.send_resident(stream, camera.eye, frame.uptime);
        }
        self.distance_chunks = Some(distance_chunks);
        self.send_colours(stream);
        if self.link.ready_for_update()
            && let Some(update) = stream.take_distant_terrain_update()
        {
            self.link.push_update(update);
        }
    }

    /// Stops selection and empties the renderer.
    fn stop(&mut self, stream: Option<&mut WorldStream>) {
        if !self.selecting {
            return;
        }
        if let Some(stream) = stream {
            stream.disable_distant_terrain_selection();
        }
        self.selecting = false;
        self.distance_chunks = None;
        self.link.reset();
    }

    /// Sends the colour tables when the mesher built new ones.
    fn send_colours(&mut self, stream: &WorldStream) {
        let Some(colours) = stream.distant_terrain_colours() else {
            return;
        };
        let same = self.colours.as_ref().is_some_and(|sent| {
            Arc::ptr_eq(&sent.materials, &colours.materials)
                && match (&sent.biomes, &colours.biomes) {
                    (Some(sent), Some(new)) => Arc::ptr_eq(sent, new),
                    (None, None) => true,
                    _ => false,
                }
        });
        if same {
            return;
        }
        self.link.set_colours(DistantTerrainColourTables {
            materials: Arc::clone(&colours.materials),
            biomes: colours.biomes.clone(),
        });
        self.colours = Some(colours);
    }

    /// Sends the resident chunk columns around `eye` when the player moved
    /// [`RESIDENT_RECENTRE_CHUNKS`] from the last centre, or when they changed and
    /// [`RESIDENT_WALK_INTERVAL`] passed since the last mask, the pace selection follows
    /// them at.
    fn send_resident(&mut self, stream: &WorldStream, eye: [f64; 3], uptime: Duration) {
        let centre = ResidentColumnMask::column_of([eye[0], eye[2]]);
        let changes = stream.resident_column_changes();
        let current = self.resident.is_some_and(|(sent, at, when)| {
            (sent == changes || uptime.saturating_sub(when) < RESIDENT_WALK_INTERVAL)
                && (0..2).all(|axis| (centre[axis] - at[axis]).abs() < RESIDENT_RECENTRE_CHUNKS)
        });
        if current {
            return;
        }
        self.link.set_resident(Arc::new(ResidentColumnMask::new(
            centre,
            stream.resident_chunk_columns(),
        )));
        self.resident = Some((changes, centre, uptime));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_render_marker_names_every_counter_and_pass() {
        let mut section_size_bins = [0; render_scene::distant_terrain::DISTANT_TERRAIN_SIZE_BINS];
        section_size_bins[2] = 7;
        let line = render_marker_line(&DistantTerrainRenderStats {
            visible_clusters: 10,
            pass_b_clusters: 5,
            degenerate_slots: CLUSTER_QUADS as u32 * 5,
            section_size_bins,
            ..Default::default()
        });
        assert!(line.starts_with(diagnostics::markers::LOD_RENDER));
        assert!(line.contains(" pass_b_share=0.500"));
        assert!(line.contains(" degenerate_share=0.500"), "{line}");
        assert!(
            line.contains(" section_size_bins=0,0,7,0,0,0,0,0 "),
            "{line}"
        );
        assert!(line.contains(" hiz=false "), "{line}");
        for pass in DistantTerrainPass::ALL {
            assert!(
                line.contains(&format!(" gpu_{}_ms=", pass.name())),
                "{line}"
            );
        }
    }
}
