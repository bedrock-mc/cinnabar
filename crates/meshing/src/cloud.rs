use std::{error::Error, fmt, mem::size_of};

use assets::{AtmosphereRole, AtmosphereTexture};

pub const CLOUD_MASK_SIZE: u32 = 256;
pub const CLOUD_UNDERSIDE_Y: f32 = 128.0;
pub const CLOUD_TOP_Y: f32 = 132.0;
pub const MAX_CLOUD_QUADS: usize = (CLOUD_MASK_SIZE as usize * CLOUD_MASK_SIZE as usize / 2) * 6;
pub const MAX_CLOUD_BYTES: usize = MAX_CLOUD_QUADS * size_of::<PackedCloudQuad>();

const MASK_SIDE: usize = CLOUD_MASK_SIZE as usize;
const MASK_WORDS: usize = MASK_SIDE / u64::BITS as usize;
const CLOUD_VERTICAL_EXTENT: u16 = (CLOUD_TOP_Y - CLOUD_UNDERSIDE_Y) as u16;
const FACE_COUNT: usize = 6;

type MaskRows = [[u64; MASK_WORDS]; MASK_SIDE];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum CloudFace {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl CloudFace {
    const fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::North),
            3 => Some(Self::South),
            4 => Some(Self::West),
            5 => Some(Self::East),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct PackedCloudQuad {
    pub bounds: u32,
    pub face_and_axis: u32,
}

impl PackedCloudQuad {
    #[must_use]
    pub fn try_pack(
        axis0_start: u16,
        axis1_start: u16,
        axis0_extent: u16,
        axis1_extent: u16,
        face: CloudFace,
    ) -> Option<Self> {
        if axis0_start >= CLOUD_MASK_SIZE as u16
            || axis1_start >= CLOUD_MASK_SIZE as u16
            || !(1..=CLOUD_MASK_SIZE as u16).contains(&axis0_extent)
            || !(1..=CLOUD_MASK_SIZE as u16).contains(&axis1_extent)
        {
            return None;
        }
        Some(Self {
            bounds: u32::from(axis0_start)
                | (u32::from(axis1_start) << 8)
                | (u32::from(axis0_extent - 1) << 16)
                | (u32::from(axis1_extent - 1) << 24),
            face_and_axis: face as u32,
        })
    }

    #[must_use]
    pub const fn try_from_words(words: [u32; 2]) -> Option<Self> {
        if words[1] & !0b111 != 0 || CloudFace::from_u32(words[1]).is_none() {
            return None;
        }
        Some(Self {
            bounds: words[0],
            face_and_axis: words[1],
        })
    }

    #[must_use]
    pub const fn words(self) -> [u32; 2] {
        [self.bounds, self.face_and_axis]
    }

    #[must_use]
    pub const fn axis0_start(self) -> u16 {
        (self.bounds & 0xff) as u16
    }

    #[must_use]
    pub const fn axis1_start(self) -> u16 {
        ((self.bounds >> 8) & 0xff) as u16
    }

    #[must_use]
    pub const fn axis0_extent(self) -> u16 {
        ((self.bounds >> 16) & 0xff) as u16 + 1
    }

    #[must_use]
    pub const fn axis1_extent(self) -> u16 {
        ((self.bounds >> 24) & 0xff) as u16 + 1
    }

    #[must_use]
    pub const fn face(self) -> CloudFace {
        match CloudFace::from_u32(self.face_and_axis) {
            Some(face) => face,
            None => panic!("invalid packed cloud face"),
        }
    }
}

const _: () = assert!(size_of::<PackedCloudQuad>() == 8);
const _: () = assert!(MAX_CLOUD_QUADS == 196_608);
const _: () = assert!(MAX_CLOUD_BYTES == 1_572_864);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloudMeshError {
    WrongRole { actual: AtmosphereRole },
    WrongDimensions { width: u32, height: u32 },
    WrongByteLength { actual: usize, expected: usize },
    CapacityOverflow,
    TooManyQuads { actual: usize, max: usize },
    TooManyBytes { actual: usize, max: usize },
}

impl fmt::Display for CloudMeshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongRole { actual } => {
                write!(
                    formatter,
                    "expected cloud atmosphere texture, got {actual:?}"
                )
            }
            Self::WrongDimensions { width, height } => write!(
                formatter,
                "cloud occupancy texture is {width}x{height}; expected 256x256"
            ),
            Self::WrongByteLength { actual, expected } => write!(
                formatter,
                "cloud occupancy texture has {actual} RGBA bytes; expected {expected}"
            ),
            Self::CapacityOverflow => write!(formatter, "cloud mesh capacity arithmetic overflow"),
            Self::TooManyQuads { actual, max } => {
                write!(formatter, "cloud mesh has {actual} quads; limit is {max}")
            }
            Self::TooManyBytes { actual, max } => {
                write!(formatter, "cloud mesh has {actual} bytes; limit is {max}")
            }
        }
    }
}

