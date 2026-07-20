use std::{error::Error, fmt, fmt::Write as _, mem::size_of};

use assets::AtmosphereTexture;
pub use assets::CloudQuality;
use meshing::{CLOUD_MASK_SIZE, MAX_CLOUD_BYTES, MAX_CLOUD_QUADS, PackedCloudQuad};

const MAX_COVERAGE_MILLIBLOCKS: u32 = 16_777_216;
const MAX_CAMERA_POSITION_MILLIBLOCKS: i64 = 64_000_000_000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CloudRenderConfig {
    quality: CloudQuality,
    grid_size: u8,
    mesh_size: u16,
    distance_scale: u8,
    distance_control: bool,
    lighting: bool,
}

impl CloudRenderConfig {
    #[must_use]
    pub const fn native(quality: CloudQuality) -> Self {
        let (grid_size, distance_scale) = match quality {
            CloudQuality::Low => (1, 2),
            CloudQuality::Medium => (2, 3),
            CloudQuality::High => (3, 3),
            CloudQuality::Ultra => (4, 3),
        };
        Self {
            quality,
            grid_size,
            mesh_size: 64,
            distance_scale,
            distance_control: true,
            lighting: true,
        }
    }

    #[must_use]
    pub const fn quality(self) -> CloudQuality {
        self.quality
    }

    #[must_use]
    pub const fn grid_size(self) -> u8 {
        self.grid_size
    }

    #[must_use]
    pub const fn mesh_size(self) -> u16 {
        self.mesh_size
    }

    #[must_use]
    pub const fn distance_scale(self) -> u8 {
        self.distance_scale
    }

    #[must_use]
    pub const fn distance_control(self) -> bool {
        self.distance_control
    }

    #[must_use]
    pub const fn lighting(self) -> bool {
        self.lighting
    }
}

impl Default for CloudRenderConfig {
    fn default() -> Self {
        Self::native(CloudQuality::default())
    }
}

/// Exact one-line evidence for the provisional cloud geometry currently sent
/// to the GPU.
///
/// This deliberately reports `calibrated=false`: the matching Bedrock config
/// names its grid, mesh, and distance controls, but matching-view evidence has
/// not yet established their world-space interpretation. Keeping the runtime
/// layout and the native values in one parser-stable record lets calibration
/// replace the provisional mapping without silently presenting it as parity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudGeometryDiagnostic {
    config: CloudRenderConfig,
    asset_identity_sha256: [u8; 32],
    occupied_texels: u32,
    quad_count: u32,
    quad_bytes: u32,
    instance_count: u8,
    texture_period_milliblocks: u32,
    underside_y_milliblocks: i32,
    top_y_milliblocks: i32,
}

