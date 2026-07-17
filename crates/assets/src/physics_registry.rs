use std::collections::HashMap;

use bitflags::bitflags;
use sha2::{Digest, Sha256};

use crate::{AssetError, CollisionBox, RegistryRecord, read_registry};

const MAGIC: &[u8; 8] = b"PREG1001";
const PROTOCOL: u32 = 1001;
const HEADER_BYTES: usize = 48;
const TRAILER_BYTES: usize = 32;
const MAX_RECORDS: usize = 65_536;
const MAX_BOXES: usize = 32;
const SCALE: f32 = 100_000_000.0;

bitflags! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct BlockPhysicsFlags: u8 {
        const CLIMBABLE = 1 << 0;
        const WATER = 1 << 1;
        const LAVA = 1 << 2;
        const COBWEB = 1 << 3;
        const POWDER_SNOW = 1 << 4;
        const SCAFFOLDING = 1 << 5;
        const PASSABLE = 1 << 6;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SurfaceResponse {
    #[default]
    None = 0,
    Slime = 1,
    Bed = 2,
    Honey = 3,
    SoulSand = 4,
    BubbleUp = 5,
    BubbleDown = 6,
}

impl SurfaceResponse {
    fn read(value: u8) -> Result<Self, AssetError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Slime),
            2 => Ok(Self::Bed),
            3 => Ok(Self::Honey),
            4 => Ok(Self::SoulSand),
            5 => Ok(Self::BubbleUp),
            6 => Ok(Self::BubbleDown),
            _ => invalid(format!("unknown surface response {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockPhysicsRecord {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub boxes: Box<[CollisionBox]>,
    pub friction_q1e8: u32,
    pub horizontal_speed_q1e8: u32,
    pub vertical_speed_q1e8: u32,
    pub fluid_height_q1e8: i32,
    pub flags: BlockPhysicsFlags,
    pub surface_response: SurfaceResponse,
}

impl BlockPhysicsRecord {
    #[must_use]
    pub fn friction(&self) -> f32 {
        self.friction_q1e8 as f32 / SCALE
    }

    #[must_use]
    pub fn horizontal_speed_factor(&self) -> f32 {
        self.horizontal_speed_q1e8 as f32 / SCALE
    }

    #[must_use]
    pub fn vertical_speed_factor(&self) -> f32 {
        self.vertical_speed_q1e8 as f32 / SCALE
    }

    #[must_use]
    pub fn fluid_height_blocks(&self) -> f32 {
        self.fluid_height_q1e8 as f32 / SCALE
    }
}

#[derive(Debug, Clone)]
pub struct PhysicsRegistry {
    records: Box<[BlockPhysicsRecord]>,
    sequential: Box<[usize]>,
    hashes: HashMap<u32, usize>,
    sha256: [u8; 32],
    breg_sha256: [u8; 32],
}

impl PhysicsRegistry {
    #[must_use]
    pub fn by_sequential_id(&self, id: u32) -> Option<&BlockPhysicsRecord> {
        let index = *self.sequential.get(usize::try_from(id).ok()?)?;
        self.records.get(index)
    }

    #[must_use]
    pub fn by_network_hash(&self, hash: u32) -> Option<&BlockPhysicsRecord> {
        self.hashes
            .get(&hash)
            .and_then(|index| self.records.get(*index))
    }

    #[must_use]
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    #[must_use]
    pub const fn breg_sha256(&self) -> [u8; 32] {
        self.breg_sha256
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

pub fn read_physics_registry(
    preg: &[u8],
    breg: &[u8],
    records: &[RegistryRecord],
) -> Result<PhysicsRegistry, AssetError> {
    if preg.len() < HEADER_BYTES + TRAILER_BYTES {
        return invalid("carrier is shorter than its header and digest");
    }
    let body_len = preg.len() - TRAILER_BYTES;
    let expected_digest: [u8; 32] = preg[body_len..]
        .try_into()
        .expect("trailer length was checked");
    let actual_digest: [u8; 32] = Sha256::digest(&preg[..body_len]).into();
    if actual_digest != expected_digest {
        return invalid("carrier SHA-256 mismatch");
    }

    let mut reader = Reader::new(&preg[..body_len]);
    if reader.read_exact(8, "magic")? != MAGIC {
        return invalid("invalid magic");
    }
    if reader.read_u32("protocol")? != PROTOCOL {
        return invalid("protocol is not 1001");
    }
    let count = usize::try_from(reader.read_u32("record count")?)
        .map_err(|_| physics_error("record count does not fit usize"))?;
    if count > MAX_RECORDS {
        return invalid(format!("record count {count} exceeds {MAX_RECORDS}"));
    }
    if count != records.len() {
        return invalid(format!(
            "record count {count} does not match BREG records {}",
            records.len()
        ));
    }
    let encoded_breg_sha: [u8; 32] = reader
        .read_exact(32, "BREG SHA-256")?
        .try_into()
        .expect("fixed digest length");
    let actual_breg_sha: [u8; 32] = Sha256::digest(breg).into();
    if encoded_breg_sha != actual_breg_sha {
        return invalid("exact BREG SHA-256 mismatch");
    }
    let decoded_breg = read_registry(breg)?;
    if decoded_breg.as_ref() != records {
        return invalid("supplied records are not the exact decoded BREG records");
    }

    let mut decoded = Vec::with_capacity(count);
    let mut sequential = vec![usize::MAX; count];
    let mut hashes = HashMap::with_capacity(count);
    for (index, sequential_slot) in sequential.iter_mut().enumerate() {
        let sequential_id = reader.read_u32("sequential ID")?;
        let network_hash = reader.read_u32("network hash")?;
        let box_count = usize::from(reader.read_u8("box count")?);
        if box_count > MAX_BOXES {
            return invalid(format!("record {index} has {box_count} boxes"));
        }
        let flags = BlockPhysicsFlags::from_bits(reader.read_u8("flags")?)
            .ok_or_else(|| physics_error(format!("record {index} has unknown flag bits")))?;
        let surface_response = SurfaceResponse::read(reader.read_u8("surface response")?)?;
        if reader.read_u8("reserved byte")? != 0 {
            return invalid(format!("record {index} has non-zero reserved byte"));
        }
        let friction_q1e8 = reader.read_u32("friction")?;
        let horizontal_speed_q1e8 = reader.read_u32("horizontal speed")?;
        let vertical_speed_q1e8 = reader.read_u32("vertical speed")?;
        let fluid_height_q1e8 = reader.read_i32("fluid height")?;
        let identity = records
            .get(index)
            .ok_or_else(|| physics_error("BREG identity index missing"))?;
        if sequential_id != u32::try_from(index).expect("bounded count")
            || identity.sequential_id != sequential_id
            || identity.network_hash != network_hash
        {
            return invalid(format!("record {index} identity does not match BREG"));
        }
        if hashes.insert(network_hash, index).is_some() {
            return invalid(format!("duplicate network hash {network_hash:#010x}"));
        }
        let mut boxes = Vec::with_capacity(box_count);
        for box_index in 0..box_count {
            let collision_box = CollisionBox {
                min_x: reader.read_i32("box min x")?,
                min_y: reader.read_i32("box min y")?,
                min_z: reader.read_i32("box min z")?,
                max_x: reader.read_i32("box max x")?,
                max_y: reader.read_i32("box max y")?,
                max_z: reader.read_i32("box max z")?,
            };
            if collision_box.min_x >= collision_box.max_x
                || collision_box.min_y >= collision_box.max_y
                || collision_box.min_z >= collision_box.max_z
            {
                return invalid(format!("record {index} box {box_index} is inverted"));
            }
            boxes.push(collision_box);
        }
        validate_semantics(
            index,
            flags,
            surface_response,
            &boxes,
            friction_q1e8,
            horizontal_speed_q1e8,
            vertical_speed_q1e8,
            fluid_height_q1e8,
        )?;
        *sequential_slot = index;
        decoded.push(BlockPhysicsRecord {
            sequential_id,
            network_hash,
            boxes: boxes.into_boxed_slice(),
            friction_q1e8,
            horizontal_speed_q1e8,
            vertical_speed_q1e8,
            fluid_height_q1e8,
            flags,
            surface_response,
        });
    }
    if reader.remaining() != 0 {
        return invalid(format!("{} trailing bytes", reader.remaining()));
    }
    Ok(PhysicsRegistry {
        records: decoded.into_boxed_slice(),
        sequential: sequential.into_boxed_slice(),
        hashes,
        sha256: Sha256::digest(preg).into(),
        breg_sha256: actual_breg_sha,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_semantics(
    index: usize,
    flags: BlockPhysicsFlags,
    response: SurfaceResponse,
    boxes: &[CollisionBox],
    friction: u32,
    horizontal_speed: u32,
    vertical_speed: u32,
    fluid_height: i32,
) -> Result<(), AssetError> {
    if friction == 0 || horizontal_speed == 0 || vertical_speed == 0 {
        return invalid(format!("record {index} contains a zero scalar"));
    }
    let water = flags.contains(BlockPhysicsFlags::WATER);
    let lava = flags.contains(BlockPhysicsFlags::LAVA);
    if water && lava {
        return invalid(format!("record {index} is both water and lava"));
    }
    if (water || lava) != (fluid_height > 0) || fluid_height > 100_000_000 {
        return invalid(format!("record {index} has contradictory fluid height"));
    }
    if matches!(
        response,
        SurfaceResponse::BubbleUp | SurfaceResponse::BubbleDown
    ) && !water
    {
        return invalid(format!("record {index} has bubble response without water"));
    }
    if flags.contains(BlockPhysicsFlags::PASSABLE) && (water || lava) && !boxes.is_empty() {
        return invalid(format!("record {index} has boxes on a passable fluid"));
    }
    Ok(())
}

fn invalid<T>(detail: impl Into<Box<str>>) -> Result<T, AssetError> {
    Err(physics_error(detail))
}

fn physics_error(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidPhysicsRegistry {
        detail: detail.into(),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8, AssetError> {
        Ok(self.read_exact(1, context)?[0])
    }

    fn read_u32(&mut self, context: &'static str) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4, context)?.try_into().expect("four bytes"),
        ))
    }

    fn read_i32(&mut self, context: &'static str) -> Result<i32, AssetError> {
        Ok(i32::from_le_bytes(
            self.read_exact(4, context)?.try_into().expect("four bytes"),
        ))
    }

    fn read_exact(&mut self, count: usize, context: &'static str) -> Result<&'a [u8], AssetError> {
        if self.remaining() < count {
            return invalid(format!("unexpected EOF reading {context}"));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }
}