impl Error for CloudMeshError {}

pub fn mesh_cloud_texture(
    texture: &AtmosphereTexture,
) -> Result<Box<[PackedCloudQuad]>, CloudMeshError> {
    validate_texture(texture)?;
    validate_worst_case_limits()?;

    let mut occupancy = Box::new([[0_u64; MASK_WORDS]; MASK_SIDE]);
    for z in 0..MASK_SIDE {
        for x in 0..MASK_SIDE {
            let alpha = texture.rgba8[(z * MASK_SIDE + x) * 4 + 3];
            if alpha >= 128 {
                set_mask_bit(&mut occupancy, x, z);
            }
        }
    }

    let mut quads = Vec::new();
    append_horizontal_quads(&occupancy, CloudFace::Down, &mut quads)?;
    append_horizontal_quads(&occupancy, CloudFace::Up, &mut quads)?;
    append_side_quads(&occupancy, CloudFace::North, &mut quads)?;
    append_side_quads(&occupancy, CloudFace::South, &mut quads)?;
    append_side_quads(&occupancy, CloudFace::West, &mut quads)?;
    append_side_quads(&occupancy, CloudFace::East, &mut quads)?;

    let byte_count = quads
        .len()
        .checked_mul(size_of::<PackedCloudQuad>())
        .ok_or(CloudMeshError::CapacityOverflow)?;
    if byte_count > MAX_CLOUD_BYTES {
        return Err(CloudMeshError::TooManyBytes {
            actual: byte_count,
            max: MAX_CLOUD_BYTES,
        });
    }
    Ok(quads.into_boxed_slice())
}

#[must_use]
pub fn cloud_instance_origins(camera_xz: [f64; 2], offset_blocks: f64) -> [[f32; 2]; 9] {
    let period = f64::from(CLOUD_MASK_SIZE);
    let offset = if offset_blocks.is_finite() {
        offset_blocks.rem_euclid(period)
    } else {
        0.0
    };
    let camera_x = bounded_coordinate(camera_xz[0]);
    let camera_z = bounded_coordinate(camera_xz[1]);
    let center_x = ((camera_x - offset) / period).floor() * period + offset;
    let center_z = (camera_z / period).floor() * period;

    let mut origins = [[0.0; 2]; 9];
    let mut index = 0;
    for row in -1..=1 {
        for column in -1..=1 {
            origins[index] = [
                finite_f32(center_x + f64::from(column) * period),
                finite_f32(center_z + f64::from(row) * period),
            ];
            index += 1;
        }
    }
    origins
}

fn validate_texture(texture: &AtmosphereTexture) -> Result<(), CloudMeshError> {
    if texture.role != AtmosphereRole::Clouds {
        return Err(CloudMeshError::WrongRole {
            actual: texture.role,
        });
    }
    if texture.width != CLOUD_MASK_SIZE || texture.height != CLOUD_MASK_SIZE {
        return Err(CloudMeshError::WrongDimensions {
            width: texture.width,
            height: texture.height,
        });
    }
    let expected = MASK_SIDE
        .checked_mul(MASK_SIDE)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(CloudMeshError::CapacityOverflow)?;
    if texture.rgba8.len() != expected {
        return Err(CloudMeshError::WrongByteLength {
            actual: texture.rgba8.len(),
            expected,
        });
    }
    Ok(())
}

fn validate_worst_case_limits() -> Result<(), CloudMeshError> {
    let cells = MASK_SIDE
        .checked_mul(MASK_SIDE)
        .ok_or(CloudMeshError::CapacityOverflow)?;
    let quads = (cells / 2)
        .checked_mul(FACE_COUNT)
        .ok_or(CloudMeshError::CapacityOverflow)?;
    if quads > MAX_CLOUD_QUADS {
        return Err(CloudMeshError::TooManyQuads {
            actual: quads,
            max: MAX_CLOUD_QUADS,
        });
    }
    let bytes = quads
        .checked_mul(size_of::<PackedCloudQuad>())
        .ok_or(CloudMeshError::CapacityOverflow)?;
    if bytes > MAX_CLOUD_BYTES {
        return Err(CloudMeshError::TooManyBytes {
            actual: bytes,
            max: MAX_CLOUD_BYTES,
        });
    }
    Ok(())
}

