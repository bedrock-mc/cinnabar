use sha2::{Digest, Sha256};
use thiserror::Error;

pub const HUD_CARRIER_MAGIC: [u8; 8] = *b"MCBEHUD1";
pub const HUD_CARRIER_VERSION: u32 = 3;
pub const HUD_SOURCE_MANIFEST_SHA256: [u8; 32] = [
    0xcb, 0x68, 0x45, 0x76, 0xd2, 0xf0, 0xb9, 0x23, 0xcb, 0xfe, 0x59, 0x74, 0x44, 0x11, 0x29, 0x8d,
    0xb1, 0xf4, 0x0a, 0x83, 0x55, 0xb5, 0x90, 0x8e, 0x7c, 0x0f, 0x13, 0x60, 0x49, 0x7a, 0x4b, 0x20,
];
pub const MAX_HUD_TEXTURE_BYTES: usize = 4 * 1024 * 1024;
const HEADER_BYTES: usize = 80;
const DESCRIPTOR_BYTES: usize = 96;
const HASH_BYTES: usize = 32;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_TEXTURE_SIDE: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum HudTextureRole {
    HeartBackground = 0,
    HeartFull = 1,
    HeartHalf = 2,
    HungerBackground = 3,
    HungerFull = 4,
    HungerHalf = 5,
    ArmorEmpty = 6,
    ArmorFull = 7,
    ArmorHalf = 8,
    BubbleFull = 9,
    BubbleEmpty = 10,
    Hotbar0 = 11,
    Hotbar1 = 12,
    Hotbar2 = 13,
    Hotbar3 = 14,
    Hotbar4 = 15,
    Hotbar5 = 16,
    Hotbar6 = 17,
    Hotbar7 = 18,
    Hotbar8 = 19,
    SelectedHotbarSlot = 20,
    HotbarStartCap = 21,
    HotbarEndCap = 22,
    BossProgressEmpty = 23,
    BossProgressFilled = 24,
}

impl HudTextureRole {
    pub const ALL: [Self; 25] = [
        Self::HeartBackground,
        Self::HeartFull,
        Self::HeartHalf,
        Self::HungerBackground,
        Self::HungerFull,
        Self::HungerHalf,
        Self::ArmorEmpty,
        Self::ArmorFull,
        Self::ArmorHalf,
        Self::BubbleFull,
        Self::BubbleEmpty,
        Self::Hotbar0,
        Self::Hotbar1,
        Self::Hotbar2,
        Self::Hotbar3,
        Self::Hotbar4,
        Self::Hotbar5,
        Self::Hotbar6,
        Self::Hotbar7,
        Self::Hotbar8,
        Self::SelectedHotbarSlot,
        Self::HotbarStartCap,
        Self::HotbarEndCap,
        Self::BossProgressEmpty,
        Self::BossProgressFilled,
    ];

