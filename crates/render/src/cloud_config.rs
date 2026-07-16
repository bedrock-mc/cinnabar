use std::{error::Error, fmt};

const MAX_COVERAGE_MILLIBLOCKS: u32 = 16_777_216;
const MAX_CAMERA_POSITION_MILLIBLOCKS: i64 = 64_000_000_000;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum CloudQuality {
    Low,
    Medium,
    #[default]
    High,
    Ultra,
}

impl CloudQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    const fn index(self) -> usize {
        self as usize
    }
}

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
        self.records[quality.index()]
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
        let slot = &mut self.matching_views[quality.index()];
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
        let slot = &mut self.coverage_semantics[quality.index()];
        if slot.is_some() {
            return Err(CloudCalibrationError::DuplicateCoverageSemantics { quality });
        }
        *slot = Some(semantics);
        Ok(())
    }

    pub fn publish(&self) -> Result<CloudCalibrationReport, CloudCalibrationError> {
        for quality in CloudQuality::ALL {
            if self.matching_views[quality.index()].is_none() {
                return Err(CloudCalibrationError::MissingMatchingView { quality });
            }
        }
        for quality in CloudQuality::ALL {
            if self.coverage_semantics[quality.index()].is_none() {
                return Err(CloudCalibrationError::UncalibratedMapping { quality });
            }
        }

        let records = CloudQuality::ALL.map(|quality| CloudCalibrationRecord {
            config: CloudRenderConfig::native(quality),
            matching_view: self.matching_views[quality.index()]
                .expect("matching views were checked above"),
            coverage_semantics: self.coverage_semantics[quality.index()]
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