fn append_horizontal_quads(
    occupancy: &MaskRows,
    face: CloudFace,
    quads: &mut Vec<PackedCloudQuad>,
) -> Result<(), CloudMeshError> {
    let mut remaining = *occupancy;
    for axis1_start in 0..MASK_SIDE {
        for axis0_start in 0..MASK_SIDE {
            if !mask_bit(&remaining, axis0_start, axis1_start) {
                continue;
            }
            let mut axis0_extent = 1;
            while axis0_start + axis0_extent < MASK_SIDE
                && mask_bit(&remaining, axis0_start + axis0_extent, axis1_start)
            {
                axis0_extent += 1;
            }
            let mut axis1_extent = 1;
            'rows: while axis1_start + axis1_extent < MASK_SIDE {
                for x in axis0_start..axis0_start + axis0_extent {
                    if !mask_bit(&remaining, x, axis1_start + axis1_extent) {
                        break 'rows;
                    }
                }
                axis1_extent += 1;
            }
            for z in axis1_start..axis1_start + axis1_extent {
                for x in axis0_start..axis0_start + axis0_extent {
                    clear_mask_bit(&mut remaining, x, z);
                }
            }
            push_quad(
                quads,
                axis0_start,
                axis1_start,
                axis0_extent,
                axis1_extent,
                face,
            )?;
        }
    }
    Ok(())
}

fn append_side_quads(
    occupancy: &MaskRows,
    face: CloudFace,
    quads: &mut Vec<PackedCloudQuad>,
) -> Result<(), CloudMeshError> {
    let mut exposed = [[0_u64; MASK_WORDS]; MASK_SIDE];
    for z in 0..MASK_SIDE {
        for x in 0..MASK_SIDE {
            if !mask_bit(occupancy, x, z) {
                continue;
            }
            let (neighbour_x, neighbour_z, plane, run) = match face {
                CloudFace::North => (x, wrap_previous(z), z, x),
                CloudFace::South => (x, wrap_next(z), wrap_next(z), x),
                CloudFace::West => (wrap_previous(x), z, x, z),
                CloudFace::East => (wrap_next(x), z, wrap_next(x), z),
                CloudFace::Down | CloudFace::Up => unreachable!("horizontal face in side mesher"),
            };
            if !mask_bit(occupancy, neighbour_x, neighbour_z) {
                set_mask_bit(&mut exposed, run, plane);
            }
        }
    }

    for plane in 0..MASK_SIDE {
        let mut run = 0;
        while run < MASK_SIDE {
            if !mask_bit(&exposed, run, plane) {
                run += 1;
                continue;
            }
            let mut extent = 1;
            while run + extent < MASK_SIDE && mask_bit(&exposed, run + extent, plane) {
                extent += 1;
            }
            push_quad(
                quads,
                run,
                plane,
                extent,
                usize::from(CLOUD_VERTICAL_EXTENT),
                face,
            )?;
            run += extent;
        }
    }
    Ok(())
}

fn push_quad(
    quads: &mut Vec<PackedCloudQuad>,
    axis0_start: usize,
    axis1_start: usize,
    axis0_extent: usize,
    axis1_extent: usize,
    face: CloudFace,
) -> Result<(), CloudMeshError> {
    let next_count = quads
        .len()
        .checked_add(1)
        .ok_or(CloudMeshError::CapacityOverflow)?;
    if next_count > MAX_CLOUD_QUADS {
        return Err(CloudMeshError::TooManyQuads {
            actual: next_count,
            max: MAX_CLOUD_QUADS,
        });
    }
    let quad = PackedCloudQuad::try_pack(
        axis0_start as u16,
        axis1_start as u16,
        axis0_extent as u16,
        axis1_extent as u16,
        face,
    )
    .ok_or(CloudMeshError::CapacityOverflow)?;
    quads.push(quad);
    Ok(())
}

fn mask_bit(mask: &MaskRows, x: usize, z: usize) -> bool {
    mask[z][x / u64::BITS as usize] & (1_u64 << (x % u64::BITS as usize)) != 0
}

fn set_mask_bit(mask: &mut MaskRows, x: usize, z: usize) {
    mask[z][x / u64::BITS as usize] |= 1_u64 << (x % u64::BITS as usize);
}

fn clear_mask_bit(mask: &mut MaskRows, x: usize, z: usize) {
    mask[z][x / u64::BITS as usize] &= !(1_u64 << (x % u64::BITS as usize));
}

const fn wrap_previous(value: usize) -> usize {
    if value == 0 { MASK_SIDE - 1 } else { value - 1 }
}

const fn wrap_next(value: usize) -> usize {
    if value + 1 == MASK_SIDE { 0 } else { value + 1 }
}

fn bounded_coordinate(value: f64) -> f64 {
    let safe_limit = f64::from(f32::MAX) / 2.0;
    if value.is_finite() && value.abs() <= safe_limit {
        value
    } else {
        0.0
    }
}

fn finite_f32(value: f64) -> f32 {
    let clamped = value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX));
    clamped as f32
}