    #[must_use]
    pub const fn source_path(self) -> &'static str {
        match self {
            Self::HeartBackground => "textures/ui/heart_background.png",
            Self::HeartFull => "textures/ui/heart.png",
            Self::HeartHalf => "textures/ui/heart_half.png",
            Self::HungerBackground => "textures/ui/hunger_background.png",
            Self::HungerFull => "textures/ui/hunger_full.png",
            Self::HungerHalf => "textures/ui/hunger_half.png",
            Self::ArmorEmpty => "textures/ui/armor_empty.png",
            Self::ArmorFull => "textures/ui/armor_full.png",
            Self::ArmorHalf => "textures/ui/armor_half.png",
            Self::BubbleFull => "textures/ui/bubble.png",
            Self::BubbleEmpty => "textures/ui/bubble_empty.png",
            Self::Hotbar0 => "textures/ui/hotbar_0.png",
            Self::Hotbar1 => "textures/ui/hotbar_1.png",
            Self::Hotbar2 => "textures/ui/hotbar_2.png",
            Self::Hotbar3 => "textures/ui/hotbar_3.png",
            Self::Hotbar4 => "textures/ui/hotbar_4.png",
            Self::Hotbar5 => "textures/ui/hotbar_5.png",
            Self::Hotbar6 => "textures/ui/hotbar_6.png",
            Self::Hotbar7 => "textures/ui/hotbar_7.png",
            Self::Hotbar8 => "textures/ui/hotbar_8.png",
            Self::SelectedHotbarSlot => "textures/ui/selected_hotbar_slot.png",
            Self::HotbarStartCap => "textures/ui/hotbar_start_cap.png",
            Self::HotbarEndCap => "textures/ui/hotbar_end_cap.png",
            Self::BossProgressEmpty => "textures/ui/empty_progress_bar.png",
            Self::BossProgressFilled => "textures/ui/filled_progress_bar.png",
        }
    }

    #[must_use]
    pub const fn expected_size(self) -> [u32; 2] {
        match self {
            Self::HeartBackground
            | Self::HeartFull
            | Self::HeartHalf
            | Self::HungerBackground
            | Self::HungerFull
            | Self::HungerHalf
            | Self::ArmorEmpty
            | Self::ArmorFull
            | Self::ArmorHalf
            | Self::BubbleFull
            | Self::BubbleEmpty => [9, 9],
            Self::Hotbar0
            | Self::Hotbar1
            | Self::Hotbar2
            | Self::Hotbar3
            | Self::Hotbar4
            | Self::Hotbar5
            | Self::Hotbar6
            | Self::Hotbar7
            | Self::Hotbar8 => [20, 22],
            Self::SelectedHotbarSlot => [24, 24],
            Self::HotbarStartCap | Self::HotbarEndCap => [1, 22],
            Self::BossProgressEmpty | Self::BossProgressFilled => [13, 5],
        }
    }

    fn from_u32(value: u32) -> Option<Self> {
        if value < Self::ALL.len() as u32 {
            Self::ALL.get(value as usize).copied()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudTexture {
    pub role: HudTextureRole,
    pub source_bytes: u32,
    pub source_sha256: [u8; 32],
    pub pixels_sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

pub struct RuntimeHudCatalog {
    source_manifest_sha256: [u8; 32],
    textures: Box<[HudTexture]>,
}

impl RuntimeHudCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, HudCatalogError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != HUD_CARRIER_MAGIC
            || read_u32(bytes, 8)? != HUD_CARRIER_VERSION
        {
            return Err(HudCatalogError::Invalid("unsupported HUD carrier header"));
        }
        let count = read_u32(bytes, 12)? as usize;
        let source_manifest_sha256 = read_array::<32>(bytes, 16)?;
        let descriptors_offset = read_usize(bytes, 48)?;
        let payload_offset = read_usize(bytes, 56)?;
        let payload_end = read_usize(bytes, 64)?;
        if count != HudTextureRole::ALL.len()
            || source_manifest_sha256 != HUD_SOURCE_MANIFEST_SHA256
            || bytes[72..HEADER_BYTES] != [0; 8]
            || descriptors_offset != HEADER_BYTES
            || payload_offset
                != HEADER_BYTES
                    .checked_add(
                        count
                            .checked_mul(DESCRIPTOR_BYTES)
                            .ok_or(HudCatalogError::Invalid("HUD descriptor length overflow"))?,
                    )
                    .ok_or(HudCatalogError::Invalid("HUD descriptor offset overflow"))?
            || payload_end < payload_offset
            || payload_end - payload_offset > MAX_HUD_TEXTURE_BYTES
            || bytes.len()
                != payload_end
                    .checked_add(HASH_BYTES)
                    .ok_or(HudCatalogError::Invalid("HUD carrier length overflow"))?
        {
            return Err(HudCatalogError::Invalid("noncanonical HUD carrier layout"));
        }
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(HudCatalogError::Invalid(
                "HUD carrier envelope hash mismatch",
            ));
        }

        let mut expected_payload_offset = payload_offset;
        let mut textures = Vec::with_capacity(count);
        for (index, expected_role) in HudTextureRole::ALL.into_iter().enumerate() {
            let descriptor = descriptors_offset + index * DESCRIPTOR_BYTES;
            let role = HudTextureRole::from_u32(read_u32(bytes, descriptor)?)
                .ok_or(HudCatalogError::Invalid("unknown HUD texture role"))?;
            let width = read_u32(bytes, descriptor + 4)?;
            let height = read_u32(bytes, descriptor + 8)?;
            let source_bytes = read_u32(bytes, descriptor + 12)?;
            let texture_offset = read_usize(bytes, descriptor + 16)?;
            let texture_length = read_usize(bytes, descriptor + 24)?;
            let source_sha256 = read_array::<32>(bytes, descriptor + 32)?;
            let pixels_sha256 = read_array::<32>(bytes, descriptor + 64)?;
            let expected_length = pixel_length(width, height)?;
            if role != expected_role
                || [width, height] != role.expected_size()
                || width > MAX_TEXTURE_SIDE
                || height > MAX_TEXTURE_SIDE
                || source_bytes == 0
                || source_bytes as usize > MAX_SOURCE_BYTES
                || source_sha256 == [0; 32]
                || texture_offset != expected_payload_offset
                || texture_length != expected_length
            {
                return Err(HudCatalogError::Invalid("invalid HUD texture descriptor"));
            }
            let texture_end = texture_offset
                .checked_add(texture_length)
                .filter(|end| *end <= payload_end)
                .ok_or(HudCatalogError::Invalid(
                    "HUD texture payload is out of range",
                ))?;
            let rgba8 = bytes[texture_offset..texture_end]
                .to_vec()
                .into_boxed_slice();
            if Sha256::digest(&rgba8).as_slice() != pixels_sha256 {
                return Err(HudCatalogError::Invalid("HUD texture pixel hash mismatch"));
            }
            textures.push(HudTexture {
                role,
                source_bytes,
                source_sha256,
                pixels_sha256,
                width,
                height,
                rgba8,
            });
            expected_payload_offset = texture_end;
        }
        if expected_payload_offset != payload_end {
            return Err(HudCatalogError::Invalid("trailing HUD texture payload"));
        }
        Ok(Self {
            source_manifest_sha256,
            textures: textures.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn textures(&self) -> &[HudTexture] {
        &self.textures
    }

    #[must_use]
    pub fn texture(&self, role: HudTextureRole) -> &HudTexture {
        &self.textures[role as usize]
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum HudCatalogError {
    #[error("invalid HUD texture carrier: {0}")]
    Invalid(&'static str),
}

pub fn encode_hud_catalog(
    source_manifest_sha256: [u8; 32],
    textures: &[HudTexture],
) -> Result<Vec<u8>, HudCatalogError> {
    if source_manifest_sha256 != HUD_SOURCE_MANIFEST_SHA256 {
        return Err(HudCatalogError::Invalid(
            "unreviewed HUD source manifest identity",
        ));
    }
    if textures.len() != HudTextureRole::ALL.len() {
        return Err(HudCatalogError::Invalid("incomplete HUD texture catalog"));
    }
    let payload_offset = HEADER_BYTES
        .checked_add(
            textures
                .len()
                .checked_mul(DESCRIPTOR_BYTES)
                .ok_or(HudCatalogError::Invalid("HUD descriptor length overflow"))?,
        )
        .ok_or(HudCatalogError::Invalid("HUD descriptor offset overflow"))?;
    let mut payload_bytes = 0usize;
    for (index, texture) in textures.iter().enumerate() {
        validate_texture(texture, HudTextureRole::ALL[index])?;
        payload_bytes = payload_bytes
            .checked_add(texture.rgba8.len())
            .ok_or(HudCatalogError::Invalid("HUD payload length overflow"))?;
    }
    if payload_bytes > MAX_HUD_TEXTURE_BYTES {
        return Err(HudCatalogError::Invalid(
            "HUD texture payload exceeds bound",
        ));
    }
    let payload_end = payload_offset
        .checked_add(payload_bytes)
        .ok_or(HudCatalogError::Invalid("HUD payload offset overflow"))?;
    let mut bytes = vec![0; payload_end];
    bytes[..8].copy_from_slice(&HUD_CARRIER_MAGIC);
    write_u32(&mut bytes, 8, HUD_CARRIER_VERSION);
    write_u32(&mut bytes, 12, textures.len() as u32);
    bytes[16..48].copy_from_slice(&source_manifest_sha256);
    write_u64(&mut bytes, 48, HEADER_BYTES as u64);
    write_u64(&mut bytes, 56, payload_offset as u64);
    write_u64(&mut bytes, 64, payload_end as u64);

    let mut texture_offset = payload_offset;
    for (index, texture) in textures.iter().enumerate() {
        validate_texture(texture, HudTextureRole::ALL[index])?;
        let descriptor = HEADER_BYTES + index * DESCRIPTOR_BYTES;
        write_u32(&mut bytes, descriptor, texture.role as u32);
        write_u32(&mut bytes, descriptor + 4, texture.width);
        write_u32(&mut bytes, descriptor + 8, texture.height);
        write_u32(&mut bytes, descriptor + 12, texture.source_bytes);
        write_u64(&mut bytes, descriptor + 16, texture_offset as u64);
        write_u64(&mut bytes, descriptor + 24, texture.rgba8.len() as u64);
        bytes[descriptor + 32..descriptor + 64].copy_from_slice(&texture.source_sha256);
        bytes[descriptor + 64..descriptor + 96].copy_from_slice(&texture.pixels_sha256);
        let end = texture_offset
            .checked_add(texture.rgba8.len())
            .ok_or(HudCatalogError::Invalid("HUD payload offset overflow"))?;
        bytes[texture_offset..end].copy_from_slice(&texture.rgba8);
        texture_offset = end;
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn validate_texture(
    texture: &HudTexture,
    expected_role: HudTextureRole,
) -> Result<(), HudCatalogError> {
    if texture.role != expected_role
        || [texture.width, texture.height] != expected_role.expected_size()
        || texture.width > MAX_TEXTURE_SIDE
        || texture.height > MAX_TEXTURE_SIDE
        || texture.source_bytes == 0
        || texture.source_bytes as usize > MAX_SOURCE_BYTES
        || texture.source_sha256 == [0; 32]
        || texture.rgba8.len() != pixel_length(texture.width, texture.height)?
        || Sha256::digest(&texture.rgba8).as_slice() != texture.pixels_sha256
    {
        return Err(HudCatalogError::Invalid("invalid HUD texture"));
    }
    Ok(())
}

fn pixel_length(width: u32, height: u32) -> Result<usize, HudCatalogError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(HudCatalogError::Invalid("HUD pixel length overflow"))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, HudCatalogError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_usize(bytes: &[u8], offset: usize) -> Result<usize, HudCatalogError> {
    usize::try_from(u64::from_le_bytes(read_array(bytes, offset)?))
        .map_err(|_| HudCatalogError::Invalid("HUD carrier offset exceeds platform"))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], HudCatalogError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(HudCatalogError::Invalid("HUD carrier field overflow"))?,
        )
        .ok_or(HudCatalogError::Invalid("truncated HUD carrier field"))?
        .try_into()
        .map_err(|_| HudCatalogError::Invalid("invalid HUD carrier field"))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
