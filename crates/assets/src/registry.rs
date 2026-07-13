use std::{collections::HashSet, str};

use bitflags::bitflags;

use crate::AssetError;

const REGISTRY_MAGIC: &[u8; 8] = b"BREG1003";
const REGISTRY_PROTOCOL: u32 = 1001;
const RECORD_HEADER_BYTES: usize = 24 + 8 * 4;
const MAX_REGISTRY_RECORDS: usize = 65_536;
const MAX_REGISTRY_STATE_BYTES: usize = 1024 * 1024;
const MAX_COLLISION_BOXES: usize = 7;

bitflags! {
    /// Geometry and full-face occlusion facts retained by BREG1003.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct BlockFlags: u8 {
        const AIR = 1 << 0;
        const CUBE_GEOMETRY = 1 << 1;
        const OCCLUDES_FULL_FACE = 1 << 2;
        const LEAF_MODEL = 1 << 3;
    }

    /// Pinned sources that proved the identity of a canonical state.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct RegistryProvenance: u8 {
        const PMMP = 1 << 0;
        const DRAGONFLY = 1 << 1;
        const PRISMARINE = 1 << 2;
        const VALENTINE = 1 << 3;
    }
}

impl BlockFlags {
    #[must_use]
    pub const fn has_valid_semantics(self) -> bool {
        let air = self.contains(Self::AIR);
        let cube = self.contains(Self::CUBE_GEOMETRY);
        let leaf = self.contains(Self::LEAF_MODEL);
        (!air || self.bits() == Self::AIR.bits())
            && (!leaf || (cube && !self.contains(Self::OCCLUDES_FULL_FACE)))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ModelFamily {
    #[default]
    Unknown = 0,
    Air = 1,
    Cube = 2,
    Leaves = 3,
    Cross = 4,
    Crop = 5,
    Liquid = 6,
    Slab = 7,
    Stair = 8,
    Door = 9,
    Trapdoor = 10,
    Pane = 11,
    Fence = 12,
    Gate = 13,
    Chest = 14,
    Sign = 15,
    Wall = 16,
    Bed = 17,
    Rail = 18,
    Torch = 19,
    Button = 20,
    PressurePlate = 21,
    Carpet = 22,
    Layer = 23,
    Decorative = 24,
    Statue = 25,
    Cuboid = 26,
    Aquatic = 27,
    Cocoa = 28,
    Lever = 29,
    Invisible = 30,
    FlowerBed = 31,
    Vine = 32,
}

impl ModelFamily {
    fn read(raw: u8) -> Result<Self, AssetError> {
        Ok(match raw {
            0 => Self::Unknown,
            1 => Self::Air,
            2 => Self::Cube,
            3 => Self::Leaves,
            4 => Self::Cross,
            5 => Self::Crop,
            6 => Self::Liquid,
            7 => Self::Slab,
            8 => Self::Stair,
            9 => Self::Door,
            10 => Self::Trapdoor,
            11 => Self::Pane,
            12 => Self::Fence,
            13 => Self::Gate,
            14 => Self::Chest,
            15 => Self::Sign,
            16 => Self::Wall,
            17 => Self::Bed,
            18 => Self::Rail,
            19 => Self::Torch,
            20 => Self::Button,
            21 => Self::PressurePlate,
            22 => Self::Carpet,
            23 => Self::Layer,
            24 => Self::Decorative,
            25 => Self::Statue,
            26 => Self::Cuboid,
            27 => Self::Aquatic,
            28 => Self::Cocoa,
            29 => Self::Lever,
            30 => Self::Invisible,
            31 => Self::FlowerBed,
            32 => Self::Vine,
            _ => return Err(AssetError::InvalidRegistryFlags(raw)),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ContributorRole {
    #[default]
    Primary = 0,
    LiquidAdditional = 1,
    Air = 2,
}

impl ContributorRole {
    pub(crate) fn read(raw: u8) -> Result<Self, AssetError> {
        match raw {
            0 => Ok(Self::Primary),
            1 => Ok(Self::LiquidAdditional),
            2 => Ok(Self::Air),
            _ => Err(AssetError::InvalidRegistryFlags(raw)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ModelStateField {
    Orientation = 1,
    Half = 2,
    Open = 3,
    Hinge = 4,
    Connections = 5,
    Growth = 6,
    LiquidDepth = 7,
    Flags = 8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ModelState {
    mask: u8,
    values: [u32; 8],
}

impl ModelState {
    #[must_use]
    pub fn get(self, field: ModelStateField) -> Option<u32> {
        let index = usize::from(field as u8 - 1);
        (self.mask & (1 << index) != 0).then_some(self.values[index])
    }

    #[must_use]
    pub const fn mask(self) -> u8 {
        self.mask
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CollisionConfidence {
    #[default]
    None = 0,
    CollisionOnly = 1,
    ReviewedVisibleBounds = 2,
}

impl CollisionConfidence {
    fn read(raw: u8) -> Result<Self, AssetError> {
        match raw {
            0 => Ok(Self::None),
            1 => Ok(Self::CollisionOnly),
            2 => Ok(Self::ReviewedVisibleBounds),
            _ => Err(AssetError::InvalidRegistryFlags(raw)),
        }
    }
}

/// Signed 1/100,000,000-block coordinates copied deterministically from the
/// pinned Prismarine collision-shape source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CollisionBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollisionSeed {
    pub shape_id: u16,
    pub confidence: CollisionConfidence,
    pub boxes: Box<[CollisionBox]>,
}

/// One canonical state from the deterministic BREG1003 export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecord {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub name: Box<str>,
    pub canonical_state: Box<str>,
    pub flags: BlockFlags,
    pub model_family: ModelFamily,
    pub contributor_role: ContributorRole,
    pub model_state: ModelState,
    pub face_coverage: u8,
    pub collision_seed: CollisionSeed,
    pub provenance: RegistryProvenance,
}

/// Reads the bounded protocol-1001 BREG1003 block registry.
pub fn read_registry(bytes: &[u8]) -> Result<Box<[RegistryRecord]>, AssetError> {
    let mut reader = Reader::new(bytes);
    if reader.read_exact(REGISTRY_MAGIC.len(), "registry magic")? != REGISTRY_MAGIC {
        return Err(AssetError::InvalidRegistryMagic);
    }
    if reader.read_u32("registry protocol")? != REGISTRY_PROTOCOL {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let name_count = reader.read_u32("canonical name count")? as usize;
    let count = reader.read_u32("canonical state count")? as usize;
    let valentine_names = reader.read_u32("Valentine name count")? as usize;
    let valentine_states = reader.read_u32("Valentine state count")? as usize;
    let gap_names = reader.read_u32("Valentine name gap")? as usize;
    let gap_states = reader.read_u32("Valentine state gap")? as usize;
    if count > MAX_REGISTRY_RECORDS {
        return Err(AssetError::TooManyRegistryRecords {
            count,
            max: MAX_REGISTRY_RECORDS,
        });
    }
    if name_count > count
        || valentine_names.checked_add(gap_names) != Some(name_count)
        || valentine_states.checked_add(gap_states) != Some(count)
    {
        return Err(AssetError::InvalidRegistryFlags(0xff));
    }
    let minimum_bytes =
        count
            .checked_mul(RECORD_HEADER_BYTES)
            .ok_or(AssetError::TooManyRegistryRecords {
                count,
                max: MAX_REGISTRY_RECORDS,
            })?;
    if reader.remaining() < minimum_bytes {
        return Err(AssetError::UnexpectedEof {
            context: "registry record headers",
            needed: minimum_bytes,
            remaining: reader.remaining(),
        });
    }

    let mut records = Vec::with_capacity(count);
    let mut sequential_ids = HashSet::with_capacity(count);
    let mut network_hashes = HashSet::with_capacity(count);
    let mut names = HashSet::with_capacity(name_count);
    let mut valentine_name_set = HashSet::with_capacity(valentine_names);
    let mut valentine_overlap = 0usize;
    for _ in 0..count {
        let sequential_id = reader.read_u32("record sequential ID")?;
        let network_hash = reader.read_u32("record network hash")?;
        let raw_flags = reader.read_u8("record flags")?;
        let model_family = ModelFamily::read(reader.read_u8("record model family")?)?;
        let contributor_role = ContributorRole::read(reader.read_u8("record contributor role")?)?;
        let model_mask = reader.read_u8("record model-state mask")?;
        let face_coverage = reader.read_u8("record face coverage")?;
        let confidence = CollisionConfidence::read(reader.read_u8("record collision confidence")?)?;
        let raw_provenance = reader.read_u8("record provenance")?;
        let box_count = reader.read_u8("record collision box count")? as usize;
        let shape_id = reader.read_u16("record collision shape ID")?;
        let name_len = reader.read_u16("record name length")? as usize;
        let state_len = reader.read_u32("record state length")? as usize;
        let mut values = [0u32; 8];
        for value in &mut values {
            *value = reader.read_u32("record model-state value")?;
        }

        if !sequential_ids.insert(sequential_id) {
            return Err(AssetError::DuplicateSequentialId(sequential_id));
        }
        if !network_hashes.insert(network_hash) {
            return Err(AssetError::DuplicateNetworkHash(network_hash));
        }
        let flags =
            BlockFlags::from_bits(raw_flags).ok_or(AssetError::InvalidRegistryFlags(raw_flags))?;
        if !flags.has_valid_semantics() {
            return Err(AssetError::InvalidRegistryFlags(raw_flags));
        }
        if values
            .iter()
            .enumerate()
            .any(|(index, value)| model_mask & (1 << index) == 0 && *value != 0)
            || face_coverage & !0x3f != 0
            || box_count > MAX_COLLISION_BOXES
        {
            return Err(AssetError::InvalidRegistryFlags(model_mask));
        }
        let provenance = RegistryProvenance::from_bits(raw_provenance)
            .filter(|source| !source.is_empty())
            .ok_or(AssetError::InvalidRegistryFlags(raw_provenance))?;
        if provenance.contains(RegistryProvenance::VALENTINE) {
            valentine_overlap += 1;
        }
        if confidence == CollisionConfidence::None && (shape_id != 0 || box_count != 0) {
            return Err(AssetError::InvalidRegistryFlags(confidence as u8));
        }
        if state_len > MAX_REGISTRY_STATE_BYTES {
            return Err(AssetError::RegistryStateTooLarge {
                size: state_len,
                max: MAX_REGISTRY_STATE_BYTES,
            });
        }
        let mut boxes = Vec::with_capacity(box_count);
        for _ in 0..box_count {
            let collision_box = CollisionBox {
                min_x: reader.read_i32("collision min x")?,
                min_y: reader.read_i32("collision min y")?,
                min_z: reader.read_i32("collision min z")?,
                max_x: reader.read_i32("collision max x")?,
                max_y: reader.read_i32("collision max y")?,
                max_z: reader.read_i32("collision max z")?,
            };
            if collision_box.min_x > collision_box.max_x
                || collision_box.min_y > collision_box.max_y
                || collision_box.min_z > collision_box.max_z
            {
                return Err(AssetError::InvalidRegistryFlags(confidence as u8));
            }
            boxes.push(collision_box);
        }
        let name: Box<str> = str::from_utf8(reader.read_exact(name_len, "record name")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "name",
                source,
            })?
            .into();
        if provenance.contains(RegistryProvenance::VALENTINE) {
            valentine_name_set.insert(name.clone());
        }
        names.insert(name.clone());
        let canonical_state = str::from_utf8(reader.read_exact(state_len, "record state")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "canonical state",
                source,
            })?
            .into();
        records.push(RegistryRecord {
            sequential_id,
            network_hash,
            name,
            canonical_state,
            flags,
            model_family,
            contributor_role,
            model_state: ModelState {
                mask: model_mask,
                values,
            },
            face_coverage,
            collision_seed: CollisionSeed {
                shape_id,
                confidence,
                boxes: boxes.into_boxed_slice(),
            },
            provenance,
        });
    }
    if names.len() != name_count
        || valentine_overlap != valentine_states
        || valentine_name_set.len() != valentine_names
    {
        return Err(AssetError::InvalidRegistryFlags(0xff));
    }
    if reader.remaining() != 0 {
        return Err(AssetError::TrailingRegistryBytes {
            remaining: reader.remaining(),
        });
    }
    Ok(records.into_boxed_slice())
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

    fn read_u16(&mut self, context: &'static str) -> Result<u16, AssetError> {
        Ok(u16::from_le_bytes(
            self.read_exact(2, context)?.try_into().expect("two bytes"),
        ))
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
        let remaining = self.remaining();
        if remaining < count {
            return Err(AssetError::UnexpectedEof {
                context,
                needed: count,
                remaining,
            });
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }
}