impl CloudGeometryDiagnostic {
    #[allow(clippy::too_many_arguments)]
    pub fn from_runtime_layout(
        config: CloudRenderConfig,
        asset_identity_sha256: [u8; 32],
        texture: &AtmosphereTexture,
        records: &[PackedCloudQuad],
        instance_count: u8,
        texture_period_milliblocks: u32,
        underside_y_milliblocks: i32,
        top_y_milliblocks: i32,
    ) -> Result<Self, CloudGeometryDiagnosticError> {
        let occupied_texels = u32::try_from(
            texture
                .rgba8
                .chunks_exact(4)
                .filter(|texel| texel[3] >= 128)
                .count(),
        )
        .map_err(|_| CloudGeometryDiagnosticError::OccupancyCountOverflow)?;
        let quad_count = u32::try_from(records.len())
            .map_err(|_| CloudGeometryDiagnosticError::QuadCountOverflow)?;
        let quad_bytes = records
            .len()
            .checked_mul(size_of::<PackedCloudQuad>())
            .and_then(|bytes| u32::try_from(bytes).ok())
            .ok_or(CloudGeometryDiagnosticError::QuadByteOverflow)?;
        Self::try_new(
            config,
            asset_identity_sha256,
            occupied_texels,
            quad_count,
            quad_bytes,
            instance_count,
            texture_period_milliblocks,
            underside_y_milliblocks,
            top_y_milliblocks,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        config: CloudRenderConfig,
        asset_identity_sha256: [u8; 32],
        occupied_texels: u32,
        quad_count: u32,
        quad_bytes: u32,
        instance_count: u8,
        texture_period_milliblocks: u32,
        underside_y_milliblocks: i32,
        top_y_milliblocks: i32,
    ) -> Result<Self, CloudGeometryDiagnosticError> {
        if asset_identity_sha256 == [0; 32] {
            return Err(CloudGeometryDiagnosticError::InvalidAssetIdentity);
        }
        let max_occupied_texels = CLOUD_MASK_SIZE * CLOUD_MASK_SIZE;
        if occupied_texels > max_occupied_texels {
            return Err(CloudGeometryDiagnosticError::TooManyOccupiedTexels {
                actual: occupied_texels,
                max: max_occupied_texels,
            });
        }
        if usize::try_from(quad_count).map_or(true, |count| count > MAX_CLOUD_QUADS) {
            return Err(CloudGeometryDiagnosticError::TooManyQuads {
                actual: quad_count,
                max: MAX_CLOUD_QUADS as u32,
            });
        }
        let expected_quad_bytes = quad_count
            .checked_mul(size_of::<PackedCloudQuad>() as u32)
            .ok_or(CloudGeometryDiagnosticError::QuadByteOverflow)?;
        if quad_bytes != expected_quad_bytes {
            return Err(CloudGeometryDiagnosticError::InconsistentQuadBytes {
                actual: quad_bytes,
                expected: expected_quad_bytes,
            });
        }
        if quad_bytes as usize > MAX_CLOUD_BYTES {
            return Err(CloudGeometryDiagnosticError::TooManyQuadBytes {
                actual: quad_bytes,
                max: MAX_CLOUD_BYTES as u32,
            });
        }
        if instance_count == 0 {
            return Err(CloudGeometryDiagnosticError::InvalidInstanceCount);
        }
        if texture_period_milliblocks == 0
            || texture_period_milliblocks > MAX_COVERAGE_MILLIBLOCKS
            || underside_y_milliblocks >= top_y_milliblocks
            || i64::from(underside_y_milliblocks).unsigned_abs()
                > MAX_CAMERA_POSITION_MILLIBLOCKS as u64
            || i64::from(top_y_milliblocks).unsigned_abs() > MAX_CAMERA_POSITION_MILLIBLOCKS as u64
        {
            return Err(CloudGeometryDiagnosticError::InvalidWorldBounds);
        }
        Ok(Self {
            config,
            asset_identity_sha256,
            occupied_texels,
            quad_count,
            quad_bytes,
            instance_count,
            texture_period_milliblocks,
            underside_y_milliblocks,
            top_y_milliblocks,
        })
    }

    #[must_use]
    pub const fn config(&self) -> CloudRenderConfig {
        self.config
    }

    #[must_use]
    pub const fn occupied_texels(&self) -> u32 {
        self.occupied_texels
    }

    #[must_use]
    pub const fn quad_count(&self) -> u32 {
        self.quad_count
    }

    #[must_use]
    pub const fn quad_bytes(&self) -> u32 {
        self.quad_bytes
    }

    #[must_use]
    pub const fn instance_count(&self) -> u8 {
        self.instance_count
    }

    #[must_use]
    pub const fn texture_period_milliblocks(&self) -> u32 {
        self.texture_period_milliblocks
    }

    #[must_use]
    pub const fn underside_y_milliblocks(&self) -> i32 {
        self.underside_y_milliblocks
    }

    #[must_use]
    pub const fn top_y_milliblocks(&self) -> i32 {
        self.top_y_milliblocks
    }

    #[must_use]
    pub fn marker_fields(&self) -> String {
        let mut asset_identity = String::with_capacity(64);
        for byte in self.asset_identity_sha256 {
            write!(&mut asset_identity, "{byte:02x}").expect("writing to a String cannot fail");
        }
        format!(
            "calibrated=false quality={:?} occupied_texels={} quad_count={} quad_bytes={} \
             instance_count={} texture_period_milliblocks={} underside_y_milliblocks={} \
             top_y_milliblocks={} native_grid_size={} native_mesh_size={} \
             native_distance_scale={} native_distance_control={} native_lighting={} \
             asset_identity_sha256={asset_identity}",
            self.config.quality(),
            self.occupied_texels,
            self.quad_count,
            self.quad_bytes,
            self.instance_count,
            self.texture_period_milliblocks,
            self.underside_y_milliblocks,
            self.top_y_milliblocks,
            self.config.grid_size(),
            self.config.mesh_size(),
            self.config.distance_scale(),
            self.config.distance_control(),
            self.config.lighting(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloudGeometryDiagnosticError {
    InvalidAssetIdentity,
    OccupancyCountOverflow,
    TooManyOccupiedTexels { actual: u32, max: u32 },
    QuadCountOverflow,
    TooManyQuads { actual: u32, max: u32 },
    QuadByteOverflow,
    InconsistentQuadBytes { actual: u32, expected: u32 },
    TooManyQuadBytes { actual: u32, max: u32 },
    InvalidInstanceCount,
    InvalidWorldBounds,
}

impl fmt::Display for CloudGeometryDiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAssetIdentity => write!(formatter, "invalid cloud asset identity"),
            Self::OccupancyCountOverflow => write!(formatter, "cloud occupancy count overflowed"),
            Self::TooManyOccupiedTexels { actual, max } => {
                write!(
                    formatter,
                    "cloud occupancy has {actual} texels; limit is {max}"
                )
            }
            Self::TooManyQuads { actual, max } => {
                write!(
                    formatter,
                    "cloud geometry has {actual} quads; limit is {max}"
                )
            }
            Self::QuadCountOverflow => write!(formatter, "cloud quad count overflowed"),
            Self::QuadByteOverflow => write!(formatter, "cloud quad byte count overflowed"),
            Self::InconsistentQuadBytes { actual, expected } => write!(
                formatter,
                "cloud geometry reports {actual} quad bytes; expected {expected}"
            ),
            Self::TooManyQuadBytes { actual, max } => {
                write!(
                    formatter,
                    "cloud geometry has {actual} bytes; limit is {max}"
                )
            }
            Self::InvalidInstanceCount => write!(formatter, "invalid cloud instance count"),
            Self::InvalidWorldBounds => write!(formatter, "invalid cloud world-space bounds"),
        }
    }
}

impl Error for CloudGeometryDiagnosticError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CloudCoverageSemantics {
    mesh_size_world_milliblocks: u32,
    grid_size_world_milliblocks: u32,
    distance_scale_world_milliblocks: u32,
    coverage_radius_milliblocks: u32,
}

impl CloudCoverageSemantics {
    pub fn try_new(
        mesh_size_world_milliblocks: u32,
        grid_size_world_milliblocks: u32,
        distance_scale_world_milliblocks: u32,
        coverage_radius_milliblocks: u32,
    ) -> Result<Self, CloudCalibrationError> {
        let values = [
            mesh_size_world_milliblocks,
            grid_size_world_milliblocks,
            distance_scale_world_milliblocks,
            coverage_radius_milliblocks,
        ];
        if values
            .into_iter()
            .any(|value| value == 0 || value > MAX_COVERAGE_MILLIBLOCKS)
        {
            return Err(CloudCalibrationError::InvalidCoverageSemantics);
        }
        Ok(Self {
            mesh_size_world_milliblocks,
            grid_size_world_milliblocks,
            distance_scale_world_milliblocks,
            coverage_radius_milliblocks,
        })
    }

    #[must_use]
    pub const fn mesh_size_world_milliblocks(self) -> u32 {
        self.mesh_size_world_milliblocks
    }

    #[must_use]
    pub const fn grid_size_world_milliblocks(self) -> u32 {
        self.grid_size_world_milliblocks
    }

    #[must_use]
    pub const fn distance_scale_world_milliblocks(self) -> u32 {
        self.distance_scale_world_milliblocks
    }

    #[must_use]
    pub const fn coverage_radius_milliblocks(self) -> u32 {
        self.coverage_radius_milliblocks
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CloudMatchingView {
    quality: CloudQuality,
    camera_position_milliblocks: [i64; 3],
    yaw_millidegrees: i32,
    pitch_millidegrees: i32,
    native_capture_sha256: [u8; 32],
    client_capture_sha256: [u8; 32],
}

impl CloudMatchingView {
    pub fn try_new(
        quality: CloudQuality,
        camera_position_milliblocks: [i64; 3],
        yaw_millidegrees: i32,
        pitch_millidegrees: i32,
        native_capture_sha256: [u8; 32],
        client_capture_sha256: [u8; 32],
    ) -> Result<Self, CloudCalibrationError> {
        if camera_position_milliblocks
            .into_iter()
            .any(|coordinate| coordinate.unsigned_abs() > MAX_CAMERA_POSITION_MILLIBLOCKS as u64)
            || !(-180_000..=180_000).contains(&yaw_millidegrees)
            || !(-90_000..=90_000).contains(&pitch_millidegrees)
            || native_capture_sha256 == [0; 32]
            || client_capture_sha256 == [0; 32]
        {
            return Err(CloudCalibrationError::InvalidMatchingView);
        }
        Ok(Self {
            quality,
            camera_position_milliblocks,
            yaw_millidegrees,
            pitch_millidegrees,
            native_capture_sha256,
            client_capture_sha256,
        })
    }

    #[must_use]
    pub const fn quality(self) -> CloudQuality {
        self.quality
    }

    #[must_use]
    pub const fn camera_position_milliblocks(self) -> [i64; 3] {
        self.camera_position_milliblocks
    }

    #[must_use]
    pub const fn yaw_millidegrees(self) -> i32 {
        self.yaw_millidegrees
    }

    #[must_use]
    pub const fn pitch_millidegrees(self) -> i32 {
        self.pitch_millidegrees
    }

    #[must_use]
    pub const fn native_capture_sha256(self) -> [u8; 32] {
        self.native_capture_sha256
    }

    #[must_use]
    pub const fn client_capture_sha256(self) -> [u8; 32] {
        self.client_capture_sha256
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CloudCalibrationRecord {
    config: CloudRenderConfig,
    matching_view: CloudMatchingView,
    coverage_semantics: CloudCoverageSemantics,
}

impl CloudCalibrationRecord {
    #[must_use]
    pub const fn config(self) -> CloudRenderConfig {
        self.config
    }

    #[must_use]
    pub const fn matching_view(self) -> CloudMatchingView {
        self.matching_view
    }

    #[must_use]
    pub const fn coverage_semantics(self) -> CloudCoverageSemantics {
        self.coverage_semantics
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloudCalibrationReport {
    records: [CloudCalibrationRecord; 4],
}

impl CloudCalibrationReport {
    #[must_use]
    pub const fn records(&self) -> &[CloudCalibrationRecord; 4] {
        &self.records
    }

    #[must_use]
    pub const fn record(&self, quality: CloudQuality) -> CloudCalibrationRecord {
        self.records[quality as usize]
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CloudCalibrationHarness {
    matching_views: [Option<CloudMatchingView>; 4],
    coverage_semantics: [Option<CloudCoverageSemantics>; 4],
}

impl CloudCalibrationHarness {
    pub fn record_matching_view(
        &mut self,
        view: CloudMatchingView,
    ) -> Result<(), CloudCalibrationError> {
        let quality = view.quality();
        let slot = &mut self.matching_views[quality as usize];
        if slot.is_some() {
            return Err(CloudCalibrationError::DuplicateMatchingView { quality });
        }
        *slot = Some(view);
        Ok(())
    }

    pub fn record_coverage_semantics(
        &mut self,
        quality: CloudQuality,
        semantics: CloudCoverageSemantics,
    ) -> Result<(), CloudCalibrationError> {
        let slot = &mut self.coverage_semantics[quality as usize];
        if slot.is_some() {
            return Err(CloudCalibrationError::DuplicateCoverageSemantics { quality });
        }
        *slot = Some(semantics);
        Ok(())
    }

    pub fn publish(&self) -> Result<CloudCalibrationReport, CloudCalibrationError> {
        for quality in CloudQuality::ALL {
            if self.matching_views[quality as usize].is_none() {
                return Err(CloudCalibrationError::MissingMatchingView { quality });
            }
        }
        for quality in CloudQuality::ALL {
            if self.coverage_semantics[quality as usize].is_none() {
                return Err(CloudCalibrationError::UncalibratedMapping { quality });
            }
        }

        let records = CloudQuality::ALL.map(|quality| CloudCalibrationRecord {
            config: CloudRenderConfig::native(quality),
            matching_view: self.matching_views[quality as usize]
                .expect("matching views were checked above"),
            coverage_semantics: self.coverage_semantics[quality as usize]
                .expect("coverage semantics were checked above"),
        });
        Ok(CloudCalibrationReport { records })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloudCalibrationError {
    InvalidCoverageSemantics,
    InvalidMatchingView,
    DuplicateMatchingView { quality: CloudQuality },
    DuplicateCoverageSemantics { quality: CloudQuality },
    MissingMatchingView { quality: CloudQuality },
    UncalibratedMapping { quality: CloudQuality },
}

impl fmt::Display for CloudCalibrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCoverageSemantics => write!(formatter, "invalid cloud coverage semantics"),
            Self::InvalidMatchingView => write!(formatter, "invalid cloud matching-view evidence"),
            Self::DuplicateMatchingView { quality } => {
                write!(formatter, "duplicate {quality:?} cloud matching view")
            }
            Self::DuplicateCoverageSemantics { quality } => {
                write!(formatter, "duplicate {quality:?} cloud coverage semantics")
            }
            Self::MissingMatchingView { quality } => {
                write!(formatter, "missing {quality:?} cloud matching view")
            }
            Self::UncalibratedMapping { quality } => {
                write!(formatter, "uncalibrated {quality:?} cloud coverage mapping")
            }
        }
    }
}

impl Error for CloudCalibrationError {}
