//! Only the image and decoded skin JSON fields consumed by the pinned Go validator.

use super::super::{Entries, RawClientData, invalid, same_field, schema_generated::Record, types};
use base64::{
    Engine as _, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig},
};
use serde_json::value::RawValue;
use std::{borrow::Cow, io};

mod json;

const STANDARD: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

#[derive(Default)]
pub(super) struct Skin {
    id: String,
    image: Image,
    cape: Image,
    geometry: String,
    patch: String,
    animations: Vec<Animation>,
    animation_len: usize,
}

#[derive(Default)]
struct Image {
    data: String,
    height: i64,
    width: i64,
}

#[derive(Default)]
struct Animation {
    image: Image,
    kind: i64,
}

impl Skin {
    /// Materializes only semantically consumed claims, retaining Go's reused animation elements.
    pub(super) fn read(raw: &RawClientData) -> io::Result<Self> {
        let mut skin = Self {
            id: raw.read("SkinId")?.unwrap_or_default(),
            image: Image {
                data: raw.read("SkinData")?.unwrap_or_default(),
                height: raw.integer("SkinImageHeight")?.unwrap_or_default(),
                width: raw.integer("SkinImageWidth")?.unwrap_or_default(),
            },
            cape: Image {
                data: raw.read("CapeData")?.unwrap_or_default(),
                height: raw.integer("CapeImageHeight")?.unwrap_or_default(),
                width: raw.integer("CapeImageWidth")?.unwrap_or_default(),
            },
            geometry: raw.read("SkinGeometryData")?.unwrap_or_default(),
            patch: raw.read("SkinResourcePatch")?.unwrap_or_default(),
            ..Default::default()
        };
        for (_, value) in raw
            .entries
            .iter()
            .filter(|(name, _)| same_field(name, "AnimatedImageData"))
        {
            skin.read_animations(value)?;
        }
        Ok(skin)
    }

    /// Validates the complete aggregate replaced together by Go's skin refusal projection.
    pub(super) fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.image.valid()
            && self.cape.valid()
            && self.animations[..self.animation_len]
                .iter()
                .all(|animation| (0..=3).contains(&animation.kind) && animation.image.valid())
            && decoded_object(&self.geometry)
            && decoded_object(&self.patch)
    }

    /// A slice null/empty array drops retained elements; shorter nonempty arrays keep reusable tails.
    fn read_animations(&mut self, raw: &RawValue) -> io::Result<()> {
        if raw.get() == "null" {
            self.animations = Vec::new();
            self.animation_len = 0;
            return Ok(());
        }
        let values: Vec<&RawValue> = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
        if values.is_empty() {
            self.animations = Vec::new();
        }
        for (index, raw) in values.iter().enumerate() {
            if index == self.animations.len() {
                self.animations.push(Animation::default());
            }
            if raw.get() != "null" {
                self.animations[index].apply(raw)?;
            }
        }
        self.animation_len = values.len();
        Ok(())
    }
}

impl Animation {
    /// Reuses the existing generated field matching and integer token owner for this actual record.
    fn apply(&mut self, raw: &RawValue) -> io::Result<()> {
        let Entries(entries) = serde_json::from_str(raw.get()).map_err(|_| invalid())?;
        for (name, value) in entries {
            if value.get() == "null" {
                continue;
            }
            let Some(field) = Record::Animation
                .fields()
                .iter()
                .find(|field| same_field(&name, field.name))
            else {
                continue;
            };
            match field.name {
                "Image" => {
                    self.image.data = serde_json::from_str(value.get()).map_err(|_| invalid())?;
                }
                "ImageHeight" => self.image.height = types::integer(&value)?,
                "ImageWidth" => self.image.width = types::integer(&value)?,
                "Type" => self.kind = types::integer(&value)?,
                _ => {}
            }
        }
        Ok(())
    }
}

impl Image {
    /// Checked signed products preserve valid source values while rejecting overflow without allocation.
    fn valid(&self) -> bool {
        let Some(expected) = self
            .height
            .checked_mul(self.width)
            .and_then(|length| length.checked_mul(4))
            .and_then(|length| usize::try_from(length).ok())
        else {
            return false;
        };
        decode(&self.data).is_some_and(|decoded| decoded.len() == expected)
    }
}

/// Go's standard base64 decoder ignores CR/LF only and permits nonzero unused padding bits.
fn decode(data: &str) -> Option<Vec<u8>> {
    let data = if data.contains(['\r', '\n']) {
        Cow::Owned(
            data.bytes()
                .filter(|byte| !matches!(byte, b'\r' | b'\n'))
                .collect::<Vec<_>>(),
        )
    } else {
        Cow::Borrowed(data.as_bytes())
    };
    STANDARD.decode(data.as_ref()).ok()
}

/// Unmarshalling into Go's map admits an object or null, and empty decoded bytes skip parsing.
fn decoded_object(data: &str) -> bool {
    let Some(decoded) = decode(data) else {
        return false;
    };
    decoded.is_empty() || json::object_or_null(&decoded)
}
