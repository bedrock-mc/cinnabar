// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use crate::enums::*;

use crate::wire;

// Domain: actor

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorDataBoundingBoxComponent {
    pub actor_data_bounding_box: [wire::F32LE; 3],
}

impl wire::Encode for ActorDataBoundingBoxComponent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        for item in self.actor_data_bounding_box.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for ActorDataBoundingBoxComponent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_data_bounding_box = [
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
        ];
        Ok(Self {
            actor_data_bounding_box,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorDataFlagComponent {
    pub actor_flag_bitset_data: Bitset131,
}

impl wire::Encode for ActorDataFlagComponent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_bitset(writer, self.actor_flag_bitset_data.0.as_slice(), 131)?;
        Ok(())
    }
}

impl wire::Decode for ActorDataFlagComponent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_flag_bitset_data = Bitset131(wire::decode_bitset::<3>(reader, 131)?);
        Ok(Self {
            actor_flag_bitset_data,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActorRuntimeID(pub u64);

impl wire::Encode for ActorRuntimeID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarULong(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorRuntimeID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::VarULong as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ActorUniqueID(pub i64);

impl wire::Encode for ActorUniqueID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag64(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorUniqueID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag64 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerUpdateEntityOverridesData {
    ClearOverride {
        type_: wire::Str,
    },
    RemoveOverride {
        type_: wire::Str,
    },
    IntOverride {
        type_: wire::Str,
        value: wire::I32LE,
    },
    FloatOverride {
        type_: wire::Str,
        value: wire::F32LE,
    },
}

impl PlayerUpdateEntityOverridesData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::ClearOverride { .. } => 0,
            Self::RemoveOverride { .. } => 1,
            Self::IntOverride { .. } => 2,
            Self::FloatOverride { .. } => 3,
        }
    }
}

impl Default for PlayerUpdateEntityOverridesData {
    fn default() -> Self {
        Self::ClearOverride {
            type_: Default::default(),
        }
    }
}

impl wire::Encode for PlayerUpdateEntityOverridesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::ClearOverride { type_ } => {
                type_.encode(writer)?;
            }
            Self::RemoveOverride { type_ } => {
                type_.encode(writer)?;
            }
            Self::IntOverride { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::FloatOverride { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for PlayerUpdateEntityOverridesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let type_ = reader.read_str(
                    crate::fields::PLAYER_UPDATE_ENTITY_OVERRIDES_DATA_CLEAR_OVERRIDE_TYPE,
                )?;
                Self::ClearOverride { type_ }
            }
            1 => {
                let type_ = reader.read_str(
                    crate::fields::PLAYER_UPDATE_ENTITY_OVERRIDES_DATA_REMOVE_OVERRIDE_TYPE,
                )?;
                Self::RemoveOverride { type_ }
            }
            2 => {
                let type_ = reader.read_str(
                    crate::fields::PLAYER_UPDATE_ENTITY_OVERRIDES_DATA_INT_OVERRIDE_TYPE,
                )?;
                let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                Self::IntOverride { type_, value }
            }
            3 => {
                let type_ = reader.read_str(
                    crate::fields::PLAYER_UPDATE_ENTITY_OVERRIDES_DATA_FLOAT_OVERRIDE_TYPE,
                )?;
                let value = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::FloatOverride { type_, value }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "PlayerUpdateEntityOverridesData",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: attribute

/// AttributeModifier temporarily buffs/debuffs a given attribute until the modifier is used. In vanilla,
/// these are mainly used for effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeModifier {
    /// `id` is the unique ID of the modifier. It is used to identify the modifier in the packet.
    pub id: wire::Str,
    /// `name` is the name of the attribute that is modified.
    pub name: wire::Str,
    /// `amount` is the amount of difference between the current value of the attribute and the new value.
    pub amount: wire::F32LE,
    /// `operation` is the operation that is performed on the attribute. It can be addition, multiply base,
    /// multiply total or cap.
    pub operation: wire::I32LE,
    /// `operand` ... TODO: Figure out what this field is used for.
    pub operand: wire::I32LE,
    /// `is_serializable` ... TODO: Figure out what this field is used for.
    pub is_serializable: bool,
}

impl wire::Encode for AttributeModifier {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        self.name.encode(writer)?;
        self.amount.encode(writer)?;
        self.operation.encode(writer)?;
        self.operand.encode(writer)?;
        self.is_serializable.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AttributeModifier {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = reader.read_str(crate::fields::ATTRIBUTE_MODIFIER_ID)?;
        let name = reader.read_str(crate::fields::ATTRIBUTE_MODIFIER_NAME)?;
        let amount = <wire::F32LE as wire::Decode>::decode(reader)?;
        let operation = <wire::I32LE as wire::Decode>::decode(reader)?;
        let operand = <wire::I32LE as wire::Decode>::decode(reader)?;
        let is_serializable = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            id,
            name,
            amount,
            operation,
            operand,
            is_serializable,
        })
    }
}

// Domain: attribute_layer

/// AttributeData represents a polymorphic attribute value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttributeData {
    pub min_value: wire::F32LE,
    pub max_value: wire::F32LE,
    pub current_value: wire::F32LE,
    pub default_min_value: wire::F32LE,
    pub default_max_value: wire::F32LE,
    pub default_value: wire::F32LE,
    pub name: wire::Str,
    pub modifiers: Vec<AttributeModifier>,
}

impl wire::Encode for AttributeData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.min_value.encode(writer)?;
        self.max_value.encode(writer)?;
        self.current_value.encode(writer)?;
        self.default_min_value.encode(writer)?;
        self.default_max_value.encode(writer)?;
        self.default_value.encode(writer)?;
        self.name.encode(writer)?;
        wire::encode_collection(writer, self.modifiers.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for AttributeData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let min_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let max_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let current_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let default_min_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let default_max_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let default_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let name = reader.read_str(crate::fields::ATTRIBUTE_DATA_NAME)?;
        let modifiers = wire::decode_collection::<AttributeModifier>(
            reader,
            crate::fields::ATTRIBUTE_DATA_MODIFIERS,
            15,
        )?;
        Ok(Self {
            min_value,
            max_value,
            current_value,
            default_min_value,
            default_max_value,
            default_value,
            name,
            modifiers,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeLayerSyncData {
    UpdateAttributeLayersData {
        attribute_layers: Vec<EASAttributeLayerData>,
    },
    UpdateAttributeLayerSettingsData {
        attribute_layer_name: wire::Str,
        attribute_layer_dimension: DimensionType,
        attributes_layer_settings: EASAttributeLayerSettings,
    },
    UpdateEnvironmentAttributesData {
        attribute_layer_name: wire::Str,
        attribute_layer_dimension: DimensionType,
        attributes: Vec<EASEnvironmentAttributeData>,
    },
    RemoveEnvironmentAttributesData {
        attribute_layer_name: wire::Str,
        attribute_layer_dimension: DimensionType,
        attributes: Vec<wire::Str>,
    },
}

impl AttributeLayerSyncData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::UpdateAttributeLayersData { .. } => 0,
            Self::UpdateAttributeLayerSettingsData { .. } => 1,
            Self::UpdateEnvironmentAttributesData { .. } => 2,
            Self::RemoveEnvironmentAttributesData { .. } => 3,
        }
    }
}

impl Default for AttributeLayerSyncData {
    fn default() -> Self {
        Self::UpdateAttributeLayersData {
            attribute_layers: Default::default(),
        }
    }
}

impl wire::Encode for AttributeLayerSyncData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::UpdateAttributeLayersData { attribute_layers } => {
                wire::encode_collection_limits(writer, attribute_layers.as_slice(), 0, 512)?;
            }
            Self::UpdateAttributeLayerSettingsData {
                attribute_layer_name,
                attribute_layer_dimension,
                attributes_layer_settings,
            } => {
                wire::encode_string_limits(writer, attribute_layer_name, 0, 128)?;
                attribute_layer_dimension.encode(writer)?;
                attributes_layer_settings.encode(writer)?;
            }
            Self::UpdateEnvironmentAttributesData {
                attribute_layer_name,
                attribute_layer_dimension,
                attributes,
            } => {
                wire::encode_string_limits(writer, attribute_layer_name, 0, 128)?;
                attribute_layer_dimension.encode(writer)?;
                wire::encode_collection_limits(writer, attributes.as_slice(), 0, 1024)?;
            }
            Self::RemoveEnvironmentAttributesData {
                attribute_layer_name,
                attribute_layer_dimension,
                attributes,
            } => {
                wire::encode_string_limits(writer, attribute_layer_name, 0, 128)?;
                attribute_layer_dimension.encode(writer)?;
                wire::check_length(attributes.len(), 0, 1024)?;
                writer.write_len_var_u32(attributes.len())?;
                for item in attributes.iter() {
                    wire::encode_string_limits(writer, item, 0, 128)?;
                }
            }
        }
        Ok(())
    }
}

impl wire::Decode for AttributeLayerSyncData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let attribute_layers = wire::decode_collection_limits::<EASAttributeLayerData>(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_UPDATE_ATTRIBUTE_LAYERS_DATA_ATTRIBUTE_LAYERS, 14, 0, 512)?;
                Self::UpdateAttributeLayersData { attribute_layers }
            }
            1 => {
                let attribute_layer_name = wire::decode_string_limits(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_UPDATE_ATTRIBUTE_LAYER_SETTINGS_DATA_ATTRIBUTE_LAYER_NAME, 0, 128)?;
                let attribute_layer_dimension = <DimensionType as wire::Decode>::decode(reader)?;
                let attributes_layer_settings =
                    <EASAttributeLayerSettings as wire::Decode>::decode(reader)?;
                Self::UpdateAttributeLayerSettingsData {
                    attribute_layer_name,
                    attribute_layer_dimension,
                    attributes_layer_settings,
                }
            }
            2 => {
                let attribute_layer_name = wire::decode_string_limits(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_UPDATE_ENVIRONMENT_ATTRIBUTES_DATA_ATTRIBUTE_LAYER_NAME, 0, 128)?;
                let attribute_layer_dimension = <DimensionType as wire::Decode>::decode(reader)?;
                let attributes = wire::decode_collection_limits::<EASEnvironmentAttributeData>(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_UPDATE_ENVIRONMENT_ATTRIBUTES_DATA_ATTRIBUTES, 22, 0, 1024)?;
                Self::UpdateEnvironmentAttributesData {
                    attribute_layer_name,
                    attribute_layer_dimension,
                    attributes,
                }
            }
            3 => {
                let attribute_layer_name = wire::decode_string_limits(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_REMOVE_ENVIRONMENT_ATTRIBUTES_DATA_ATTRIBUTE_LAYER_NAME, 0, 128)?;
                let attribute_layer_dimension = <DimensionType as wire::Decode>::decode(reader)?;
                let attributes = {
                    let declared = u64::from(reader.read_var_u32()?);
                    let count = reader.checked_count_limits(crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_REMOVE_ENVIRONMENT_ATTRIBUTES_DATA_ATTRIBUTES, declared, 1, 0, 1024)?;
                    let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
                    for _ in 0..count {
                        wire::push(&mut out, wire::decode_string_limits(reader, crate::fields::ATTRIBUTE_LAYER_SYNC_DATA_REMOVE_ENVIRONMENT_ATTRIBUTES_DATA_ATTRIBUTES_ITEM, 0, 128)?)?;
                    }
                    out
                };
                Self::RemoveEnvironmentAttributesData {
                    attribute_layer_name,
                    attribute_layer_dimension,
                    attributes,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "AttributeLayerSyncData",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EAS {
    BoolAttributeData {
        value: bool,
        operation: wire::Str,
    },
    FloatAttributeData {
        value: wire::F32LE,
        operation: wire::Str,
        /// Wire presence: optional value is preceded by a presence marker.
        constraint_min: Option<wire::F32LE>,
        /// Wire presence: optional value is preceded by a presence marker.
        constraint_max: Option<wire::F32LE>,
    },
    ColorAttributeData {
        value: [wire::I32LE; 4],
        operation: wire::Str,
    },
}

impl EAS {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::BoolAttributeData { .. } => 0,
            Self::FloatAttributeData { .. } => 1,
            Self::ColorAttributeData { .. } => 2,
        }
    }
}

impl Default for EAS {
    fn default() -> Self {
        Self::BoolAttributeData {
            value: Default::default(),
            operation: Default::default(),
        }
    }
}

impl wire::Encode for EAS {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::BoolAttributeData { value, operation } => {
                value.encode(writer)?;
                operation.encode(writer)?;
            }
            Self::FloatAttributeData {
                value,
                operation,
                constraint_min,
                constraint_max,
            } => {
                value.encode(writer)?;
                operation.encode(writer)?;
                match &constraint_min {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                match &constraint_max {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
            }
            Self::ColorAttributeData { value, operation } => {
                for item in value.iter() {
                    item.encode(writer)?;
                }
                operation.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for EAS {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let value = <bool as wire::Decode>::decode(reader)?;
                let operation =
                    reader.read_str(crate::fields::EAS_BOOL_ATTRIBUTE_DATA_OPERATION)?;
                Self::BoolAttributeData { value, operation }
            }
            1 => {
                let value = <wire::F32LE as wire::Decode>::decode(reader)?;
                let operation =
                    reader.read_str(crate::fields::EAS_FLOAT_ATTRIBUTE_DATA_OPERATION)?;
                let constraint_min = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::F32LE as wire::Decode>::decode(reader)?)
                    }
                };
                let constraint_max = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::F32LE as wire::Decode>::decode(reader)?)
                    }
                };
                Self::FloatAttributeData {
                    value,
                    operation,
                    constraint_min,
                    constraint_max,
                }
            }
            2 => {
                let value = [
                    <wire::I32LE as wire::Decode>::decode(reader)?,
                    <wire::I32LE as wire::Decode>::decode(reader)?,
                    <wire::I32LE as wire::Decode>::decode(reader)?,
                    <wire::I32LE as wire::Decode>::decode(reader)?,
                ];
                let operation =
                    reader.read_str(crate::fields::EAS_COLOR_ATTRIBUTE_DATA_OPERATION)?;
                Self::ColorAttributeData { value, operation }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "EAS",
                    value: value as i64,
                });
            }
        })
    }
}

/// NoiseAlignment represents the way the noise of an environment attribute transition is aligned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EASNoiseAlignment {
    /// `type_` is the type of the alignment. It is one of the NoiseAlignmentType constants above.
    pub type_: EASNoiseAlignmentType,
    /// `value` is the value that the noise is aligned against, the meaning of which depends on Type.
    pub value: wire::VarUInt,
}

impl wire::Encode for EASNoiseAlignment {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        self.value.encode(writer)?;
        wire::check_number_limits(self.value.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for EASNoiseAlignment {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = <EASNoiseAlignmentType as wire::Decode>::decode(reader)?;
        let value = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self { type_, value })
    }
}

// Domain: bedrock_profile

/// WhiskerScopeDataSummary represents a whisker profiler scope diagnostic summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BedrockProfileWhiskerDiagnosticsScopeDataSummary {
    /// `label` is the label of the whisker scope.
    pub label: wire::Str,
    /// `indentation` is the indentation string of the whisker scope within the profiler hierarchy.
    pub indentation: wire::Str,
    /// `total_high_cost_ns` is the total time, in nanoseconds, spent in the high-cost portion of the scope.
    pub total_high_cost_ns: wire::U64LE,
    /// `total_mid_cost_ns` is the total time, in nanoseconds, spent in the mid-cost portion of the scope.
    pub total_mid_cost_ns: wire::U64LE,
    /// `total_low_cost_ns` is the total time, in nanoseconds, spent in the low-cost portion of the scope.
    pub total_low_cost_ns: wire::U64LE,
}

impl wire::Encode for BedrockProfileWhiskerDiagnosticsScopeDataSummary {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.label.encode(writer)?;
        self.indentation.encode(writer)?;
        self.total_high_cost_ns.encode(writer)?;
        self.total_mid_cost_ns.encode(writer)?;
        self.total_low_cost_ns.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BedrockProfileWhiskerDiagnosticsScopeDataSummary {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let label = reader.read_str(
            crate::fields::BEDROCK_PROFILE_WHISKER_DIAGNOSTICS_SCOPE_DATA_SUMMARY_LABEL,
        )?;
        let indentation = reader.read_str(
            crate::fields::BEDROCK_PROFILE_WHISKER_DIAGNOSTICS_SCOPE_DATA_SUMMARY_INDENTATION,
        )?;
        let total_high_cost_ns = <wire::U64LE as wire::Decode>::decode(reader)?;
        let total_mid_cost_ns = <wire::U64LE as wire::Decode>::decode(reader)?;
        let total_low_cost_ns = <wire::U64LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            label,
            indentation,
            total_high_cost_ns,
            total_mid_cost_ns,
            total_low_cost_ns,
        })
    }
}

// Domain: bedrock_safety

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BedrockSafetyRedactableString {
    pub unredacted: wire::Str,
    /// Wire presence: optional value is preceded by a presence marker.
    pub redacted: Option<wire::Str>,
}

impl wire::Encode for BedrockSafetyRedactableString {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.unredacted.encode(writer)?;
        match &self.redacted {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for BedrockSafetyRedactableString {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let unredacted =
            reader.read_str(crate::fields::BEDROCK_SAFETY_REDACTABLE_STRING_UNREDACTED)?;
        let redacted = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::BEDROCK_SAFETY_REDACTABLE_STRING_REDACTED)?)
            }
        };
        Ok(Self {
            unredacted,
            redacted,
        })
    }
}

// Domain: biome

/// BiomeCappedSurface specifies the materials to use for the capped surface of a biome, such as in the
/// Nether.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeCappedSurfaceData {
    /// `floor_blocks` is a list of runtime IDs to use for the floor blocks.
    pub floor_blocks: Vec<wire::U32LE>,
    /// `ceiling_blocks` is a list of runtime IDs to use for the ceiling blocks.
    pub ceiling_blocks: Vec<wire::U32LE>,
    /// `sea_block` is an optional runtime ID to use for the sea block.
    /// Wire presence: optional value is preceded by a presence marker.
    pub sea_block: Option<wire::U32LE>,
    /// `foundation_block` is an optional runtime ID to use for the foundation block.
    /// Wire presence: optional value is preceded by a presence marker.
    pub foundation_block: Option<wire::U32LE>,
    /// `beach_block` is an optional runtime ID to use for the beach block.
    /// Wire presence: optional value is preceded by a presence marker.
    pub beach_block: Option<wire::U32LE>,
}

impl wire::Encode for BiomeCappedSurfaceData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.floor_blocks.as_slice())?;
        wire::encode_collection(writer, self.ceiling_blocks.as_slice())?;
        match &self.sea_block {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.foundation_block {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.beach_block {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for BiomeCappedSurfaceData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let floor_blocks = wire::decode_collection::<wire::U32LE>(
            reader,
            crate::fields::BIOME_CAPPED_SURFACE_DATA_FLOOR_BLOCKS,
            4,
        )?;
        let ceiling_blocks = wire::decode_collection::<wire::U32LE>(
            reader,
            crate::fields::BIOME_CAPPED_SURFACE_DATA_CEILING_BLOCKS,
            4,
        )?;
        let sea_block = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        let foundation_block = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        let beach_block = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            floor_blocks,
            ceiling_blocks,
            sea_block,
            foundation_block,
            beach_block,
        })
    }
}

/// BiomeClimate represents the climate of a biome, mainly for ambience but also defines certain behaviours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeClimateData {
    /// `temperature` is the temperature of the biome, used for weather, biome behaviours and sky colour.
    pub temperature: wire::F32LE,
    /// `downfall` is the amount that precipitation affects colours and block changes.
    pub downfall: wire::F32LE,
    /// `snow_accumulation_min` is the minimum amount of snow that can accumulate in the biome, every 0.125 is
    /// another layer of snow.
    pub snow_accumulation_min: wire::F32LE,
    /// `snow_accumulation_max` is the maximum amount of snow that can accumulate in the biome, every 0.125 is
    /// another layer of snow.
    pub snow_accumulation_max: wire::F32LE,
}

impl wire::Encode for BiomeClimateData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.temperature.encode(writer)?;
        self.downfall.encode(writer)?;
        self.snow_accumulation_min.encode(writer)?;
        self.snow_accumulation_max.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeClimateData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let temperature = <wire::F32LE as wire::Decode>::decode(reader)?;
        let downfall = <wire::F32LE as wire::Decode>::decode(reader)?;
        let snow_accumulation_min = <wire::F32LE as wire::Decode>::decode(reader)?;
        let snow_accumulation_max = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            temperature,
            downfall,
            snow_accumulation_min,
            snow_accumulation_max,
        })
    }
}

/// BiomeConditionalTransformation is the legacy method of transforming biomes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeConditionalTransformationData {
    /// WeightedBiomes is a list of biomes and their weights.
    pub transforms_into: Vec<BiomeWeightedData>,
    /// `condition_json` is an index of the condition JSON data in the string list.
    pub condition_json: wire::U16LE,
    /// MinPassingNeighbours is the minimum number of neighbours that must pass the condition for the
    /// transformation to be applied.
    pub min_passing_neighbors: wire::U32LE,
}

impl wire::Encode for BiomeConditionalTransformationData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.transforms_into.as_slice())?;
        self.condition_json.encode(writer)?;
        self.min_passing_neighbors.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeConditionalTransformationData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let transforms_into = wire::decode_collection::<BiomeWeightedData>(
            reader,
            crate::fields::BIOME_CONDITIONAL_TRANSFORMATION_DATA_TRANSFORMS_INTO,
            6,
        )?;
        let condition_json = <wire::U16LE as wire::Decode>::decode(reader)?;
        let min_passing_neighbors = <wire::U32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            transforms_into,
            condition_json,
            min_passing_neighbors,
        })
    }
}

/// BiomeConsolidatedFeature represents a feature that is consolidated into a single feature for the biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeConsolidatedFeatureData {
    /// `scatter` defines how the feature is scattered in the biome.
    pub scatter: BiomeScatterParamData,
    /// `feature` is the index of the feature's name in the string list.
    pub feature: wire::U16LE,
    /// `identifier` is the index of the feature's identifier in the string list.
    pub identifier: wire::U16LE,
    /// `pass` is the index of the feature's pass in the string list.
    pub pass: wire::U16LE,
    /// CanUseInternal is true if the feature can use internal features.
    pub can_use_internal_feature: bool,
}

impl wire::Encode for BiomeConsolidatedFeatureData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.scatter.encode(writer)?;
        self.feature.encode(writer)?;
        self.identifier.encode(writer)?;
        self.pass.encode(writer)?;
        self.can_use_internal_feature.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeConsolidatedFeatureData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let scatter = <BiomeScatterParamData as wire::Decode>::decode(reader)?;
        let feature = <wire::U16LE as wire::Decode>::decode(reader)?;
        let identifier = <wire::U16LE as wire::Decode>::decode(reader)?;
        let pass = <wire::U16LE as wire::Decode>::decode(reader)?;
        let can_use_internal_feature = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            scatter,
            feature,
            identifier,
            pass,
            can_use_internal_feature,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeConsolidatedFeaturesData {
    pub features: Vec<BiomeConsolidatedFeatureData>,
}

impl wire::Encode for BiomeConsolidatedFeaturesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.features.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeConsolidatedFeaturesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let features = wire::decode_collection::<BiomeConsolidatedFeatureData>(
            reader,
            crate::fields::BIOME_CONSOLIDATED_FEATURES_DATA_FEATURES,
            23,
        )?;
        Ok(Self { features })
    }
}

/// BiomeCoordinate specifies coordinate rules for where features can be scattered in the biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeCoordinateData {
    /// `min_value_type` is the type of expression operation to use for the minimum value, and is one of the
    /// BiomeExpressionOp constants above.
    pub min_value_type: wire::ZigZag32,
    /// `min_value` is the index of the minimum value expression in the string list.
    pub min_value: wire::U16LE,
    /// `max_value_type` is the type of expression operation to use for the maximum value, and is one of the
    pub max_value_type: wire::ZigZag32,
    /// `max_value` is the index of the maximum value expression in the string list.
    pub max_value: wire::U16LE,
    /// `grid_offset` is the offset of the grid, used for fixed grid and jittered grid distributions.
    pub grid_offset: wire::U32LE,
    /// `grid_step_size` is the step size of the grid, used for fixed grid and jittered grid distributions.
    pub grid_step_size: wire::U32LE,
    /// `distribution` is the type of distribution to use for the coordinate, and is one of the
    /// BiomeRandomDistributionType constants above.
    pub distribution: RandomDistributionType,
}

impl wire::Encode for BiomeCoordinateData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.min_value_type.encode(writer)?;
        self.min_value.encode(writer)?;
        self.max_value_type.encode(writer)?;
        self.max_value.encode(writer)?;
        self.grid_offset.encode(writer)?;
        self.grid_step_size.encode(writer)?;
        self.distribution.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeCoordinateData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let min_value_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let min_value = <wire::U16LE as wire::Decode>::decode(reader)?;
        let max_value_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let max_value = <wire::U16LE as wire::Decode>::decode(reader)?;
        let grid_offset = <wire::U32LE as wire::Decode>::decode(reader)?;
        let grid_step_size = <wire::U32LE as wire::Decode>::decode(reader)?;
        let distribution = <RandomDistributionType as wire::Decode>::decode(reader)?;
        Ok(Self {
            min_value_type,
            min_value,
            max_value_type,
            max_value,
            grid_offset,
            grid_step_size,
            distribution,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeDefinitionChunkGenData {
    /// Wire presence: optional value is preceded by a presence marker.
    pub climate: Option<BiomeClimateData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub consolidated_features: Option<BiomeConsolidatedFeaturesData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub mountain_params: Option<BiomeMountainParamsData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub surface_material_adjustments: Option<BiomeSurfaceMaterialAdjustmentData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub overworld_gen_rules: Option<BiomeOverworldGenRulesData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub multinoise_gen_rules: Option<BiomeMultinoiseGenRulesData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub legacy_world_gen_rules: Option<BiomeLegacyWorldGenRulesData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub replacement_biomes: Option<BiomeReplacementsData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub village_type: Option<VillageType>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub surface_builder_data: Option<BiomeSurfaceBuilderData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub subsurface_builder_data: Option<BiomeSurfaceBuilderData>,
}

impl wire::Encode for BiomeDefinitionChunkGenData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.climate {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.consolidated_features {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.mountain_params {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.surface_material_adjustments {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.overworld_gen_rules {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.multinoise_gen_rules {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.legacy_world_gen_rules {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.replacement_biomes {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.village_type {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.surface_builder_data {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.subsurface_builder_data {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for BiomeDefinitionChunkGenData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let climate = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeClimateData as wire::Decode>::decode(reader)?)
            }
        };
        let consolidated_features = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeConsolidatedFeaturesData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let mountain_params = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeMountainParamsData as wire::Decode>::decode(reader)?)
            }
        };
        let surface_material_adjustments = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeSurfaceMaterialAdjustmentData as wire::Decode>::decode(reader)?)
            }
        };
        let overworld_gen_rules = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeOverworldGenRulesData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let multinoise_gen_rules = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeMultinoiseGenRulesData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let legacy_world_gen_rules = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeLegacyWorldGenRulesData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let replacement_biomes = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeReplacementsData as wire::Decode>::decode(reader)?)
            }
        };
        let village_type = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<VillageType as wire::Decode>::decode(reader)?)
            }
        };
        let surface_builder_data = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeSurfaceBuilderData as wire::Decode>::decode(reader)?)
            }
        };
        let subsurface_builder_data = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeSurfaceBuilderData as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            climate,
            consolidated_features,
            mountain_params,
            surface_material_adjustments,
            overworld_gen_rules,
            multinoise_gen_rules,
            legacy_world_gen_rules,
            replacement_biomes,
            village_type,
            surface_builder_data,
            subsurface_builder_data,
        })
    }
}

/// BiomeDefinition represents a biome definition in the game. This can be a vanilla biome or a completely
/// custom biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeDefinitionData {
    pub id: wire::U16LE,
    /// `temperature` is the temperature of the biome, used for weather, biome behaviours and sky colour.
    pub temperature: wire::F32LE,
    /// `downfall` is the amount that precipitation affects colours and block changes.
    pub downfall: wire::F32LE,
    /// `foliage_snow` is the progression factor for foliage turning white due to snow.
    pub foliage_snow: wire::F32LE,
    /// `depth` is the depth of the biome.
    pub depth: wire::F32LE,
    /// `scale` is the scale of the biome.
    pub scale: wire::F32LE,
    pub map_water_color_argb: wire::I32LE,
    /// `rain` is true if the biome has rain, false if it is a dry biome.
    pub rain: bool,
    /// `tags` are a list of indices of tags in the string list. These are used to group biomes together for biome
    /// generation and other purposes.
    /// Wire presence: optional value is preceded by a presence marker.
    pub tags: Option<BiomeTagsData>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub chunk_gen_data: Option<BiomeDefinitionChunkGenData>,
}

impl wire::Encode for BiomeDefinitionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        self.temperature.encode(writer)?;
        self.downfall.encode(writer)?;
        self.foliage_snow.encode(writer)?;
        self.depth.encode(writer)?;
        self.scale.encode(writer)?;
        self.map_water_color_argb.encode(writer)?;
        self.rain.encode(writer)?;
        match &self.tags {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.chunk_gen_data {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for BiomeDefinitionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = <wire::U16LE as wire::Decode>::decode(reader)?;
        let temperature = <wire::F32LE as wire::Decode>::decode(reader)?;
        let downfall = <wire::F32LE as wire::Decode>::decode(reader)?;
        let foliage_snow = <wire::F32LE as wire::Decode>::decode(reader)?;
        let depth = <wire::F32LE as wire::Decode>::decode(reader)?;
        let scale = <wire::F32LE as wire::Decode>::decode(reader)?;
        let map_water_color_argb = <wire::I32LE as wire::Decode>::decode(reader)?;
        let rain = <bool as wire::Decode>::decode(reader)?;
        let tags = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeTagsData as wire::Decode>::decode(reader)?)
            }
        };
        let chunk_gen_data = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeDefinitionChunkGenData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        Ok(Self {
            id,
            temperature,
            downfall,
            foliage_snow,
            depth,
            scale,
            map_water_color_argb,
            rain,
            tags,
            chunk_gen_data,
        })
    }
}

/// BiomeElementData are set rules to adjust the surface materials of the biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeElementData {
    /// NoiseFrequencyScale is the frequency scale of the noise used to adjust the surface materials.
    pub noise_freq_scale: wire::F32LE,
    /// `noise_lower_bound` is the minimum noise value required to be selected.
    pub noise_lower_bound: wire::F32LE,
    /// `noise_upper_bound` is the maximum noise value required to be selected.
    pub noise_upper_bound: wire::F32LE,
    /// `height_min_type` is the type of expression operation to use for the minimum height, and is one of the
    /// BiomeExpressionOp constants above.
    pub height_min_type: wire::ZigZag32,
    /// `height_min` is the index of the minimum height expression in the string list.
    pub height_min: wire::U16LE,
    /// `height_max_type` is the type of expression operation to use for the maximum height, and is one of the
    /// BiomeExpressionOp constants above.
    pub height_max_type: wire::ZigZag32,
    /// `height_max` is the index of the maximum height expression in the string list.
    pub height_max: wire::U16LE,
    /// `adjusted_materials` is the materials to use for the surface layers of the biome if selected.
    pub adjusted_materials: BiomeSurfaceMaterialData,
}

impl wire::Encode for BiomeElementData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.noise_freq_scale.encode(writer)?;
        self.noise_lower_bound.encode(writer)?;
        self.noise_upper_bound.encode(writer)?;
        self.height_min_type.encode(writer)?;
        self.height_min.encode(writer)?;
        self.height_max_type.encode(writer)?;
        self.height_max.encode(writer)?;
        self.adjusted_materials.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeElementData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let noise_freq_scale = <wire::F32LE as wire::Decode>::decode(reader)?;
        let noise_lower_bound = <wire::F32LE as wire::Decode>::decode(reader)?;
        let noise_upper_bound = <wire::F32LE as wire::Decode>::decode(reader)?;
        let height_min_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let height_min = <wire::U16LE as wire::Decode>::decode(reader)?;
        let height_max_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let height_max = <wire::U16LE as wire::Decode>::decode(reader)?;
        let adjusted_materials = <BiomeSurfaceMaterialData as wire::Decode>::decode(reader)?;
        Ok(Self {
            noise_freq_scale,
            noise_lower_bound,
            noise_upper_bound,
            height_min_type,
            height_min,
            height_max_type,
            height_max,
            adjusted_materials,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeLegacyWorldGenRulesData {
    pub legacy_pre_hills_edge: Vec<BiomeConditionalTransformationData>,
}

impl wire::Encode for BiomeLegacyWorldGenRulesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.legacy_pre_hills_edge.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeLegacyWorldGenRulesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let legacy_pre_hills_edge = wire::decode_collection::<BiomeConditionalTransformationData>(
            reader,
            crate::fields::BIOME_LEGACY_WORLD_GEN_RULES_DATA_LEGACY_PRE_HILLS_EDGE,
            7,
        )?;
        Ok(Self {
            legacy_pre_hills_edge,
        })
    }
}

/// BiomeMesaSurface specifies the materials to use for the mesa biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeMesaSurfaceData {
    /// `clay_material` is the runtime ID of the block to use for clay layers.
    pub clay_material: wire::U32LE,
    /// `hard_clay_material` is the runtime ID of the block to use for hard clay layers.
    pub hard_clay_material: wire::U32LE,
    /// `bryce_pillars` is true if the biome has bryce pillars, which are tall spire-like structures.
    pub bryce_pillars: bool,
    /// `has_forest` is true if the biome has a forest.
    pub has_forest: bool,
}

impl wire::Encode for BiomeMesaSurfaceData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.clay_material.encode(writer)?;
        self.hard_clay_material.encode(writer)?;
        self.bryce_pillars.encode(writer)?;
        self.has_forest.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeMesaSurfaceData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let clay_material = <wire::U32LE as wire::Decode>::decode(reader)?;
        let hard_clay_material = <wire::U32LE as wire::Decode>::decode(reader)?;
        let bryce_pillars = <bool as wire::Decode>::decode(reader)?;
        let has_forest = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            clay_material,
            hard_clay_material,
            bryce_pillars,
            has_forest,
        })
    }
}

/// BiomeMountainParameters specifies the parameters for a mountain biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeMountainParamsData {
    /// `steep_block` is the runtime ID of the block to use for steep slopes.
    pub steep_block: wire::U32LE,
    /// `north_slopes` is true if the biome has north slopes.
    pub north_slopes: bool,
    /// `south_slopes` is true if the biome has south slopes.
    pub south_slopes: bool,
    /// `west_slopes` is true if the biome has west slopes.
    pub west_slopes: bool,
    /// `east_slopes` is true if the biome has east slopes.
    pub east_slopes: bool,
    /// `top_slide_enabled` is true if the biome has top slide enabled.
    pub top_slide_enabled: bool,
}

impl wire::Encode for BiomeMountainParamsData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.steep_block.encode(writer)?;
        self.north_slopes.encode(writer)?;
        self.south_slopes.encode(writer)?;
        self.west_slopes.encode(writer)?;
        self.east_slopes.encode(writer)?;
        self.top_slide_enabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeMountainParamsData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let steep_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let north_slopes = <bool as wire::Decode>::decode(reader)?;
        let south_slopes = <bool as wire::Decode>::decode(reader)?;
        let west_slopes = <bool as wire::Decode>::decode(reader)?;
        let east_slopes = <bool as wire::Decode>::decode(reader)?;
        let top_slide_enabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            steep_block,
            north_slopes,
            south_slopes,
            west_slopes,
            east_slopes,
            top_slide_enabled,
        })
    }
}

/// BiomeMultiNoiseRules specifies the rules for multi-noise biomes, which are biomes that are defined by
/// multiple noise parameters instead of just temperature and humidity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeMultinoiseGenRulesData {
    /// `temperature` is the temperature level of the biome.
    pub temperature: wire::F32LE,
    /// `humidity` is the humidity level of the biome.
    pub humidity: wire::F32LE,
    /// `altitude` is the altitude level of the biome.
    pub altitude: wire::F32LE,
    /// `weirdness` is the weirdness level of the biome.
    pub weirdness: wire::F32LE,
    /// `weight` is the weight of the biome, with a higher weight being more likely to be selected.
    pub weight: wire::F32LE,
}

impl wire::Encode for BiomeMultinoiseGenRulesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.temperature.encode(writer)?;
        self.humidity.encode(writer)?;
        self.altitude.encode(writer)?;
        self.weirdness.encode(writer)?;
        self.weight.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeMultinoiseGenRulesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let temperature = <wire::F32LE as wire::Decode>::decode(reader)?;
        let humidity = <wire::F32LE as wire::Decode>::decode(reader)?;
        let altitude = <wire::F32LE as wire::Decode>::decode(reader)?;
        let weirdness = <wire::F32LE as wire::Decode>::decode(reader)?;
        let weight = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            temperature,
            humidity,
            altitude,
            weirdness,
            weight,
        })
    }
}

/// BiomeNoiseGradientSurface specifies noise-gradient surface block data for a biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeNoiseGradientSurfaceData {
    /// `non_replaceable_blocks` is a list of block runtime IDs that may not be replaced.
    pub non_replaceable_blocks: Vec<wire::U32LE>,
    /// `gradient_blocks` is a list of noise block specifiers used by the gradient.
    pub gradient_blocks: Vec<SerializedNoiseBlockSpecifier>,
    /// `noise` is the noise descriptor used by the gradient.
    pub noise: NoiseDescriptor,
}

impl wire::Encode for BiomeNoiseGradientSurfaceData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.non_replaceable_blocks.as_slice())?;
        wire::encode_collection(writer, self.gradient_blocks.as_slice())?;
        self.noise.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeNoiseGradientSurfaceData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let non_replaceable_blocks = wire::decode_collection::<wire::U32LE>(
            reader,
            crate::fields::BIOME_NOISE_GRADIENT_SURFACE_DATA_NON_REPLACEABLE_BLOCKS,
            4,
        )?;
        let gradient_blocks = wire::decode_collection::<SerializedNoiseBlockSpecifier>(
            reader,
            crate::fields::BIOME_NOISE_GRADIENT_SURFACE_DATA_GRADIENT_BLOCKS,
            17,
        )?;
        let noise = <NoiseDescriptor as wire::Decode>::decode(reader)?;
        Ok(Self {
            non_replaceable_blocks,
            gradient_blocks,
            noise,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeOverworldGenRulesData {
    pub hills_transformations: Vec<BiomeWeightedData>,
    pub mutate_transformations: Vec<BiomeWeightedData>,
    pub river_transformations: Vec<BiomeWeightedData>,
    pub shore_transformations: Vec<BiomeWeightedData>,
    pub pre_hills_edge: Vec<BiomeConditionalTransformationData>,
    pub post_shore_edge: Vec<BiomeConditionalTransformationData>,
    pub climate: Vec<BiomeWeightedTemperatureData>,
}

impl wire::Encode for BiomeOverworldGenRulesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.hills_transformations.as_slice())?;
        wire::encode_collection(writer, self.mutate_transformations.as_slice())?;
        wire::encode_collection(writer, self.river_transformations.as_slice())?;
        wire::encode_collection(writer, self.shore_transformations.as_slice())?;
        wire::encode_collection(writer, self.pre_hills_edge.as_slice())?;
        wire::encode_collection(writer, self.post_shore_edge.as_slice())?;
        wire::encode_collection(writer, self.climate.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeOverworldGenRulesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let hills_transformations = wire::decode_collection::<BiomeWeightedData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_HILLS_TRANSFORMATIONS,
            6,
        )?;
        let mutate_transformations = wire::decode_collection::<BiomeWeightedData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_MUTATE_TRANSFORMATIONS,
            6,
        )?;
        let river_transformations = wire::decode_collection::<BiomeWeightedData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_RIVER_TRANSFORMATIONS,
            6,
        )?;
        let shore_transformations = wire::decode_collection::<BiomeWeightedData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_SHORE_TRANSFORMATIONS,
            6,
        )?;
        let pre_hills_edge = wire::decode_collection::<BiomeConditionalTransformationData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_PRE_HILLS_EDGE,
            7,
        )?;
        let post_shore_edge = wire::decode_collection::<BiomeConditionalTransformationData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_POST_SHORE_EDGE,
            7,
        )?;
        let climate = wire::decode_collection::<BiomeWeightedTemperatureData>(
            reader,
            crate::fields::BIOME_OVERWORLD_GEN_RULES_DATA_CLIMATE,
            5,
        )?;
        Ok(Self {
            hills_transformations,
            mutate_transformations,
            river_transformations,
            shore_transformations,
            pre_hills_edge,
            post_shore_edge,
            climate,
        })
    }
}

/// BiomeReplacementData represents data for biome replacements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeReplacementData {
    /// Biome is the biome ID to replace.
    pub replacement_biome: wire::U16LE,
    /// `dimension` is the dimension ID where the replacement applies.
    pub dimension: wire::U16LE,
    /// `target_biomes` is a list of target biome IDs for the replacement.
    pub target_biomes: Vec<wire::U16LE>,
    /// `amount` is the amount of replacement to apply.
    pub amount: wire::F32LE,
    /// `noise_frequency_scale` ...
    pub noise_frequency_scale: wire::F32LE,
    /// `replacement_index` is the index of the replacement.
    pub replacement_index: wire::U32LE,
}

impl wire::Encode for BiomeReplacementData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.replacement_biome.encode(writer)?;
        self.dimension.encode(writer)?;
        wire::encode_collection(writer, self.target_biomes.as_slice())?;
        self.amount.encode(writer)?;
        self.noise_frequency_scale.encode(writer)?;
        self.replacement_index.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeReplacementData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let replacement_biome = <wire::U16LE as wire::Decode>::decode(reader)?;
        let dimension = <wire::U16LE as wire::Decode>::decode(reader)?;
        let target_biomes = wire::decode_collection::<wire::U16LE>(
            reader,
            crate::fields::BIOME_REPLACEMENT_DATA_TARGET_BIOMES,
            2,
        )?;
        let amount = <wire::F32LE as wire::Decode>::decode(reader)?;
        let noise_frequency_scale = <wire::F32LE as wire::Decode>::decode(reader)?;
        let replacement_index = <wire::U32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            replacement_biome,
            dimension,
            target_biomes,
            amount,
            noise_frequency_scale,
            replacement_index,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeReplacementsData {
    pub biome_replacements: Vec<BiomeReplacementData>,
}

impl wire::Encode for BiomeReplacementsData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.biome_replacements.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeReplacementsData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let biome_replacements = wire::decode_collection::<BiomeReplacementData>(
            reader,
            crate::fields::BIOME_REPLACEMENTS_DATA_BIOME_REPLACEMENTS,
            17,
        )?;
        Ok(Self { biome_replacements })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeScatterParamData {
    /// `coordinates` is a list of coordinate rules to scatter the feature within.
    pub coordinates: Vec<BiomeCoordinateData>,
    /// EvaluationOrder is the order in which the coordinates are evaluated, and is one of the
    /// CoordinateEvaluationOrder constants above.
    pub eval_order: CoordinateEvaluationOrder,
    /// `chance_percent_type` is the type of expression operation to use for the chance percent, and is one of the
    /// BiomeExpressionOp constants above.
    pub chance_percent_type: wire::ZigZag32,
    /// ChangePercent is the index of the chance expression in the string list.
    pub chance_percent: wire::U16LE,
    /// `chance_numerator` is the numerator of the chance expression.
    pub chance_numerator: wire::I32LE,
    /// `chance_denominator` is the denominator of the chance expression.
    pub chance_denominator: wire::I32LE,
    /// `iterations_type` is the type of expression operation to use for the iterations, and is one of the
    /// BiomeExpressionOp constants above.
    pub iterations_type: wire::ZigZag32,
    /// `iterations` is the index of the iterations expression in the string list.
    pub iterations: wire::U16LE,
}

impl wire::Encode for BiomeScatterParamData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.coordinates.as_slice())?;
        self.eval_order.encode(writer)?;
        self.chance_percent_type.encode(writer)?;
        self.chance_percent.encode(writer)?;
        self.chance_numerator.encode(writer)?;
        self.chance_denominator.encode(writer)?;
        self.iterations_type.encode(writer)?;
        self.iterations.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeScatterParamData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let coordinates = wire::decode_collection::<BiomeCoordinateData>(
            reader,
            crate::fields::BIOME_SCATTER_PARAM_DATA_COORDINATES,
            15,
        )?;
        let eval_order = <CoordinateEvaluationOrder as wire::Decode>::decode(reader)?;
        let chance_percent_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let chance_percent = <wire::U16LE as wire::Decode>::decode(reader)?;
        let chance_numerator = <wire::I32LE as wire::Decode>::decode(reader)?;
        let chance_denominator = <wire::I32LE as wire::Decode>::decode(reader)?;
        let iterations_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let iterations = <wire::U16LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            coordinates,
            eval_order,
            chance_percent_type,
            chance_percent,
            chance_numerator,
            chance_denominator,
            iterations_type,
            iterations,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeStringList {
    pub strings: Vec<wire::Str>,
}

impl wire::Encode for BiomeStringList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.strings.len())?;
        for item in self.strings.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for BiomeStringList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let strings = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::BIOME_STRING_LIST_STRINGS, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::BIOME_STRING_LIST_STRINGS_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self { strings })
    }
}

/// BiomeSurfaceBuilder specifies the materials and special surface rules to use for a biome surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeSurfaceBuilderData {
    /// `surface_materials` is a set of materials to use for the surface layers of the biome.
    /// Wire presence: optional value is preceded by a presence marker.
    pub surface_materials: Option<BiomeSurfaceMaterialData>,
    /// `has_default_overworld_surface` is true if the biome has a default overworld surface.
    pub has_default_overworld_surface: bool,
    /// `has_swamp_surface` is true if the biome has a swamp surface.
    pub has_swamp_surface: bool,
    /// `has_frozen_ocean_surface` is true if the biome has a frozen ocean surface.
    pub has_frozen_ocean_surface: bool,
    /// `has_the_end_surface` is true if the biome has an end surface.
    pub has_the_end_surface: bool,
    /// `mesa_surface` is optional information to specify the biome's mesa surface.
    /// Wire presence: optional value is preceded by a presence marker.
    pub mesa_surface: Option<BiomeMesaSurfaceData>,
    /// `capped_surface` is optional information to specify the biome's capped surface, i.e. in the Nether.
    /// Wire presence: optional value is preceded by a presence marker.
    pub capped_surface: Option<BiomeCappedSurfaceData>,
    /// `noise_gradient_surface` is optional information to specify noise-gradient surface data.
    /// Wire presence: optional value is preceded by a presence marker.
    pub noise_gradient_surface: Option<BiomeNoiseGradientSurfaceData>,
}

impl wire::Encode for BiomeSurfaceBuilderData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.surface_materials {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.has_default_overworld_surface.encode(writer)?;
        self.has_swamp_surface.encode(writer)?;
        self.has_frozen_ocean_surface.encode(writer)?;
        self.has_the_end_surface.encode(writer)?;
        match &self.mesa_surface {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.capped_surface {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.noise_gradient_surface {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for BiomeSurfaceBuilderData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let surface_materials = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeSurfaceMaterialData as wire::Decode>::decode(reader)?)
            }
        };
        let has_default_overworld_surface = <bool as wire::Decode>::decode(reader)?;
        let has_swamp_surface = <bool as wire::Decode>::decode(reader)?;
        let has_frozen_ocean_surface = <bool as wire::Decode>::decode(reader)?;
        let has_the_end_surface = <bool as wire::Decode>::decode(reader)?;
        let mesa_surface = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeMesaSurfaceData as wire::Decode>::decode(reader)?)
            }
        };
        let capped_surface = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeCappedSurfaceData as wire::Decode>::decode(reader)?)
            }
        };
        let noise_gradient_surface = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BiomeNoiseGradientSurfaceData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        Ok(Self {
            surface_materials,
            has_default_overworld_surface,
            has_swamp_surface,
            has_frozen_ocean_surface,
            has_the_end_surface,
            mesa_surface,
            capped_surface,
            noise_gradient_surface,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeSurfaceMaterialAdjustmentData {
    pub adjustments: Vec<BiomeElementData>,
}

impl wire::Encode for BiomeSurfaceMaterialAdjustmentData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.adjustments.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeSurfaceMaterialAdjustmentData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let adjustments = wire::decode_collection::<BiomeElementData>(
            reader,
            crate::fields::BIOME_SURFACE_MATERIAL_ADJUSTMENT_DATA_ADJUSTMENTS,
            42,
        )?;
        Ok(Self { adjustments })
    }
}

/// BiomeSurfaceMaterial specifies the materials to use for the surface layers of the biome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeSurfaceMaterialData {
    /// `top_block` is the runtime ID of the block to use for the top layer.
    pub top_block: wire::U32LE,
    /// `mid_block` is the runtime ID to use for the middle layers.
    pub mid_block: wire::U32LE,
    /// `sea_floor_block` is the runtime ID to use for the sea floor.
    pub sea_floor_block: wire::U32LE,
    /// `foundation_block` is the runtime ID to use for the foundation layers.
    pub foundation_block: wire::U32LE,
    /// `sea_block` is the runtime ID to use for the sea layers.
    pub sea_block: wire::U32LE,
    /// `sea_floor_depth` is the depth of the sea floor, in blocks.
    pub sea_floor_depth: wire::I32LE,
}

impl wire::Encode for BiomeSurfaceMaterialData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.top_block.encode(writer)?;
        self.mid_block.encode(writer)?;
        self.sea_floor_block.encode(writer)?;
        self.foundation_block.encode(writer)?;
        self.sea_block.encode(writer)?;
        self.sea_floor_depth.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeSurfaceMaterialData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let top_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let mid_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let sea_floor_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let foundation_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let sea_block = <wire::U32LE as wire::Decode>::decode(reader)?;
        let sea_floor_depth = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            top_block,
            mid_block,
            sea_floor_block,
            foundation_block,
            sea_block,
            sea_floor_depth,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeTagsData {
    pub tags: Vec<wire::U16LE>,
}

impl wire::Encode for BiomeTagsData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.tags.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for BiomeTagsData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let tags =
            wire::decode_collection::<wire::U16LE>(reader, crate::fields::BIOME_TAGS_DATA_TAGS, 2)?;
        Ok(Self { tags })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeWeightedData {
    pub biome_identifier: wire::U16LE,
    pub weight: wire::U32LE,
}

impl wire::Encode for BiomeWeightedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.biome_identifier.encode(writer)?;
        self.weight.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeWeightedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let biome_identifier = <wire::U16LE as wire::Decode>::decode(reader)?;
        let weight = <wire::U32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            biome_identifier,
            weight,
        })
    }
}

/// BiomeTemperatureWeight defines the weight for a temperature, used for weighted randomness.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeWeightedTemperatureData {
    /// `temperature` is the temperature that can be selected.
    pub temperature: wire::ZigZag32,
    /// `weight` is the weight of the temperature, with a higher weight being more likely to be selected.
    pub weight: wire::U32LE,
}

impl wire::Encode for BiomeWeightedTemperatureData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.temperature.encode(writer)?;
        self.weight.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeWeightedTemperatureData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let temperature = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let weight = <wire::U32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            temperature,
            weight,
        })
    }
}

/// FloatRange is an inclusive minimum/maximum pair of float32 values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloatRange {
    /// `min` is the minimum value of the range.
    pub min: wire::F32LE,
    /// `max` is the maximum value of the range.
    pub max: wire::F32LE,
}

impl wire::Encode for FloatRange {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.min.encode(writer)?;
        self.max.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FloatRange {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let min = <wire::F32LE as wire::Decode>::decode(reader)?;
        let max = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self { min, max })
    }
}

/// NoiseDescriptor describes the gradient noise used by a BiomeNoiseGradientSurface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoiseDescriptor {
    /// `name` is the string used to initialise the noise.
    pub name: wire::Str,
    /// `first_octave` is the first octave used by the noise.
    pub first_octave: wire::I32LE,
    /// `amplitudes` is a list of amplitude values used by the noise. It must contain between 1 and 100 entries.
    pub amplitudes: Vec<wire::F32LE>,
}

impl wire::Encode for NoiseDescriptor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.first_octave.encode(writer)?;
        wire::encode_collection_limits(writer, self.amplitudes.as_slice(), 1, 100)?;
        Ok(())
    }
}

impl wire::Decode for NoiseDescriptor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::NOISE_DESCRIPTOR_NAME)?;
        let first_octave = <wire::I32LE as wire::Decode>::decode(reader)?;
        let amplitudes = wire::decode_collection_limits::<wire::F32LE>(
            reader,
            crate::fields::NOISE_DESCRIPTOR_AMPLITUDES,
            4,
            1,
            100,
        )?;
        Ok(Self {
            name,
            first_octave,
            amplitudes,
        })
    }
}

// Domain: block_pos

/// BlockPos is the position of a block. It is composed of three integers, and is typically written as either
/// 3 varint32s or a varint32, varuint32 and varint32.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockPos {
    pub x: wire::ZigZag32,
    pub y: wire::ZigZag32,
    pub z: wire::ZigZag32,
}

impl wire::Encode for BlockPos {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.x.encode(writer)?;
        self.y.encode(writer)?;
        self.z.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BlockPos {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let x = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let y = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let z = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { x, y, z })
    }
}

// Domain: book

#[derive(Clone, Debug, PartialEq)]
pub enum BookEditAction {
    ReplacePage {
        page_index: wire::ZigZag32,
        page_text: wire::Str,
        photo_name: wire::Str,
    },
    AddPage {
        page_index: wire::ZigZag32,
        page_text: wire::Str,
        photo_name: wire::Str,
    },
    DeletePage {
        page_index: wire::ZigZag32,
    },
    SwapPages {
        page_index: wire::ZigZag32,
        swap_with_index: wire::ZigZag32,
    },
    Finalize {
        title: wire::Str,
        author: wire::Str,
        xuid: wire::Str,
    },
}

impl BookEditAction {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::ReplacePage { .. } => 0,
            Self::AddPage { .. } => 1,
            Self::DeletePage { .. } => 2,
            Self::SwapPages { .. } => 3,
            Self::Finalize { .. } => 4,
        }
    }
}

impl Default for BookEditAction {
    fn default() -> Self {
        Self::ReplacePage {
            page_index: Default::default(),
            page_text: Default::default(),
            photo_name: Default::default(),
        }
    }
}

impl wire::Encode for BookEditAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::ReplacePage {
                page_index,
                page_text,
                photo_name,
            } => {
                page_index.encode(writer)?;
                wire::encode_string_limits(writer, page_text, 0, 768)?;
                wire::encode_string_limits(writer, photo_name, 0, 768)?;
            }
            Self::AddPage {
                page_index,
                page_text,
                photo_name,
            } => {
                page_index.encode(writer)?;
                wire::encode_string_limits(writer, page_text, 0, 768)?;
                wire::encode_string_limits(writer, photo_name, 0, 768)?;
            }
            Self::DeletePage { page_index } => {
                page_index.encode(writer)?;
            }
            Self::SwapPages {
                page_index,
                swap_with_index,
            } => {
                page_index.encode(writer)?;
                swap_with_index.encode(writer)?;
            }
            Self::Finalize {
                title,
                author,
                xuid,
            } => {
                wire::encode_string_limits(writer, title, 0, 768)?;
                wire::encode_string_limits(writer, author, 0, 768)?;
                wire::encode_string_limits(writer, xuid, 0, 768)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for BookEditAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let page_index = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let page_text = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_REPLACE_PAGE_PAGE_TEXT,
                    0,
                    768,
                )?;
                let photo_name = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_REPLACE_PAGE_PHOTO_NAME,
                    0,
                    768,
                )?;
                Self::ReplacePage {
                    page_index,
                    page_text,
                    photo_name,
                }
            }
            1 => {
                let page_index = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let page_text = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_ADD_PAGE_PAGE_TEXT,
                    0,
                    768,
                )?;
                let photo_name = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_ADD_PAGE_PHOTO_NAME,
                    0,
                    768,
                )?;
                Self::AddPage {
                    page_index,
                    page_text,
                    photo_name,
                }
            }
            2 => {
                let page_index = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::DeletePage { page_index }
            }
            3 => {
                let page_index = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let swap_with_index = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::SwapPages {
                    page_index,
                    swap_with_index,
                }
            }
            4 => {
                let title = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_FINALIZE_TITLE,
                    0,
                    768,
                )?;
                let author = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_FINALIZE_AUTHOR,
                    0,
                    768,
                )?;
                let xuid = wire::decode_string_limits(
                    reader,
                    crate::fields::BOOK_EDIT_ACTION_FINALIZE_XUID,
                    0,
                    768,
                )?;
                Self::Finalize {
                    title,
                    author,
                    xuid,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "BookEditAction",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: camera

/// CameraAimAssistActorPriorityData represents priority data for aim assist actor targeting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistActorPriorityData {
    /// `preset_index` is the index of the aim assist preset.
    pub preset_index: wire::I32LE,
    /// `category_index` is the index of the aim assist category.
    pub category_index: wire::I32LE,
    /// `actor_index` is the index of the actor.
    pub actor_index: wire::I32LE,
    /// `priority_value` is the priority value for this actor.
    pub priority_value: wire::I32LE,
}

impl wire::Encode for CameraAimAssistActorPriorityData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.preset_index.encode(writer)?;
        self.category_index.encode(writer)?;
        self.actor_index.encode(writer)?;
        self.priority_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistActorPriorityData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let preset_index = <wire::I32LE as wire::Decode>::decode(reader)?;
        let category_index = <wire::I32LE as wire::Decode>::decode(reader)?;
        let actor_index = <wire::I32LE as wire::Decode>::decode(reader)?;
        let priority_value = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            preset_index,
            category_index,
            actor_index,
            priority_value,
        })
    }
}

/// CameraAimAssistCategory is an aim assist category that defines priorities for specific blocks and
/// entities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistCategoryDefinition {
    /// `name` is the name of the category which can be used by a CameraAimAssistPreset.
    pub name: wire::Str,
    /// `priorities` represents the block and entity specific priorities as well as the default priorities for
    /// this category.
    pub priorities: CameraAimAssistCategoryPriorities,
}

impl wire::Encode for CameraAimAssistCategoryDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.priorities.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistCategoryDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::CAMERA_AIM_ASSIST_CATEGORY_DEFINITION_NAME)?;
        let priorities = <CameraAimAssistCategoryPriorities as wire::Decode>::decode(reader)?;
        Ok(Self { name, priorities })
    }
}

/// CameraAimAssistPriorities represents the block and entity specific priorities for targetting. The aim
/// assist will select the block or entity with the highest priority within the specified thresholds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistCategoryPriorities {
    /// `entities` is a list of priorities for specific entity identifiers.
    pub entities: Vec<(wire::Str, wire::I32LE)>,
    /// `blocks` is a list of priorities for specific block identifiers.
    pub blocks: Vec<(wire::Str, wire::I32LE)>,
    /// `block_tags` is a list of priorities for specific block tags.
    pub block_tags: Vec<(wire::Str, wire::I32LE)>,
    /// `entity_type_families` is a list of priorities for specific entity type families.
    pub entity_type_families: Vec<(wire::Str, wire::I32LE)>,
    /// `entity_default` is the default priority for entities.
    /// Wire presence: optional value is preceded by a presence marker.
    pub entity_default: Option<wire::I32LE>,
    /// `block_default` is the default priority for blocks.
    /// Wire presence: optional value is preceded by a presence marker.
    pub block_default: Option<wire::I32LE>,
}

impl wire::Encode for CameraAimAssistCategoryPriorities {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.entities.len())?;
        for (key, value) in self.entities.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
            wire::check_number_limits(value.0, Some(0), Some(100))?;
        }
        writer.write_len_var_u32(self.blocks.len())?;
        for (key, value) in self.blocks.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
            wire::check_number_limits(value.0, Some(0), Some(100))?;
        }
        writer.write_len_var_u32(self.block_tags.len())?;
        for (key, value) in self.block_tags.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
            wire::check_number_limits(value.0, Some(0), Some(100))?;
        }
        writer.write_len_var_u32(self.entity_type_families.len())?;
        for (key, value) in self.entity_type_families.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
            wire::check_number_limits(value.0, Some(0), Some(100))?;
        }
        match &self.entity_default {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
                wire::check_number_limits(value.0, Some(0), Some(100))?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.block_default {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
                wire::check_number_limits(value.0, Some(0), Some(100))?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistCategoryPriorities {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entities = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_ENTITIES,
                declared,
                5,
            )?;
            let mut out: Vec<(wire::Str, wire::I32LE)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader
                    .read_str(crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_ENTITIES_KEY)?;
                let value = {
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                };
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let blocks = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_BLOCKS,
                declared,
                5,
            )?;
            let mut out: Vec<(wire::Str, wire::I32LE)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader
                    .read_str(crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_BLOCKS_KEY)?;
                let value = {
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                };
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let block_tags = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_BLOCK_TAGS,
                declared,
                5,
            )?;
            let mut out: Vec<(wire::Str, wire::I32LE)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_BLOCK_TAGS_KEY,
                )?;
                let value = {
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                };
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let entity_type_families = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_ENTITY_TYPE_FAMILIES,
                declared,
                5,
            )?;
            let mut out: Vec<(wire::Str, wire::I32LE)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_CATEGORY_PRIORITIES_ENTITY_TYPE_FAMILIES_KEY,
                )?;
                let value = {
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                };
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let entity_default = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some({
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                })
            }
        };
        let block_default = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some({
                    let value = <wire::I32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(100))?;
                    value
                })
            }
        };
        Ok(Self {
            entities,
            blocks,
            block_tags,
            entity_type_families,
            entity_default,
            block_default,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistCommandPresetDefinition {
    /// Wire presence: optional value is preceded by a presence marker.
    pub preset_id: Option<wire::Str>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub target_mode: Option<CameraAimAssistTargetMode>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub view_angle: Option<glam::Vec2>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub distance: Option<wire::F32LE>,
}

impl wire::Encode for CameraAimAssistCommandPresetDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.preset_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.target_mode {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.view_angle {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.distance {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistCommandPresetDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let preset_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_COMMAND_PRESET_DEFINITION_PRESET_ID,
                )?)
            }
        };
        let target_mode = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraAimAssistTargetMode as wire::Decode>::decode(reader)?)
            }
        };
        let view_angle = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let distance = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            preset_id,
            target_mode,
            view_angle,
            distance,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistPresetDefinition {
    pub identifier: wire::Str,
    pub exclusion_settings: CameraAimAssistPresetExclusionDefinition,
    pub liquid_targeting_list: Vec<wire::Str>,
    pub item_settings: Vec<(wire::Str, wire::Str)>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub default_item_settings: Option<wire::Str>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub hand_settings: Option<wire::Str>,
}

impl wire::Encode for CameraAimAssistPresetDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.identifier.encode(writer)?;
        self.exclusion_settings.encode(writer)?;
        writer.write_len_var_u32(self.liquid_targeting_list.len())?;
        for item in self.liquid_targeting_list.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.item_settings.len())?;
        for (key, value) in self.item_settings.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
        }
        match &self.default_item_settings {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.hand_settings {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistPresetDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let identifier =
            reader.read_str(crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_IDENTIFIER)?;
        let exclusion_settings =
            <CameraAimAssistPresetExclusionDefinition as wire::Decode>::decode(reader)?;
        let liquid_targeting_list = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_LIQUID_TARGETING_LIST,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(&mut out, reader.read_str(crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_LIQUID_TARGETING_LIST_ITEM)?)?;
            }
            out
        };
        let item_settings = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_ITEM_SETTINGS,
                declared,
                2,
            )?;
            let mut out: Vec<(wire::Str, wire::Str)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_ITEM_SETTINGS_KEY,
                )?;
                let value = reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_ITEM_SETTINGS_VALUE,
                )?;
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let default_item_settings = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(
                    crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_DEFAULT_ITEM_SETTINGS,
                )?)
            }
        };
        let hand_settings =
            {
                if reader.read_u8()? == 0 {
                    None
                } else {
                    Some(reader.read_str(
                        crate::fields::CAMERA_AIM_ASSIST_PRESET_DEFINITION_HAND_SETTINGS,
                    )?)
                }
            };
        Ok(Self {
            identifier,
            exclusion_settings,
            liquid_targeting_list,
            item_settings,
            default_item_settings,
            hand_settings,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistPresetExclusionDefinition {
    pub blocks: Vec<wire::Str>,
    pub entities: Vec<wire::Str>,
    pub block_tags: Vec<wire::Str>,
    pub entity_type_families: Vec<wire::Str>,
}

impl wire::Encode for CameraAimAssistPresetExclusionDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.blocks.len())?;
        for item in self.blocks.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.entities.len())?;
        for item in self.entities.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.block_tags.len())?;
        for item in self.block_tags.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.entity_type_families.len())?;
        for item in self.entity_type_families.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistPresetExclusionDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let blocks = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_BLOCKS,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(
                        crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_BLOCKS_ITEM,
                    )?,
                )?;
            }
            out
        };
        let entities = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_ENTITIES,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(
                        crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_ENTITIES_ITEM,
                    )?,
                )?;
            }
            out
        };
        let block_tags = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_BLOCK_TAGS,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(&mut out, reader.read_str(crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_BLOCK_TAGS_ITEM)?)?;
            }
            out
        };
        let entity_type_families = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_ENTITY_TYPE_FAMILIES,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(&mut out, reader.read_str(crate::fields::CAMERA_AIM_ASSIST_PRESET_EXCLUSION_DEFINITION_ENTITY_TYPE_FAMILIES_ITEM)?)?;
            }
            out
        };
        Ok(Self {
            blocks,
            entities,
            block_tags,
            entity_type_families,
        })
    }
}

/// CameraEase represents an easing function that can be used by a CameraInstructionSet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraEase {
    /// `type_` is the type of easing function used. This is one of the constants above.
    pub type_: wire::U8,
    /// `time` is the time in seconds that the easing function should take.
    pub time: wire::F32LE,
}

impl wire::Encode for CameraEase {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        self.time.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraEase {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = <wire::U8 as wire::Decode>::decode(reader)?;
        let time = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self { type_, time })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraEntityOffset {
    pub entity_offset_x: wire::F32LE,
    pub entity_offset_y: wire::F32LE,
    pub entity_offset_z: wire::F32LE,
}

impl wire::Encode for CameraEntityOffset {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.entity_offset_x.encode(writer)?;
        self.entity_offset_y.encode(writer)?;
        self.entity_offset_z.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraEntityOffset {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entity_offset_x = <wire::F32LE as wire::Decode>::decode(reader)?;
        let entity_offset_y = <wire::F32LE as wire::Decode>::decode(reader)?;
        let entity_offset_z = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            entity_offset_x,
            entity_offset_y,
            entity_offset_z,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraFacing {
    pub pos: glam::Vec3,
}

impl wire::Encode for CameraFacing {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraFacing {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos = <glam::Vec3 as wire::Decode>::decode(reader)?;
        Ok(Self { pos })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraFadeColor {
    pub red: wire::F32LE,
    pub green: wire::F32LE,
    pub blue: wire::F32LE,
}

impl wire::Encode for CameraFadeColor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.red.encode(writer)?;
        self.green.encode(writer)?;
        self.blue.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraFadeColor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let red = <wire::F32LE as wire::Decode>::decode(reader)?;
        let green = <wire::F32LE as wire::Decode>::decode(reader)?;
        let blue = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self { red, green, blue })
    }
}

/// CameraFadeTimeData represents the time data for a CameraInstructionFade.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraFadeTimeData {
    /// `fade_in_time` is the time in seconds for the screen to fully fade in.
    pub fade_in_time: wire::F32LE,
    /// `hold_time` is time in seconds to wait before fading out.
    pub hold_time: wire::F32LE,
    /// `fade_out_time` is the time in seconds for the screen to fully fade out.
    pub fade_out_time: wire::F32LE,
}

impl wire::Encode for CameraFadeTimeData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.fade_in_time.encode(writer)?;
        self.hold_time.encode(writer)?;
        self.fade_out_time.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraFadeTimeData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let fade_in_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let hold_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let fade_out_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            fade_in_time,
            hold_time,
            fade_out_time,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstructionData {
    /// `set` is a camera instruction that sets the camera to a specified preset.
    /// Wire presence: optional value is preceded by a presence marker.
    pub set: Option<CameraInstructionSet>,
    /// `clear` can be set to true to clear all the current camera instructions.
    /// Wire presence: optional value is preceded by a presence marker.
    pub clear: Option<bool>,
    /// `fade` is a camera instruction that fades the screen to a specified colour.
    /// Wire presence: optional value is preceded by a presence marker.
    pub fade: Option<CameraInstructionFade>,
    /// `target` is a camera instruction that targets a specific entity.
    /// Wire presence: optional value is preceded by a presence marker.
    pub target: Option<CameraInstructionTargetData>,
    /// `remove_target` can be set to true to remove the current aim assist target.
    /// Wire presence: optional value is preceded by a presence marker.
    pub remove_target: Option<bool>,
    /// `field_of_view` is a camera instruction that updates the field of view for the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub field_of_view: Option<CameraInstructionFieldOfView>,
    /// `spline` is a camera instruction that creates a spline path for the camera to follow.
    /// Wire presence: optional value is preceded by a presence marker.
    pub spline: Option<CameraSplineInstruction>,
    /// `attach_to_entity` is the entity ID to attach the camera to.
    /// Wire presence: optional value is preceded by a presence marker.
    pub attach_to_entity: Option<CameraInstructionTarget>,
    /// `detach_from_entity` can be set to true to detach the camera from the current entity.
    /// Wire presence: optional value is preceded by a presence marker.
    pub detach_from_entity: Option<bool>,
}

impl wire::Encode for CameraInstructionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.set {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.clear {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.fade {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.target {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.remove_target {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.field_of_view {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.spline {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.attach_to_entity {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.detach_from_entity {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraInstructionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let set = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraInstructionSet as wire::Decode>::decode(reader)?)
            }
        };
        let clear = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let fade = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraInstructionFade as wire::Decode>::decode(reader)?)
            }
        };
        let target = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraInstructionTargetData as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let remove_target = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let field_of_view = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraInstructionFieldOfView as wire::Decode>::decode(
                    reader,
                )?)
            }
        };
        let spline = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraSplineInstruction as wire::Decode>::decode(reader)?)
            }
        };
        let attach_to_entity = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraInstructionTarget as wire::Decode>::decode(reader)?)
            }
        };
        let detach_from_entity = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            set,
            clear,
            fade,
            target,
            remove_target,
            field_of_view,
            spline,
            attach_to_entity,
            detach_from_entity,
        })
    }
}

/// CameraInstructionFade represents a camera instruction that fades the screen to a specified colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstructionFade {
    /// `time` is the time data for the fade, which includes the fade in duration, wait duration and fade out
    /// duration.
    /// Wire presence: optional value is preceded by a presence marker.
    pub time: Option<CameraFadeTimeData>,
    /// `color` is the colour of the screen to fade to. This only uses the red, green and blue components.
    /// Wire presence: optional value is preceded by a presence marker.
    pub color: Option<CameraFadeColor>,
}

impl wire::Encode for CameraInstructionFade {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.time {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.color {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraInstructionFade {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let time = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraFadeTimeData as wire::Decode>::decode(reader)?)
            }
        };
        let color = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraFadeColor as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self { time, color })
    }
}

/// CameraInstructionFieldOfView represents a camera instruction that updates the field of view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstructionFieldOfView {
    /// `field_of_view` is the field of view of the camera.
    pub field_of_view: wire::F32LE,
    /// EaseTime is the time in seconds that the easing function should take.
    pub fov_ease_time: wire::F32LE,
    pub fov_ease_type: wire::Str,
    /// Clear can be set to true to clear the current instruction.
    pub field_of_view_clear: bool,
}

impl wire::Encode for CameraInstructionFieldOfView {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.field_of_view.encode(writer)?;
        self.fov_ease_time.encode(writer)?;
        self.fov_ease_type.encode(writer)?;
        self.field_of_view_clear.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraInstructionFieldOfView {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let field_of_view = <wire::F32LE as wire::Decode>::decode(reader)?;
        let fov_ease_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let fov_ease_type =
            reader.read_str(crate::fields::CAMERA_INSTRUCTION_FIELD_OF_VIEW_FOV_EASE_TYPE)?;
        let field_of_view_clear = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            field_of_view,
            fov_ease_time,
            fov_ease_type,
            field_of_view_clear,
        })
    }
}

/// CameraInstructionSet represents a camera instruction that sets the camera to a specified preset and can be
/// extended with easing functions and translations to the camera's position and rotation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstructionSet {
    /// `preset` is the index of the preset in the CameraPresets packet sent to the player.
    pub preset: wire::U32LE,
    /// `ease` represents the easing function that is used by the instruction.
    /// Wire presence: optional value is preceded by a presence marker.
    pub ease: Option<CameraEase>,
    /// `pos` represents the position of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub pos: Option<CameraPosition>,
    /// `rot` represents the rotation of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rot: Option<CameraRotation>,
    /// `facing` is a vector that the camera will always face towards during the duration of the instruction.
    /// Wire presence: optional value is preceded by a presence marker.
    pub facing: Option<CameraFacing>,
    /// `view_offset` is an offset based on a pivot point to the player, causing the camera to be shifted in a
    /// certain direction.
    /// Wire presence: optional value is preceded by a presence marker.
    pub view_offset: Option<CameraViewOffset>,
    /// `entity_offset` is an offset from the entity that the camera should be rendered at.
    /// Wire presence: optional value is preceded by a presence marker.
    pub entity_offset: Option<CameraEntityOffset>,
    /// `default` determines whether the camera is a default camera or not.
    /// Wire presence: optional value is preceded by a presence marker.
    pub default: Option<bool>,
    /// `remove_ignore_starting_values_component` behavior is currently unknown.
    pub remove_ignore_starting_values_component: bool,
}

impl wire::Encode for CameraInstructionSet {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.preset.encode(writer)?;
        match &self.ease {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.pos {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rot {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.facing {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.view_offset {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.entity_offset {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.default {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.remove_ignore_starting_values_component
            .encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraInstructionSet {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let preset = <wire::U32LE as wire::Decode>::decode(reader)?;
        let ease = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraEase as wire::Decode>::decode(reader)?)
            }
        };
        let pos = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraPosition as wire::Decode>::decode(reader)?)
            }
        };
        let rot = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraRotation as wire::Decode>::decode(reader)?)
            }
        };
        let facing = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraFacing as wire::Decode>::decode(reader)?)
            }
        };
        let view_offset = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraViewOffset as wire::Decode>::decode(reader)?)
            }
        };
        let entity_offset = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraEntityOffset as wire::Decode>::decode(reader)?)
            }
        };
        let default = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let remove_ignore_starting_values_component = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            preset,
            ease,
            pos,
            rot,
            facing,
            view_offset,
            entity_offset,
            default,
            remove_ignore_starting_values_component,
        })
    }
}

/// CameraInstructionTarget represents a camera instruction that targets a specific entity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CameraInstructionTarget(pub i64);

impl wire::Encode for CameraInstructionTarget {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I64LE(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraInstructionTarget {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::I64LE as wire::Decode>::decode(reader)?.0))
    }
}

/// CameraInstructionTarget represents a camera instruction that targets a specific entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstructionTargetData {
    /// `target_center_offset` is the offset from the center of the entity that the camera should target.
    /// Wire presence: optional value is preceded by a presence marker.
    pub target_center_offset: Option<glam::Vec3>,
    /// `target_actor_id` is the unique ID of the entity that the camera should target.
    pub target_actor_id: wire::I64LE,
}

impl wire::Encode for CameraInstructionTargetData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.target_center_offset {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.target_actor_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraInstructionTargetData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_center_offset = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let target_actor_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_center_offset,
            target_actor_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraPosition {
    pub pos: glam::Vec3,
}

impl wire::Encode for CameraPosition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraPosition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos = <glam::Vec3 as wire::Decode>::decode(reader)?;
        Ok(Self { pos })
    }
}

/// CameraPreset represents a basic preset that can be extended upon by more complex instructions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraPreset {
    /// `name` is the name of the preset. Each preset must have their own unique name.
    pub name: wire::Str,
    /// Parent is the name of the preset that this preset extends upon. This can be left empty.
    pub inherit_from: wire::Str,
    /// `pos_x` is the default X position of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub pos_x: Option<wire::F32LE>,
    /// `pos_y` is the default Y position of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub pos_y: Option<wire::F32LE>,
    /// `pos_z` is the default Z position of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub pos_z: Option<wire::F32LE>,
    /// `rot_x` is the default pitch of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rot_x: Option<wire::F32LE>,
    /// `rot_y` is the default yaw of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rot_y: Option<wire::F32LE>,
    /// `rotation_speed` is the speed at which the camera should rotate.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rotation_speed: Option<wire::F32LE>,
    /// `snap_to_target` determines whether the camera should snap to the target entity or not.
    /// Wire presence: optional value is preceded by a presence marker.
    pub snap_to_target: Option<bool>,
    /// `horizontal_rotation_limit` is the horizontal rotation limit of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub horizontal_rotation_limit: Option<glam::Vec2>,
    /// `vertical_rotation_limit` is the vertical rotation limit of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub vertical_rotation_limit: Option<glam::Vec2>,
    /// `continue_targeting` determines whether the camera should continue targeting when using aim assist.
    /// Wire presence: optional value is preceded by a presence marker.
    pub continue_targeting: Option<bool>,
    /// TrackingRadius is the radius around the camera that the aim assist should track targets.
    /// Wire presence: optional value is preceded by a presence marker.
    pub block_listening_radius: Option<wire::F32LE>,
    /// `view_offset` is only used in a follow_orbit camera and controls an offset based on a pivot point to the
    /// player, causing it to be shifted in a certain direction.
    /// Wire presence: optional value is preceded by a presence marker.
    pub view_offset: Option<glam::Vec2>,
    /// `entity_offset` controls the offset from the entity that the camera should be rendered at.
    /// Wire presence: optional value is preceded by a presence marker.
    pub entity_offset: Option<glam::Vec3>,
    /// `radius` is only used in a follow_orbit camera and controls how far away from the player the camera should
    /// be rendered.
    /// Wire presence: optional value is preceded by a presence marker.
    pub radius: Option<wire::F32LE>,
    /// MinYawLimit is the minimum yaw limit of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub yaw_limit_min: Option<wire::F32LE>,
    /// MaxYawLimit is the maximum yaw limit of the camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub yaw_limit_max: Option<wire::F32LE>,
    /// AudioListener defines where the audio should be played from when using this preset. This is one of the
    /// constants above.
    /// Wire presence: optional value is preceded by a presence marker.
    pub listener: Option<CameraPresetAudioListener>,
    /// `player_effects` is currently unknown.
    /// Wire presence: optional value is preceded by a presence marker.
    pub player_effects: Option<bool>,
    /// `aim_assist` defines the aim assist to use when using this preset.
    /// Wire presence: optional value is preceded by a presence marker.
    pub aim_assist: Option<CameraAimAssistCommandPresetDefinition>,
    /// `control_scheme` is the control scheme that the client should use in this camera. It is one of the
    /// following: - ControlSchemeLockedPlayerRelativeStrafe is the default behaviour, this cannot be set when the
    /// client is in a custom camera. - ControlSchemeCameraRelative makes movement relative to the camera's
    /// transform, with the client's rotation being relative to the client's movement. -
    /// ControlSchemeCameraRelativeStrafe makes movement relative to the camera's transform, with the client's
    /// rotation being locked. - ControlSchemePlayerRelative makes movement relative to the player's transform,
    /// meaning holding left/right will make the player turn in a circle. - ControlSchemePlayerRelativeStrafe
    /// makes movement the same as the default behaviour, but can be used in a custom camera.
    /// Wire presence: optional value is preceded by a presence marker.
    pub control_scheme: Option<ControlScheme>,
    /// `apply_inherited_starting_rotation` specifies if the camera should start at the rotation of the preset it
    /// inherits from, rather than at StartingRotation.
    pub apply_inherited_starting_rotation: bool,
    /// `starting_rotation` is the rotation that the camera starts at when the preset is applied.
    /// Wire presence: optional value is preceded by a presence marker.
    pub starting_rotation: Option<glam::Vec2>,
}

impl wire::Encode for CameraPreset {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.inherit_from.encode(writer)?;
        match &self.pos_x {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.pos_y {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.pos_z {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rot_x {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rot_y {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rotation_speed {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.snap_to_target {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.horizontal_rotation_limit {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.vertical_rotation_limit {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.continue_targeting {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.block_listening_radius {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.view_offset {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.entity_offset {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.radius {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.yaw_limit_min {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.yaw_limit_max {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.listener {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.player_effects {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.aim_assist {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.control_scheme {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.apply_inherited_starting_rotation.encode(writer)?;
        match &self.starting_rotation {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraPreset {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::CAMERA_PRESET_NAME)?;
        let inherit_from = reader.read_str(crate::fields::CAMERA_PRESET_INHERIT_FROM)?;
        let pos_x = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let pos_y = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let pos_z = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let rot_x = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let rot_y = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let rotation_speed = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let snap_to_target = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let horizontal_rotation_limit = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let vertical_rotation_limit = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let continue_targeting = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let block_listening_radius = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let view_offset = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let entity_offset = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let radius = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let yaw_limit_min = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let yaw_limit_max = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let listener = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraPresetAudioListener as wire::Decode>::decode(reader)?)
            }
        };
        let player_effects = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let aim_assist = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<CameraAimAssistCommandPresetDefinition as wire::Decode>::decode(reader)?)
            }
        };
        let control_scheme = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ControlScheme as wire::Decode>::decode(reader)?)
            }
        };
        let apply_inherited_starting_rotation = <bool as wire::Decode>::decode(reader)?;
        let starting_rotation = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            name,
            inherit_from,
            pos_x,
            pos_y,
            pos_z,
            rot_x,
            rot_y,
            rotation_speed,
            snap_to_target,
            horizontal_rotation_limit,
            vertical_rotation_limit,
            continue_targeting,
            block_listening_radius,
            view_offset,
            entity_offset,
            radius,
            yaw_limit_min,
            yaw_limit_max,
            listener,
            player_effects,
            aim_assist,
            control_scheme,
            apply_inherited_starting_rotation,
            starting_rotation,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraPresetList {
    /// `presets` is a list of camera presets that can be used by other cameras. The order of this list is
    /// important because the index of presets is used as a pointer in the CameraInstruction packet.
    pub presets: Vec<CameraPreset>,
}

impl wire::Encode for CameraPresetList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.presets.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CameraPresetList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let presets = wire::decode_collection::<CameraPreset>(
            reader,
            crate::fields::CAMERA_PRESET_LIST_PRESETS,
            24,
        )?;
        Ok(Self { presets })
    }
}

/// CameraProgressOption represents a progress keyframe option for camera spline instructions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraProgressOption {
    /// Value is the progress value.
    pub key_frame_value: wire::F32LE,
    /// Time is the time for this progress option.
    pub key_frame_time: wire::F32LE,
    pub key_frame_easing_func: wire::Str,
}

impl wire::Encode for CameraProgressOption {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.key_frame_value.encode(writer)?;
        self.key_frame_time.encode(writer)?;
        self.key_frame_easing_func.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraProgressOption {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let key_frame_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let key_frame_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let key_frame_easing_func =
            reader.read_str(crate::fields::CAMERA_PROGRESS_OPTION_KEY_FRAME_EASING_FUNC)?;
        Ok(Self {
            key_frame_value,
            key_frame_time,
            key_frame_easing_func,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRotation {
    pub x: wire::F32LE,
    pub y: wire::F32LE,
}

impl wire::Encode for CameraRotation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.x.encode(writer)?;
        self.y.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraRotation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let x = <wire::F32LE as wire::Decode>::decode(reader)?;
        let y = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self { x, y })
    }
}

/// CameraRotationOption represents a rotation option for camera spline instructions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRotationOption {
    /// Value is the rotation value.
    pub key_frame_value: glam::Vec3,
    /// Time is the time for this rotation option.
    pub key_frame_time: wire::F32LE,
    pub key_frame_easing_func: wire::Str,
}

impl wire::Encode for CameraRotationOption {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.key_frame_value.encode(writer)?;
        self.key_frame_time.encode(writer)?;
        self.key_frame_easing_func.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraRotationOption {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let key_frame_value = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let key_frame_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let key_frame_easing_func =
            reader.read_str(crate::fields::CAMERA_ROTATION_OPTION_KEY_FRAME_EASING_FUNC)?;
        Ok(Self {
            key_frame_value,
            key_frame_time,
            key_frame_easing_func,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSplineControlPoint {
    pub position: glam::Vec3,
}

impl wire::Encode for CameraSplineControlPoint {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.position.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraSplineControlPoint {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        Ok(Self { position })
    }
}

/// CameraSplineDefinition represents a named camera spline definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSplineDefinition {
    /// `name` is the name of the spline definition.
    pub name: wire::Str,
    /// `total_time` is the total time for the spline animation.
    pub total_time: wire::F32LE,
    /// `spline_type` is the optional spline interpolation type.
    pub spline_type: wire::Str,
    /// `control_points` is a list of points that define the spline curve.
    pub control_points: Vec<CameraSplineControlPoint>,
    /// `progress_key_frames` is a list of progress key frames for the spline.
    pub progress_key_frames: Vec<CameraSplineProgressKeyFrame>,
    /// `rotation_key_frames` is a list of rotation key frames for the spline.
    pub rotation_key_frames: Vec<CameraSplineRotationKeyFrame>,
}

impl wire::Encode for CameraSplineDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        wire::check_pattern(&self.name, "^\\w+:\\w+$")?;
        self.total_time.encode(writer)?;
        wire::check_number_limits(self.total_time.0, Some(0.0), None)?;
        self.spline_type.encode(writer)?;
        wire::check_pattern(&self.spline_type, "^(?:catmullrom|linear)$")?;
        wire::encode_collection(writer, self.control_points.as_slice())?;
        wire::encode_collection(writer, self.progress_key_frames.as_slice())?;
        wire::encode_collection(writer, self.rotation_key_frames.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CameraSplineDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = {
            let value = reader.read_str(crate::fields::CAMERA_SPLINE_DEFINITION_NAME)?;
            wire::validate_pattern(&value, "^\\w+:\\w+$")?;
            value
        };
        let total_time = {
            let value = <wire::F32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0.0), None)?;
            value
        };
        let spline_type = {
            let value = reader.read_str(crate::fields::CAMERA_SPLINE_DEFINITION_SPLINE_TYPE)?;
            wire::validate_pattern(&value, "^(?:catmullrom|linear)$")?;
            value
        };
        let control_points = wire::decode_collection::<CameraSplineControlPoint>(
            reader,
            crate::fields::CAMERA_SPLINE_DEFINITION_CONTROL_POINTS,
            12,
        )?;
        let progress_key_frames = wire::decode_collection::<CameraSplineProgressKeyFrame>(
            reader,
            crate::fields::CAMERA_SPLINE_DEFINITION_PROGRESS_KEY_FRAMES,
            9,
        )?;
        let rotation_key_frames = wire::decode_collection::<CameraSplineRotationKeyFrame>(
            reader,
            crate::fields::CAMERA_SPLINE_DEFINITION_ROTATION_KEY_FRAMES,
            17,
        )?;
        Ok(Self {
            name,
            total_time,
            spline_type,
            control_points,
            progress_key_frames,
            rotation_key_frames,
        })
    }
}

/// CameraSplineInstruction represents a camera instruction that creates a spline path for the camera to
/// follow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSplineInstruction {
    /// `total_time` is the total time for the spline animation.
    pub total_time: wire::F32LE,
    pub type_: wire::U8,
    /// `curve` is a list of points that define the spline curve.
    pub curve: Vec<glam::Vec3>,
    /// `progress_key_frames` is a list of progress key frames for the spline.
    pub progress_key_frames: Vec<CameraProgressOption>,
    /// RotationOptions is a list of rotation options for the spline.
    pub rotation_option: Vec<CameraRotationOption>,
    /// `spline_identifier` is an optional identifier for referencing the spline by name.
    pub spline_identifier: wire::Str,
    /// `load_from_json` optionally determines whether the spline should be loaded from a JSON definition.
    pub load_from_json: bool,
}

impl wire::Encode for CameraSplineInstruction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.total_time.encode(writer)?;
        self.type_.encode(writer)?;
        wire::encode_collection(writer, self.curve.as_slice())?;
        wire::encode_collection(writer, self.progress_key_frames.as_slice())?;
        wire::encode_collection(writer, self.rotation_option.as_slice())?;
        wire::encode_string_limits(writer, &self.spline_identifier, 0, 1024)?;
        self.load_from_json.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraSplineInstruction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let total_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let type_ = <wire::U8 as wire::Decode>::decode(reader)?;
        let curve = wire::decode_collection::<glam::Vec3>(
            reader,
            crate::fields::CAMERA_SPLINE_INSTRUCTION_CURVE,
            12,
        )?;
        let progress_key_frames = wire::decode_collection::<CameraProgressOption>(
            reader,
            crate::fields::CAMERA_SPLINE_INSTRUCTION_PROGRESS_KEY_FRAMES,
            9,
        )?;
        let rotation_option = wire::decode_collection::<CameraRotationOption>(
            reader,
            crate::fields::CAMERA_SPLINE_INSTRUCTION_ROTATION_OPTION,
            17,
        )?;
        let spline_identifier = wire::decode_string_limits(
            reader,
            crate::fields::CAMERA_SPLINE_INSTRUCTION_SPLINE_IDENTIFIER,
            0,
            1024,
        )?;
        let load_from_json = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            total_time,
            type_,
            curve,
            progress_key_frames,
            rotation_option,
            spline_identifier,
            load_from_json,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSplineProgressKeyFrame {
    pub progress: wire::F32LE,
    pub time: wire::F32LE,
    /// Wire presence: optional value is preceded by a presence marker.
    pub easing: Option<wire::Str>,
}

impl wire::Encode for CameraSplineProgressKeyFrame {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.progress.encode(writer)?;
        wire::check_number_limits(self.progress.0, Some(0.0), Some(1.0))?;
        self.time.encode(writer)?;
        wire::check_number_limits(self.time.0, Some(0.0), None)?;
        match &self.easing {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraSplineProgressKeyFrame {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let progress = {
            let value = <wire::F32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0.0), Some(1.0))?;
            value
        };
        let time = {
            let value = <wire::F32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0.0), None)?;
            value
        };
        let easing = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::CAMERA_SPLINE_PROGRESS_KEY_FRAME_EASING)?)
            }
        };
        Ok(Self {
            progress,
            time,
            easing,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSplineRotationKeyFrame {
    pub rotation: glam::Vec3,
    pub time: wire::F32LE,
    /// Wire presence: optional value is preceded by a presence marker.
    pub easing: Option<wire::Str>,
}

impl wire::Encode for CameraSplineRotationKeyFrame {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.rotation.encode(writer)?;
        self.time.encode(writer)?;
        wire::check_number_limits(self.time.0, Some(0.0), None)?;
        match &self.easing {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CameraSplineRotationKeyFrame {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rotation = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let time = {
            let value = <wire::F32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0.0), None)?;
            value
        };
        let easing = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::CAMERA_SPLINE_ROTATION_KEY_FRAME_EASING)?)
            }
        };
        Ok(Self {
            rotation,
            time,
            easing,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraViewOffset {
    pub x: wire::F32LE,
    pub y: wire::F32LE,
}

impl wire::Encode for CameraViewOffset {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.x.encode(writer)?;
        self.y.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraViewOffset {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let x = <wire::F32LE as wire::Decode>::decode(reader)?;
        let y = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self { x, y })
    }
}

// Domain: chunk_pos

/// ChunkPos is the position of a chunk. It is composed of two integers and is written as two varint32s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkPos {
    pub x: wire::ZigZag32,
    pub z: wire::ZigZag32,
}

impl wire::Encode for ChunkPos {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.x.encode(writer)?;
        self.z.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChunkPos {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let x = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let z = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { x, z })
    }
}

/// SubChunkPos is the position of a sub-chunk. The X and Z coordinates are the coordinates of the chunk, and
/// the Y coordinate is the absolute sub-chunk index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubChunkPos {
    pub subchunk_position_x: wire::I32LE,
    pub subchunk_position_y: wire::I32LE,
    pub subchunk_position_z: wire::I32LE,
}

impl wire::Encode for SubChunkPos {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.subchunk_position_x.encode(writer)?;
        self.subchunk_position_y.encode(writer)?;
        self.subchunk_position_z.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkPos {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let subchunk_position_x = <wire::I32LE as wire::Decode>::decode(reader)?;
        let subchunk_position_y = <wire::I32LE as wire::Decode>::decode(reader)?;
        let subchunk_position_z = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            subchunk_position_x,
            subchunk_position_y,
            subchunk_position_z,
        })
    }
}

// Domain: clock

/// SyncWorldClockStateData represents the state data for synchronising a world clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncWorldClockStateData {
    /// `clock_id` is the unique identifier for the clock.
    pub clock_id: wire::VarULong,
    /// `time` is the current time of the clock.
    pub time: wire::ZigZag32,
    /// `is_paused` indicates if the clock is paused.
    pub is_paused: bool,
}

impl wire::Encode for SyncWorldClockStateData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.clock_id.encode(writer)?;
        self.time.encode(writer)?;
        self.is_paused.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SyncWorldClockStateData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let clock_id = <wire::VarULong as wire::Decode>::decode(reader)?;
        let time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let is_paused = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            clock_id,
            time,
            is_paused,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SyncWorldClocksData {
    SyncStateData {
        clock_data: Vec<SyncWorldClockStateData>,
    },
    InitializeRegistryData {
        clock_data: Vec<WorldClockData>,
    },
    AddTimeMarkerData {
        clock_id: wire::VarULong,
        time_markers: Vec<TimeMarkerData>,
    },
    RemoveTimeMarkerData {
        clock_id: wire::VarULong,
        time_marker_ids: Vec<wire::VarULong>,
    },
}

impl SyncWorldClocksData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::SyncStateData { .. } => 0,
            Self::InitializeRegistryData { .. } => 1,
            Self::AddTimeMarkerData { .. } => 2,
            Self::RemoveTimeMarkerData { .. } => 3,
        }
    }
}

impl Default for SyncWorldClocksData {
    fn default() -> Self {
        Self::SyncStateData {
            clock_data: Default::default(),
        }
    }
}

impl wire::Encode for SyncWorldClocksData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::SyncStateData { clock_data } => {
                wire::encode_collection_limits(writer, clock_data.as_slice(), 0, 256)?;
            }
            Self::InitializeRegistryData { clock_data } => {
                wire::encode_collection_limits(writer, clock_data.as_slice(), 0, 256)?;
            }
            Self::AddTimeMarkerData {
                clock_id,
                time_markers,
            } => {
                clock_id.encode(writer)?;
                wire::encode_collection_limits(writer, time_markers.as_slice(), 0, 256)?;
            }
            Self::RemoveTimeMarkerData {
                clock_id,
                time_marker_ids,
            } => {
                clock_id.encode(writer)?;
                wire::encode_collection_limits(writer, time_marker_ids.as_slice(), 0, 256)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for SyncWorldClocksData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let clock_data = wire::decode_collection_limits::<SyncWorldClockStateData>(
                    reader,
                    crate::fields::SYNC_WORLD_CLOCKS_DATA_SYNC_STATE_DATA_CLOCK_DATA,
                    3,
                    0,
                    256,
                )?;
                Self::SyncStateData { clock_data }
            }
            1 => {
                let clock_data = wire::decode_collection_limits::<WorldClockData>(
                    reader,
                    crate::fields::SYNC_WORLD_CLOCKS_DATA_INITIALIZE_REGISTRY_DATA_CLOCK_DATA,
                    5,
                    0,
                    256,
                )?;
                Self::InitializeRegistryData { clock_data }
            }
            2 => {
                let clock_id = <wire::VarULong as wire::Decode>::decode(reader)?;
                let time_markers = wire::decode_collection_limits::<TimeMarkerData>(
                    reader,
                    crate::fields::SYNC_WORLD_CLOCKS_DATA_ADD_TIME_MARKER_DATA_TIME_MARKERS,
                    4,
                    0,
                    256,
                )?;
                Self::AddTimeMarkerData {
                    clock_id,
                    time_markers,
                }
            }
            3 => {
                let clock_id = <wire::VarULong as wire::Decode>::decode(reader)?;
                let time_marker_ids = wire::decode_collection_limits::<wire::VarULong>(
                    reader,
                    crate::fields::SYNC_WORLD_CLOCKS_DATA_REMOVE_TIME_MARKER_DATA_TIME_MARKER_IDS,
                    1,
                    0,
                    256,
                )?;
                Self::RemoveTimeMarkerData {
                    clock_id,
                    time_marker_ids,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "SyncWorldClocksData",
                    value: value as i64,
                });
            }
        })
    }
}

/// TimeMarkerData represents a time marker within a world clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeMarkerData {
    /// `id` is the unique identifier for the time marker.
    pub id: wire::VarULong,
    /// `name` is the name of the time marker.
    pub name: wire::Str,
    /// `time` is the time at which the marker is set.
    pub time: wire::ZigZag32,
    /// `period` is the optional period for the time marker.
    /// Wire presence: optional value is preceded by a presence marker.
    pub period: Option<wire::I32LE>,
}

impl wire::Encode for TimeMarkerData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        wire::encode_string_limits(writer, &self.name, 0, 128)?;
        self.time.encode(writer)?;
        match &self.period {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for TimeMarkerData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = <wire::VarULong as wire::Decode>::decode(reader)?;
        let name =
            wire::decode_string_limits(reader, crate::fields::TIME_MARKER_DATA_NAME, 0, 128)?;
        let time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let period = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            id,
            name,
            time,
            period,
        })
    }
}

/// WorldClockData represents a complete world clock with its time markers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldClockData {
    /// `id` is the unique identifier for the clock.
    pub id: wire::VarULong,
    /// `name` is the name of the clock.
    pub name: wire::Str,
    /// `time` is the current time of the clock.
    pub time: wire::ZigZag32,
    /// `is_paused` indicates if the clock is paused.
    pub is_paused: bool,
    /// `time_markers` is a list of time markers for this clock.
    pub time_markers: Vec<TimeMarkerData>,
}

impl wire::Encode for WorldClockData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        wire::encode_string_limits(writer, &self.name, 0, 128)?;
        self.time.encode(writer)?;
        self.is_paused.encode(writer)?;
        wire::encode_collection_limits(writer, self.time_markers.as_slice(), 0, 256)?;
        Ok(())
    }
}

impl wire::Decode for WorldClockData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = <wire::VarULong as wire::Decode>::decode(reader)?;
        let name =
            wire::decode_string_limits(reader, crate::fields::WORLD_CLOCK_DATA_NAME, 0, 128)?;
        let time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let is_paused = <bool as wire::Decode>::decode(reader)?;
        let time_markers = wire::decode_collection_limits::<TimeMarkerData>(
            reader,
            crate::fields::WORLD_CLOCK_DATA_TIME_MARKERS,
            4,
            0,
            256,
        )?;
        Ok(Self {
            id,
            name,
            time,
            is_paused,
            time_markers,
        })
    }
}

// Domain: command

/// ChainedSubcommand represents a subcommand that can have chained commands, such as /execute which allows
/// you to run another command as another entity or at a different position etc.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainedSubcommand {
    /// `name` is the name of the chained subcommand and shows up in the list as a regular subcommand enum.
    pub name: wire::Str,
    /// `sub_command_values` contains the index and parameter type of the chained subcommand.
    pub sub_command_values: Vec<ChainedSubcommandValue>,
}

impl wire::Encode for ChainedSubcommand {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.name, 0, 512)?;
        wire::encode_collection_limits(writer, self.sub_command_values.as_slice(), 0, 32)?;
        Ok(())
    }
}

impl wire::Decode for ChainedSubcommand {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name =
            wire::decode_string_limits(reader, crate::fields::CHAINED_SUBCOMMAND_NAME, 0, 512)?;
        let sub_command_values = wire::decode_collection_limits::<ChainedSubcommandValue>(
            reader,
            crate::fields::CHAINED_SUBCOMMAND_SUB_COMMAND_VALUES,
            2,
            0,
            32,
        )?;
        Ok(Self {
            name,
            sub_command_values,
        })
    }
}

/// ChainedSubcommandValue represents the value for a chained subcommand argument.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainedSubcommandValue {
    /// `sub_command_first_value` is the index of the argument in the ChainedSubcommandValues slice from the
    /// AvailableCommands packet. This is then used to set the type specified by the Value field below.
    pub sub_command_first_value: wire::VarUInt,
    /// `sub_command_second_value` is a combination of the flags above and specified the type of argument. Unlike
    /// regular parameter types, this should NOT contain any of the special flags (valid, enum, suffixed or soft
    /// enum) but only the basic types.
    pub sub_command_second_value: wire::VarUInt,
}

impl wire::Encode for ChainedSubcommandValue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sub_command_first_value.encode(writer)?;
        self.sub_command_second_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChainedSubcommandValue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sub_command_first_value = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let sub_command_second_value = <wire::VarUInt as wire::Decode>::decode(reader)?;
        Ok(Self {
            sub_command_first_value,
            sub_command_second_value,
        })
    }
}

/// Command holds the data that a command requires to be shown to a player client-side. The command is shown
/// in the /help command and auto-completed using this data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Command {
    /// `name` is the name of the command. The command may be executed using this name, and will be shown in the
    /// /help list with it. It currently seems that the client crashes if the Name contains uppercase letters.
    pub name: wire::Str,
    /// `description` is the description of the command. It is shown in the /help list and when starting to write
    /// a command.
    pub description: wire::Str,
    /// `flags` is a combination of flags not currently known. Leaving the Flags field empty appears to work.
    pub flags: wire::U16LE,
    /// `permission_level` is the command permission level that the player required to execute this command. The
    /// field no longer seems to serve a purpose, as the client does not handle the execution of commands anymore:
    /// The permissions should be checked server-side.
    pub permission_level: wire::Str,
    /// AliasesOffset is the offset to a CommandEnum that holds the values that should be used as aliases for this
    /// command.
    pub alias_enum: wire::I32LE,
    /// ChainedSubcommandOffsets is a slice of offsets that all point to a different ChainedSubcommand from the
    /// ChainedSubcommands slice in the AvailableCommands packet.
    pub command_data_chained_subcommand_indexes: Vec<wire::U32LE>,
    /// `overloads` is a list of command overloads that specify the ways in which a command may be executed. The
    /// overloads may be completely different.
    pub overloads: Vec<CommandOverload>,
}

impl wire::Encode for Command {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.name, 0, 1000)?;
        wire::encode_string_limits(writer, &self.description, 0, 1000)?;
        self.flags.encode(writer)?;
        wire::check_number_limits(self.flags.0, Some(0), None)?;
        self.permission_level.encode(writer)?;
        self.alias_enum.encode(writer)?;
        wire::encode_collection_limits(
            writer,
            self.command_data_chained_subcommand_indexes.as_slice(),
            0,
            250,
        )?;
        wire::encode_collection_limits(writer, self.overloads.as_slice(), 0, 250)?;
        Ok(())
    }
}

impl wire::Decode for Command {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = wire::decode_string_limits(reader, crate::fields::COMMAND_NAME, 0, 1000)?;
        let description =
            wire::decode_string_limits(reader, crate::fields::COMMAND_DESCRIPTION, 0, 1000)?;
        let flags = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let permission_level = reader.read_str(crate::fields::COMMAND_PERMISSION_LEVEL)?;
        let alias_enum = <wire::I32LE as wire::Decode>::decode(reader)?;
        let command_data_chained_subcommand_indexes = wire::decode_collection_limits::<wire::U32LE>(
            reader,
            crate::fields::COMMAND_COMMAND_DATA_CHAINED_SUBCOMMAND_INDEXES,
            4,
            0,
            250,
        )?;
        let overloads = wire::decode_collection_limits::<CommandOverload>(
            reader,
            crate::fields::COMMAND_OVERLOADS,
            2,
            0,
            250,
        )?;
        Ok(Self {
            name,
            description,
            flags,
            permission_level,
            alias_enum,
            command_data_chained_subcommand_indexes,
            overloads,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandBlockUpdateData {
    EntityCommandTarget {
        target_runtime_id: ActorRuntimeID,
    },
    BlockCommandData {
        block_position: BlockPos,
        command_block_mode: wire::VarUInt,
        redstone_mode: bool,
        is_conditional: bool,
    },
}

impl CommandBlockUpdateData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::EntityCommandTarget { .. } => 0,
            Self::BlockCommandData { .. } => 1,
        }
    }
}

impl Default for CommandBlockUpdateData {
    fn default() -> Self {
        Self::EntityCommandTarget {
            target_runtime_id: Default::default(),
        }
    }
}

impl wire::Encode for CommandBlockUpdateData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::EntityCommandTarget { target_runtime_id } => {
                target_runtime_id.encode(writer)?;
            }
            Self::BlockCommandData {
                block_position,
                command_block_mode,
                redstone_mode,
                is_conditional,
            } => {
                block_position.encode(writer)?;
                command_block_mode.encode(writer)?;
                redstone_mode.encode(writer)?;
                is_conditional.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for CommandBlockUpdateData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
                Self::EntityCommandTarget { target_runtime_id }
            }
            1 => {
                let block_position = <BlockPos as wire::Decode>::decode(reader)?;
                let command_block_mode = <wire::VarUInt as wire::Decode>::decode(reader)?;
                let redstone_mode = <bool as wire::Decode>::decode(reader)?;
                let is_conditional = <bool as wire::Decode>::decode(reader)?;
                Self::BlockCommandData {
                    block_position,
                    command_block_mode,
                    redstone_mode,
                    is_conditional,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "CommandBlockUpdateData",
                    value: value as i64,
                });
            }
        })
    }
}

/// CommandEnum represents an enum in a command usage. The enum typically has a type and a set of options that
/// are valid. A value that is not one of the options results in a failure during execution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandEnum {
    /// `name` is the type of the command enum. The type will show up in the command usage as the type of the
    /// argument if it has a certain amount of arguments, or when Options is set to true in the command holding
    /// the enum.
    pub name: wire::Str,
    /// `values` holds a list of indices that point to the EnumValues slice in the AvailableCommandsPacket. These
    /// represent the options of the enum.
    pub values: Vec<wire::U32LE>,
}

impl wire::Encode for CommandEnum {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        wire::encode_collection(writer, self.values.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CommandEnum {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::COMMAND_ENUM_NAME)?;
        let values =
            wire::decode_collection::<wire::U32LE>(reader, crate::fields::COMMAND_ENUM_VALUES, 4)?;
        Ok(Self { name, values })
    }
}

/// CommandEnumConstraint is sent in the AvailableCommands packet to limit what values of an enum may be used
/// taking in account things such as whether cheats are enabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandEnumConstraint {
    /// `enum_value_symbol` points to an enum value in the AvailableCommands packet that this constraint should
    /// apply to.
    pub enum_value_symbol: wire::U32LE,
    /// `enum_symbol` points to an enum in the AvailableCommands packet to which this constraint should apply to.
    pub enum_symbol: wire::U32LE,
    /// `constraint_indices` holds a slice of constraints as present in the constants above.
    pub constraint_indices: bytes::Bytes,
}

impl wire::Encode for CommandEnumConstraint {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.enum_value_symbol.encode(writer)?;
        self.enum_symbol.encode(writer)?;
        wire::encode_bytes_limits(writer, self.constraint_indices.as_ref(), 0, 250)?;
        Ok(())
    }
}

impl wire::Decode for CommandEnumConstraint {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let enum_value_symbol = <wire::U32LE as wire::Decode>::decode(reader)?;
        let enum_symbol = <wire::U32LE as wire::Decode>::decode(reader)?;
        let constraint_indices = wire::decode_bytes_limits(
            reader,
            crate::fields::COMMAND_ENUM_CONSTRAINT_CONSTRAINT_INDICES,
            0,
            250,
        )?;
        Ok(Self {
            enum_value_symbol,
            enum_symbol,
            constraint_indices,
        })
    }
}

/// CommandOrigin holds data that identifies the origin of the requesting of a command. It holds several
/// fields that may be used to get specific information. When sent in a CommandRequest packet, the same
/// CommandOrigin should be sent in a CommandOutput packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOriginData {
    pub type_: wire::Str,
    /// `uuid` is a unique identifier for every instantiation of a command.
    pub uuid: uuid::Uuid,
    /// `request_id` is an ID that identifies the request of the client. The server should send a CommandOrigin
    /// with the same request ID to ensure it can be matched with the request by the caller of the command. This
    /// is especially important for websocket servers and it seems that this field is only non-empty for these
    /// websocket servers.
    pub request_id: wire::Str,
    pub player_id: wire::I64LE,
}

impl wire::Encode for CommandOriginData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        self.uuid.encode(writer)?;
        wire::encode_string_limits(writer, &self.request_id, 0, 39)?;
        self.player_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandOriginData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = reader.read_str(crate::fields::COMMAND_ORIGIN_DATA_TYPE)?;
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let request_id = wire::decode_string_limits(
            reader,
            crate::fields::COMMAND_ORIGIN_DATA_REQUEST_ID,
            0,
            39,
        )?;
        let player_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            type_,
            uuid,
            request_id,
            player_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOutputData {
    pub output_type: wire::Str,
    pub success_count: wire::U32LE,
    pub output_messages: Vec<CommandOutputMessage>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub data_set: Option<wire::Str>,
}

impl wire::Encode for CommandOutputData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.output_type.encode(writer)?;
        self.success_count.encode(writer)?;
        wire::check_number_limits(self.success_count.0, Some(0), None)?;
        wire::encode_collection(writer, self.output_messages.as_slice())?;
        match &self.data_set {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for CommandOutputData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let output_type = reader.read_str(crate::fields::COMMAND_OUTPUT_DATA_OUTPUT_TYPE)?;
        let success_count = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let output_messages = wire::decode_collection::<CommandOutputMessage>(
            reader,
            crate::fields::COMMAND_OUTPUT_DATA_OUTPUT_MESSAGES,
            3,
        )?;
        let data_set = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::COMMAND_OUTPUT_DATA_DATA_SET)?)
            }
        };
        Ok(Self {
            output_type,
            success_count,
            output_messages,
            data_set,
        })
    }
}

/// CommandOutputMessage represents a message sent by a command that holds the output of one of the commands
/// executed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOutputMessage {
    /// Message is the message that is sent to the client in the chat window. It may either be simply a message or
    /// a translated built-in string like 'commands.tp.success.coordinates', combined with specific parameters
    /// below.
    pub message_id: wire::Str,
    /// Success indicates if the output message was one of a successful command execution. If set to true, the
    /// output message is by default coloured white, whereas if set to false, the message is by default coloured
    /// red.
    pub successful: bool,
    /// `parameters` is a list of parameters that serve to supply the message sent with additional information,
    /// such as the position that a player was teleported to or the effect that was applied to an entity. These
    /// parameters only apply for the Minecraft built-in command output.
    pub parameters: Vec<wire::Str>,
}

impl wire::Encode for CommandOutputMessage {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.message_id.encode(writer)?;
        self.successful.encode(writer)?;
        writer.write_len_var_u32(self.parameters.len())?;
        for item in self.parameters.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for CommandOutputMessage {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let message_id = reader.read_str(crate::fields::COMMAND_OUTPUT_MESSAGE_MESSAGE_ID)?;
        let successful = <bool as wire::Decode>::decode(reader)?;
        let parameters = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::COMMAND_OUTPUT_MESSAGE_PARAMETERS,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::COMMAND_OUTPUT_MESSAGE_PARAMETERS_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self {
            message_id,
            successful,
            parameters,
        })
    }
}

/// CommandOverload represents an overload of a command. This overload can be compared to function overloading
/// in languages such as java. It represents a single usage of the command. A command may have multiple
/// different overloads, which are handled differently.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOverload {
    /// `is_chaining` determines if the parameters use chained subcommands or not.
    pub is_chaining: bool,
    /// `parameter_data` is a list of command parameters that are part of the overload. These parameters specify
    /// the usage of the command when this overload is applied.
    pub parameter_data: Vec<CommandParameter>,
}

impl wire::Encode for CommandOverload {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.is_chaining.encode(writer)?;
        wire::encode_collection(writer, self.parameter_data.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CommandOverload {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let is_chaining = <bool as wire::Decode>::decode(reader)?;
        let parameter_data = wire::decode_collection::<CommandParameter>(
            reader,
            crate::fields::COMMAND_OVERLOAD_PARAMETER_DATA,
            7,
        )?;
        Ok(Self {
            is_chaining,
            parameter_data,
        })
    }
}

/// CommandParameter represents a single parameter of a command overload, which accepts a certain type of
/// input values. It has a name and a type which show up client-side when a player is entering the command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandParameter {
    /// `name` is the name of the command parameter. It shows up in the usage like <$Name: $Type>, with the
    /// exception of enum types, which show up simply as a list of options if the list is short enough and Options
    /// is set to false.
    pub name: wire::Str,
    /// Type is a rather odd combination of type(flag)s that result in a certain parameter type to show up
    /// client-side. It is a combination of the flags above. The basic types must be combined with the
    /// ArgumentTypeFlagBasic flag (and integers with a suffix ArgumentTypeFlagSuffixed), whereas enums are
    /// combined with the ArgumentTypeFlagEnum flag.
    pub parse_symbol: wire::U32LE,
    /// Optional specifies if the command parameter is optional to enter. Note that no non-optional parameter
    /// should ever be present in a command overload after an optional parameter. When optional, the parameter
    /// shows up like so: [$Name: $Type], whereas when mandatory, it shows up like so: <$Name: $Type>.
    pub is_optional: bool,
    /// `options` holds a combinations of options that additionally apply to the command parameter. The list of
    /// options can be found above.
    pub options: wire::U8,
}

impl wire::Encode for CommandParameter {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.parse_symbol.encode(writer)?;
        self.is_optional.encode(writer)?;
        self.options.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandParameter {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::COMMAND_PARAMETER_NAME)?;
        let parse_symbol = <wire::U32LE as wire::Decode>::decode(reader)?;
        let is_optional = <bool as wire::Decode>::decode(reader)?;
        let options = <wire::U8 as wire::Decode>::decode(reader)?;
        Ok(Self {
            name,
            parse_symbol,
            is_optional,
            options,
        })
    }
}

/// DynamicEnum is an enum variant that can have its options changed during runtime, without sending a new
/// AvailableCommands packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicEnum {
    /// `enum_name` is the type of the command enum. The type will show up in the command usage as the type of the
    /// argument if it has a certain amount of arguments, or when Options is set to true in the command holding
    /// the enum.
    pub enum_name: wire::Str,
    /// `enum_options` is a slice of possible options for the enum.
    pub enum_options: Vec<wire::Str>,
}

impl wire::Encode for DynamicEnum {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.enum_name, 0, 512)?;
        writer.write_len_var_u32(self.enum_options.len())?;
        for item in self.enum_options.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for DynamicEnum {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let enum_name =
            wire::decode_string_limits(reader, crate::fields::DYNAMIC_ENUM_ENUM_NAME, 0, 512)?;
        let enum_options = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::DYNAMIC_ENUM_ENUM_OPTIONS, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::DYNAMIC_ENUM_ENUM_OPTIONS_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self {
            enum_name,
            enum_options,
        })
    }
}

// Domain: container

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerMixDataEntry {
    pub from_item_id: wire::ZigZag32,
    pub reagent_item_id: wire::ZigZag32,
    pub to_item_id: wire::ZigZag32,
}

impl wire::Encode for ContainerMixDataEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.from_item_id.encode(writer)?;
        self.reagent_item_id.encode(writer)?;
        self.to_item_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerMixDataEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let from_item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let reagent_item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let to_item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            from_item_id,
            reagent_item_id,
            to_item_id,
        })
    }
}

/// FullContainerName contains information required to identify a container in a StackRequestSlotInfo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FullContainerName {
    /// `container_name` is the ID of the container that the slot was in.
    pub container_name: ContainerEnumName,
    /// `dynamic_id` is the ID of the container if it is dynamic. If the container is not dynamic, this field
    /// should be left empty. A non-optional value of 0 is assumed to be non-empty.
    /// Wire presence: optional value is preceded by a presence marker.
    pub dynamic_id: Option<wire::U32LE>,
}

impl wire::Encode for FullContainerName {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_name.encode(writer)?;
        match &self.dynamic_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for FullContainerName {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_name = <ContainerEnumName as wire::Decode>::decode(reader)?;
        let dynamic_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            container_name,
            dynamic_id,
        })
    }
}

// Domain: creative

/// CreativeGroup represents a group of items in the creative inventory. Each group has a category, name and
/// an icon that represents the group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreativeGroupInfo {
    /// `creative_category` is the category the group falls under. It is one of the constants above.
    pub creative_category: CreativeItemCategory,
    /// `name` is the locale name of the group, i.e. "itemGroup.name.planks".
    pub name: wire::Str,
    /// `group_icon_item` is the item that represents the group in the creative inventory.
    pub group_icon_item: NetworkItemInstanceDescriptorSerializedData,
}

impl wire::Encode for CreativeGroupInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.creative_category.encode(writer)?;
        self.name.encode(writer)?;
        self.group_icon_item.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreativeGroupInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let creative_category = <CreativeItemCategory as wire::Decode>::decode(reader)?;
        let name = reader.read_str(crate::fields::CREATIVE_GROUP_INFO_NAME)?;
        let group_icon_item =
            <NetworkItemInstanceDescriptorSerializedData as wire::Decode>::decode(reader)?;
        Ok(Self {
            creative_category,
            name,
            group_icon_item,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreativeItemEntry {
    pub creative_net_id: CreativeItemNetID,
    pub item_instance: NetworkItemInstanceDescriptorSerializedData,
    pub group_index: wire::VarUInt,
}

impl wire::Encode for CreativeItemEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.creative_net_id.encode(writer)?;
        self.item_instance.encode(writer)?;
        self.group_index.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreativeItemEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let creative_net_id = <CreativeItemNetID as wire::Decode>::decode(reader)?;
        let item_instance =
            <NetworkItemInstanceDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let group_index = <wire::VarUInt as wire::Decode>::decode(reader)?;
        Ok(Self {
            creative_net_id,
            item_instance,
            group_index,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CreativeItemNetID(pub u32);

impl wire::Encode for CreativeItemNetID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreativeItemNetID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::VarUInt as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: education

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EducationLevelSettings {
    /// `code_builder_default_uri` is the default URI that the code builder is ran on. Using this, a Code Builder
    /// program can make code directly affect the server.
    pub code_builder_default_uri: wire::Str,
    /// `code_builder_title` is the title of the code builder shown when connected to the CodeBuilderDefaultURI.
    pub code_builder_title: wire::Str,
    /// `can_resize_code_builder` specifies if clients connected to the world should be able to resize the code
    /// builder when it is opened.
    pub can_resize_code_builder: bool,
    /// `disable_legacy_title_bar` ...
    pub disable_legacy_title_bar: bool,
    /// `post_process_filter` ...
    pub post_process_filter: wire::Str,
    pub screenshot_border_resource_path: wire::Str,
    /// Wire presence: optional value is preceded by a presence marker.
    pub agent_capabilities: Option<bool>,
    pub local_settings: EducationLocalLevelSettings,
    pub deprecated_always_false: bool,
    /// `external_link_settings` ...
    /// Wire presence: optional value is preceded by a presence marker.
    pub external_link_settings: Option<ExternalLinkSettings>,
}

impl wire::Encode for EducationLevelSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.code_builder_default_uri.encode(writer)?;
        self.code_builder_title.encode(writer)?;
        self.can_resize_code_builder.encode(writer)?;
        self.disable_legacy_title_bar.encode(writer)?;
        self.post_process_filter.encode(writer)?;
        self.screenshot_border_resource_path.encode(writer)?;
        match &self.agent_capabilities {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.local_settings.encode(writer)?;
        self.deprecated_always_false.encode(writer)?;
        match &self.external_link_settings {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for EducationLevelSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let code_builder_default_uri =
            reader.read_str(crate::fields::EDUCATION_LEVEL_SETTINGS_CODE_BUILDER_DEFAULT_URI)?;
        let code_builder_title =
            reader.read_str(crate::fields::EDUCATION_LEVEL_SETTINGS_CODE_BUILDER_TITLE)?;
        let can_resize_code_builder = <bool as wire::Decode>::decode(reader)?;
        let disable_legacy_title_bar = <bool as wire::Decode>::decode(reader)?;
        let post_process_filter =
            reader.read_str(crate::fields::EDUCATION_LEVEL_SETTINGS_POST_PROCESS_FILTER)?;
        let screenshot_border_resource_path = reader
            .read_str(crate::fields::EDUCATION_LEVEL_SETTINGS_SCREENSHOT_BORDER_RESOURCE_PATH)?;
        let agent_capabilities = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let local_settings = <EducationLocalLevelSettings as wire::Decode>::decode(reader)?;
        let deprecated_always_false = <bool as wire::Decode>::decode(reader)?;
        let external_link_settings = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ExternalLinkSettings as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            code_builder_default_uri,
            code_builder_title,
            can_resize_code_builder,
            disable_legacy_title_bar,
            post_process_filter,
            screenshot_border_resource_path,
            agent_capabilities,
            local_settings,
            deprecated_always_false,
            external_link_settings,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EducationLocalLevelSettings {
    /// Wire presence: optional value is preceded by a presence marker.
    pub code_builder_override_uri: Option<wire::Str>,
}

impl wire::Encode for EducationLocalLevelSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.code_builder_override_uri {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for EducationLocalLevelSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let code_builder_override_uri = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(
                    crate::fields::EDUCATION_LOCAL_LEVEL_SETTINGS_CODE_BUILDER_OVERRIDE_URI,
                )?)
            }
        };
        Ok(Self {
            code_builder_override_uri,
        })
    }
}

// Domain: enchant

/// EnchantmentInstance represents a single enchantment instance with the type of the enchantment and its
/// level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnchantmentInstance {
    pub enchant_type: EnchantType,
    pub enchant_level: wire::U8,
}

impl wire::Encode for EnchantmentInstance {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.enchant_type.encode(writer)?;
        self.enchant_level.encode(writer)?;
        wire::check_number_limits(self.enchant_level.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for EnchantmentInstance {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let enchant_type = <EnchantType as wire::Decode>::decode(reader)?;
        let enchant_level = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            enchant_type,
            enchant_level,
        })
    }
}

// Domain: entity

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EntityNetId(pub u32);

impl wire::Encode for EntityNetId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EntityNetId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::VarUInt as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: entity_link

/// EntityLink is a link between two entities, typically being one entity riding another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityLink {
    /// RiddenEntityUniqueID is the entity unique ID of the entity that is being ridden. For a player sitting in a
    /// boat, this is the unique ID of the boat.
    pub target_a: ActorUniqueID,
    /// RiderEntityUniqueID is the entity unique ID of the entity that is riding. For a player sitting in a boat,
    /// this is the unique ID of the player.
    pub target_b: ActorUniqueID,
    /// `type_` is one of the types above. It specifies the way the entity is linked to another entity.
    pub type_: ActorLinkType,
    /// `immediate` is set to immediately dismount an entity from another. This should be set when the mount of an
    /// entity is killed.
    pub immediate: bool,
    /// RiderInitiated specifies if the link was created by the rider, for example the player starting to ride a
    /// horse by itself. This is generally true in vanilla environment for players.
    pub passenger_initiated: bool,
    /// `vehicle_angular_velocity` is the angular velocity of the vehicle that the rider is riding.
    pub vehicle_angular_velocity: wire::F32LE,
}

impl wire::Encode for EntityLink {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_a.encode(writer)?;
        self.target_b.encode(writer)?;
        self.type_.encode(writer)?;
        self.immediate.encode(writer)?;
        self.passenger_initiated.encode(writer)?;
        self.vehicle_angular_velocity.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EntityLink {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_a = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let target_b = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let type_ = <ActorLinkType as wire::Decode>::decode(reader)?;
        let immediate = <bool as wire::Decode>::decode(reader)?;
        let passenger_initiated = <bool as wire::Decode>::decode(reader)?;
        let vehicle_angular_velocity = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_a,
            target_b,
            type_,
            immediate,
            passenger_initiated,
            vehicle_angular_velocity,
        })
    }
}

// Domain: event

#[derive(Clone, Debug, PartialEq)]
pub enum EventData {
    Achievement {
        achievement_id: MinecraftEventingAchievementIds,
    },
    Interaction {
        interacted_entity_id: wire::ZigZag64,
        interaction_type: MinecraftEventingInteractionType,
        interaction_actor_type: wire::ZigZag32,
        interaction_actor_variant: wire::ZigZag32,
        interaction_actor_color: wire::U8,
    },
    PortalCreated {
        dimension_id: wire::ZigZag32,
    },
    PortalUsed {
        /// FromDimensionID ...
        source_dimension_id: wire::ZigZag32,
        /// ToDimensionID ...
        target_dimension_id: wire::ZigZag32,
    },
    MobKilled {
        /// KillerEntityUniqueID ...
        instigator_actor_id: wire::ZigZag64,
        /// VictimEntityUniqueID ...
        target_actor_id: wire::ZigZag64,
        /// KillerEntityType ...
        instigator_child_actor_type: ActorType,
        /// EntityDamageCause ...
        damage_source: wire::ZigZag32,
        /// VillagerTradeTier -1 if not a trading actor.
        trade_tier: wire::ZigZag32,
        /// VillagerDisplayName Empty if not a trading actor.
        trader_name: wire::Str,
    },
    CauldronUsed {
        /// Colour ...
        contents_color: wire::VarUInt,
        /// PotionID ...
        contents_type: wire::ZigZag32,
        /// `fill_level` ...
        fill_level: wire::ZigZag32,
    },
    PlayerDied {
        /// AttackerEntityID ...
        instigator_actor_id: wire::ZigZag32,
        /// AttackerVariant ...
        instigator_mob_variant: wire::ZigZag32,
        /// EntityDamageCause ...
        damage_source: wire::ZigZag32,
        /// InRaid ...
        died_in_raid: bool,
    },
    BossKilled {
        /// BossEntityUniqueID ...
        boss_actor_id: wire::ZigZag64,
        /// PlayerPartySize ...
        party_size: wire::ZigZag32,
        /// InteractionEntityType ...
        boss_type: wire::ZigZag32,
    },
    SlashCommand {
        success_count: wire::ZigZag32,
        error_count: wire::ZigZag32,
        command_name: wire::Str,
        error_list: wire::Str,
    },
    MobBorn {
        /// EntityType ...
        born_baby_entity_type: wire::ZigZag32,
        /// Variant ...
        born_baby_entity_variant: wire::ZigZag32,
        /// Colour ...
        born_baby_color: wire::U8,
    },
    PoiCauldronUsed {
        /// `block_interaction_type` ...
        block_interaction_type: MinecraftEventingPOIBlockInteractionType,
        /// `item_id` ...
        item_id: wire::ZigZag32,
    },
    ComposterUsed {
        /// `block_interaction_type` ...
        block_interaction_type: MinecraftEventingPOIBlockInteractionType,
        /// `item_id` ...
        item_id: wire::ZigZag32,
    },
    BellUsed {
        /// `item_id` ...
        item_id: wire::ZigZag32,
    },
    ActorDefinition {
        event_name: wire::Str,
    },
    RaidUpdate {
        /// CurrentRaidWave ...
        current_wave: wire::ZigZag32,
        /// TotalRaidWaves ...
        total_waves: wire::ZigZag32,
        /// WonRaid ...
        success: bool,
    },
    TargetBlockHit {
        /// `redstone_level` ...
        redstone_level: wire::ZigZag32,
    },
    PiglinBarter {
        /// `item_id` ...
        item_id: wire::ZigZag32,
        /// `was_targeting_bartering_player` ...
        was_targeting_bartering_player: bool,
    },
    PlayerWaxedOrUnwaxedCopper {
        player_waxed_or_unwaxed_copper_block_id: wire::ZigZag32,
    },
    CodeBuilderRuntimeAction {
        /// Action ...
        code_builder_runtime_action: wire::Str,
    },
    CodeBuilderScoreboard {
        /// `objective_name` ...
        objective_name: wire::Str,
        /// `score` ...
        score: wire::ZigZag32,
    },
    ItemUsed {
        item_id: wire::I16LE,
        item_aux: wire::I32LE,
        use_method: wire::I32LE,
        count: wire::I32LE,
    },
    Empty,
}

impl EventData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Achievement { .. } => 0,
            Self::Interaction { .. } => 1,
            Self::PortalCreated { .. } => 2,
            Self::PortalUsed { .. } => 3,
            Self::MobKilled { .. } => 4,
            Self::CauldronUsed { .. } => 5,
            Self::PlayerDied { .. } => 6,
            Self::BossKilled { .. } => 7,
            Self::SlashCommand { .. } => 8,
            Self::MobBorn { .. } => 9,
            Self::PoiCauldronUsed { .. } => 10,
            Self::ComposterUsed { .. } => 11,
            Self::BellUsed { .. } => 12,
            Self::ActorDefinition { .. } => 13,
            Self::RaidUpdate { .. } => 14,
            Self::TargetBlockHit { .. } => 15,
            Self::PiglinBarter { .. } => 16,
            Self::PlayerWaxedOrUnwaxedCopper { .. } => 17,
            Self::CodeBuilderRuntimeAction { .. } => 18,
            Self::CodeBuilderScoreboard { .. } => 19,
            Self::ItemUsed { .. } => 20,
            Self::Empty => 21,
        }
    }
}

impl Default for EventData {
    fn default() -> Self {
        Self::Achievement {
            achievement_id: Default::default(),
        }
    }
}

impl wire::Encode for EventData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Achievement { achievement_id } => {
                achievement_id.encode(writer)?;
            }
            Self::Interaction {
                interacted_entity_id,
                interaction_type,
                interaction_actor_type,
                interaction_actor_variant,
                interaction_actor_color,
            } => {
                interacted_entity_id.encode(writer)?;
                interaction_type.encode(writer)?;
                interaction_actor_type.encode(writer)?;
                interaction_actor_variant.encode(writer)?;
                interaction_actor_color.encode(writer)?;
            }
            Self::PortalCreated { dimension_id } => {
                dimension_id.encode(writer)?;
            }
            Self::PortalUsed {
                source_dimension_id,
                target_dimension_id,
            } => {
                source_dimension_id.encode(writer)?;
                target_dimension_id.encode(writer)?;
            }
            Self::MobKilled {
                instigator_actor_id,
                target_actor_id,
                instigator_child_actor_type,
                damage_source,
                trade_tier,
                trader_name,
            } => {
                instigator_actor_id.encode(writer)?;
                target_actor_id.encode(writer)?;
                instigator_child_actor_type.encode(writer)?;
                damage_source.encode(writer)?;
                trade_tier.encode(writer)?;
                wire::encode_string_limits(writer, trader_name, 0, 128)?;
            }
            Self::CauldronUsed {
                contents_color,
                contents_type,
                fill_level,
            } => {
                contents_color.encode(writer)?;
                contents_type.encode(writer)?;
                fill_level.encode(writer)?;
            }
            Self::PlayerDied {
                instigator_actor_id,
                instigator_mob_variant,
                damage_source,
                died_in_raid,
            } => {
                instigator_actor_id.encode(writer)?;
                instigator_mob_variant.encode(writer)?;
                damage_source.encode(writer)?;
                died_in_raid.encode(writer)?;
            }
            Self::BossKilled {
                boss_actor_id,
                party_size,
                boss_type,
            } => {
                boss_actor_id.encode(writer)?;
                party_size.encode(writer)?;
                boss_type.encode(writer)?;
            }
            Self::SlashCommand {
                success_count,
                error_count,
                command_name,
                error_list,
            } => {
                success_count.encode(writer)?;
                error_count.encode(writer)?;
                wire::encode_string_limits(writer, command_name, 0, 512)?;
                wire::encode_string_limits(writer, error_list, 0, 2048)?;
            }
            Self::MobBorn {
                born_baby_entity_type,
                born_baby_entity_variant,
                born_baby_color,
            } => {
                born_baby_entity_type.encode(writer)?;
                born_baby_entity_variant.encode(writer)?;
                born_baby_color.encode(writer)?;
            }
            Self::PoiCauldronUsed {
                block_interaction_type,
                item_id,
            } => {
                block_interaction_type.encode(writer)?;
                item_id.encode(writer)?;
            }
            Self::ComposterUsed {
                block_interaction_type,
                item_id,
            } => {
                block_interaction_type.encode(writer)?;
                item_id.encode(writer)?;
            }
            Self::BellUsed { item_id } => {
                item_id.encode(writer)?;
            }
            Self::ActorDefinition { event_name } => {
                wire::encode_string_limits(writer, event_name, 0, 256)?;
            }
            Self::RaidUpdate {
                current_wave,
                total_waves,
                success,
            } => {
                current_wave.encode(writer)?;
                total_waves.encode(writer)?;
                success.encode(writer)?;
            }
            Self::TargetBlockHit { redstone_level } => {
                redstone_level.encode(writer)?;
            }
            Self::PiglinBarter {
                item_id,
                was_targeting_bartering_player,
            } => {
                item_id.encode(writer)?;
                was_targeting_bartering_player.encode(writer)?;
            }
            Self::PlayerWaxedOrUnwaxedCopper {
                player_waxed_or_unwaxed_copper_block_id,
            } => {
                player_waxed_or_unwaxed_copper_block_id.encode(writer)?;
            }
            Self::CodeBuilderRuntimeAction {
                code_builder_runtime_action,
            } => {
                wire::encode_string_limits(writer, code_builder_runtime_action, 0, 16)?;
            }
            Self::CodeBuilderScoreboard {
                objective_name,
                score,
            } => {
                wire::encode_string_limits(writer, objective_name, 0, 256)?;
                score.encode(writer)?;
            }
            Self::ItemUsed {
                item_id,
                item_aux,
                use_method,
                count,
            } => {
                item_id.encode(writer)?;
                item_aux.encode(writer)?;
                use_method.encode(writer)?;
                count.encode(writer)?;
            }
            Self::Empty => {}
        }
        Ok(())
    }
}

impl wire::Decode for EventData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let achievement_id =
                    <MinecraftEventingAchievementIds as wire::Decode>::decode(reader)?;
                Self::Achievement { achievement_id }
            }
            1 => {
                let interacted_entity_id = <wire::ZigZag64 as wire::Decode>::decode(reader)?;
                let interaction_type =
                    <MinecraftEventingInteractionType as wire::Decode>::decode(reader)?;
                let interaction_actor_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let interaction_actor_variant = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let interaction_actor_color = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::Interaction {
                    interacted_entity_id,
                    interaction_type,
                    interaction_actor_type,
                    interaction_actor_variant,
                    interaction_actor_color,
                }
            }
            2 => {
                let dimension_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::PortalCreated { dimension_id }
            }
            3 => {
                let source_dimension_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let target_dimension_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::PortalUsed {
                    source_dimension_id,
                    target_dimension_id,
                }
            }
            4 => {
                let instigator_actor_id = <wire::ZigZag64 as wire::Decode>::decode(reader)?;
                let target_actor_id = <wire::ZigZag64 as wire::Decode>::decode(reader)?;
                let instigator_child_actor_type = <ActorType as wire::Decode>::decode(reader)?;
                let damage_source = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let trade_tier = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let trader_name = wire::decode_string_limits(
                    reader,
                    crate::fields::EVENT_DATA_MOB_KILLED_TRADER_NAME,
                    0,
                    128,
                )?;
                Self::MobKilled {
                    instigator_actor_id,
                    target_actor_id,
                    instigator_child_actor_type,
                    damage_source,
                    trade_tier,
                    trader_name,
                }
            }
            5 => {
                let contents_color = <wire::VarUInt as wire::Decode>::decode(reader)?;
                let contents_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let fill_level = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::CauldronUsed {
                    contents_color,
                    contents_type,
                    fill_level,
                }
            }
            6 => {
                let instigator_actor_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let instigator_mob_variant = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let damage_source = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let died_in_raid = <bool as wire::Decode>::decode(reader)?;
                Self::PlayerDied {
                    instigator_actor_id,
                    instigator_mob_variant,
                    damage_source,
                    died_in_raid,
                }
            }
            7 => {
                let boss_actor_id = <wire::ZigZag64 as wire::Decode>::decode(reader)?;
                let party_size = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let boss_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::BossKilled {
                    boss_actor_id,
                    party_size,
                    boss_type,
                }
            }
            8 => {
                let success_count = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let error_count = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let command_name = wire::decode_string_limits(
                    reader,
                    crate::fields::EVENT_DATA_SLASH_COMMAND_COMMAND_NAME,
                    0,
                    512,
                )?;
                let error_list = wire::decode_string_limits(
                    reader,
                    crate::fields::EVENT_DATA_SLASH_COMMAND_ERROR_LIST,
                    0,
                    2048,
                )?;
                Self::SlashCommand {
                    success_count,
                    error_count,
                    command_name,
                    error_list,
                }
            }
            9 => {
                let born_baby_entity_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let born_baby_entity_variant = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let born_baby_color = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::MobBorn {
                    born_baby_entity_type,
                    born_baby_entity_variant,
                    born_baby_color,
                }
            }
            10 => {
                let block_interaction_type =
                    <MinecraftEventingPOIBlockInteractionType as wire::Decode>::decode(reader)?;
                let item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::PoiCauldronUsed {
                    block_interaction_type,
                    item_id,
                }
            }
            11 => {
                let block_interaction_type =
                    <MinecraftEventingPOIBlockInteractionType as wire::Decode>::decode(reader)?;
                let item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::ComposterUsed {
                    block_interaction_type,
                    item_id,
                }
            }
            12 => {
                let item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::BellUsed { item_id }
            }
            13 => {
                let event_name = wire::decode_string_limits(
                    reader,
                    crate::fields::EVENT_DATA_ACTOR_DEFINITION_EVENT_NAME,
                    0,
                    256,
                )?;
                Self::ActorDefinition { event_name }
            }
            14 => {
                let current_wave = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let total_waves = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let success = <bool as wire::Decode>::decode(reader)?;
                Self::RaidUpdate {
                    current_wave,
                    total_waves,
                    success,
                }
            }
            15 => {
                let redstone_level = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::TargetBlockHit { redstone_level }
            }
            16 => {
                let item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let was_targeting_bartering_player = <bool as wire::Decode>::decode(reader)?;
                Self::PiglinBarter {
                    item_id,
                    was_targeting_bartering_player,
                }
            }
            17 => {
                let player_waxed_or_unwaxed_copper_block_id =
                    <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::PlayerWaxedOrUnwaxedCopper {
                    player_waxed_or_unwaxed_copper_block_id,
                }
            }
            18 => {
                let code_builder_runtime_action = wire::decode_string_limits(reader, crate::fields::EVENT_DATA_CODE_BUILDER_RUNTIME_ACTION_CODE_BUILDER_RUNTIME_ACTION, 0, 16)?;
                Self::CodeBuilderRuntimeAction {
                    code_builder_runtime_action,
                }
            }
            19 => {
                let objective_name = wire::decode_string_limits(
                    reader,
                    crate::fields::EVENT_DATA_CODE_BUILDER_SCOREBOARD_OBJECTIVE_NAME,
                    0,
                    256,
                )?;
                let score = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::CodeBuilderScoreboard {
                    objective_name,
                    score,
                }
            }
            20 => {
                let item_id = <wire::I16LE as wire::Decode>::decode(reader)?;
                let item_aux = <wire::I32LE as wire::Decode>::decode(reader)?;
                let use_method = <wire::I32LE as wire::Decode>::decode(reader)?;
                let count = <wire::I32LE as wire::Decode>::decode(reader)?;
                Self::ItemUsed {
                    item_id,
                    item_aux,
                    use_method,
                    count,
                }
            }
            21 => Self::Empty,
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "EventData",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: experiment

/// ExperimentData holds data on an experiment that is either enabled or disabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExperimentToggle {
    /// `name` is the name of the experiment.
    pub name: wire::Str,
    /// `enabled` specifies if the experiment is enabled. Vanilla typically always sets this to true for any
    /// experiments sent.
    pub enabled: bool,
}

impl wire::Encode for ExperimentToggle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.enabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ExperimentToggle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::EXPERIMENT_TOGGLE_NAME)?;
        let enabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self { name, enabled })
    }
}

// Domain: furnace

/// FurnaceOptions holds the options that a player has selected in a furnace-like container's UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FurnaceOptions {
    /// `left_furnace_tab` is the tab that is selected on the left side of the furnace UI. It is one of the
    /// FurnaceLeftTab constants above.
    pub left_furnace_tab: FurnaceLeftTabIndex,
    /// `filtering` is whether the player has enabled the filtering between recipes they have unlocked or not.
    pub filtering: bool,
    /// `layout` is the layout of the furnace UI. It is one of the FurnaceLayout constants above.
    pub layout: FurnaceLayout,
}

impl wire::Encode for FurnaceOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.left_furnace_tab.encode(writer)?;
        self.filtering.encode(writer)?;
        self.layout.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FurnaceOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let left_furnace_tab = <FurnaceLeftTabIndex as wire::Decode>::decode(reader)?;
        let filtering = <bool as wire::Decode>::decode(reader)?;
        let layout = <FurnaceLayout as wire::Decode>::decode(reader)?;
        Ok(Self {
            left_furnace_tab,
            filtering,
            layout,
        })
    }
}

// Domain: game_rule

/// GameRule contains game rule data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameRule {
    /// `rule_name` is the name of the game rule.
    pub rule_name: wire::Str,
    /// `rule_can_be_modified` specifies if the game rule can be modified by the player through the in-game UI.
    pub rule_can_be_modified: bool,
    /// `rule_value` is the new value of the game rule. This is either a bool, uint32 or float32, or nil for the
    /// null variant, which carries no value at all.
    pub rule_value: GameRuleValue,
}

impl wire::Encode for GameRule {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.rule_name.encode(writer)?;
        self.rule_can_be_modified.encode(writer)?;
        self.rule_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameRule {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rule_name = reader.read_str(crate::fields::GAME_RULE_RULE_NAME)?;
        let rule_can_be_modified = <bool as wire::Decode>::decode(reader)?;
        let rule_value = <GameRuleValue as wire::Decode>::decode(reader)?;
        Ok(Self {
            rule_name,
            rule_can_be_modified,
            rule_value,
        })
    }
}

// Domain: generated

#[derive(Clone, Debug, PartialEq)]
pub enum BedrockDDUIDataStoreUpdateData {
    Double(wire::F64LE),
    Bool(bool),
    String(wire::Str),
}

impl BedrockDDUIDataStoreUpdateData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Double(..) => 0,
            Self::Bool(..) => 1,
            Self::String(..) => 2,
        }
    }
}

impl Default for BedrockDDUIDataStoreUpdateData {
    fn default() -> Self {
        Self::Double(Default::default())
    }
}

impl wire::Encode for BedrockDDUIDataStoreUpdateData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Double(value) => {
                value.encode(writer)?;
            }
            Self::Bool(value) => {
                value.encode(writer)?;
            }
            Self::String(value) => {
                wire::encode_string_limits(writer, value, 0, 5000)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for BedrockDDUIDataStoreUpdateData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::Double(<wire::F64LE as wire::Decode>::decode(reader)?),
            1 => Self::Bool(<bool as wire::Decode>::decode(reader)?),
            2 => Self::String(wire::decode_string_limits(
                reader,
                crate::fields::BEDROCK_DDUI_DATA_STORE_UPDATE_DATA_STRING,
                0,
                5000,
            )?),
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "BedrockDDUIDataStoreUpdateData",
                    value: value as i64,
                });
            }
        })
    }
}

/// Stores the 131-bit value used by the wire bitset encoding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitset131(pub [u64; 3]);

impl wire::Encode for Bitset131 {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_bitset(writer, &self.0, 131)?;
        Ok(())
    }
}

impl wire::Decode for Bitset131 {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(wire::decode_bitset(reader, 131)?))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataItemEntryValue {
    DataItemByte {
        type_: DataItemType,
        value: wire::I8,
    },
    DataItemShort {
        type_: DataItemType,
        value: wire::I16LE,
    },
    DataItemInt {
        type_: DataItemType,
        value: wire::ZigZag32,
    },
    DataItemFloat {
        type_: DataItemType,
        value: wire::F32LE,
    },
    DataItemString {
        type_: DataItemType,
        value: wire::Str,
    },
    DataItemCompoundTag {
        type_: DataItemType,
        value: wire::NetworkNbt,
    },
    DataItemPos {
        type_: DataItemType,
        value: BlockPos,
    },
    DataItemInt64 {
        type_: DataItemType,
        value: wire::ZigZag64,
    },
    DataItemVec3 {
        type_: DataItemType,
        value: glam::Vec3,
    },
}

impl DataItemEntryValue {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::DataItemByte { .. } => 0,
            Self::DataItemShort { .. } => 1,
            Self::DataItemInt { .. } => 2,
            Self::DataItemFloat { .. } => 3,
            Self::DataItemString { .. } => 4,
            Self::DataItemCompoundTag { .. } => 5,
            Self::DataItemPos { .. } => 6,
            Self::DataItemInt64 { .. } => 7,
            Self::DataItemVec3 { .. } => 8,
        }
    }
}

impl Default for DataItemEntryValue {
    fn default() -> Self {
        Self::DataItemByte {
            type_: Default::default(),
            value: Default::default(),
        }
    }
}

impl wire::Encode for DataItemEntryValue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::DataItemByte { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
                wire::check_number_limits(value.0, Some(-128), Some(127))?;
            }
            Self::DataItemShort { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemInt { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemFloat { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemString { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemCompoundTag { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemPos { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemInt64 { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
            Self::DataItemVec3 { type_, value } => {
                type_.encode(writer)?;
                value.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for DataItemEntryValue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = {
                    let value = <wire::I8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(-128), Some(127))?;
                    value
                };
                Self::DataItemByte { type_, value }
            }
            1 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <wire::I16LE as wire::Decode>::decode(reader)?;
                Self::DataItemShort { type_, value }
            }
            2 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                Self::DataItemInt { type_, value }
            }
            3 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::DataItemFloat { type_, value }
            }
            4 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value =
                    reader.read_str(crate::fields::DATA_ITEM_ENTRY_VALUE_DATA_ITEM_STRING_VALUE)?;
                Self::DataItemString { type_, value }
            }
            5 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
                Self::DataItemCompoundTag { type_, value }
            }
            6 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <BlockPos as wire::Decode>::decode(reader)?;
                Self::DataItemPos { type_, value }
            }
            7 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <wire::ZigZag64 as wire::Decode>::decode(reader)?;
                Self::DataItemInt64 { type_, value }
            }
            8 => {
                let type_ = <DataItemType as wire::Decode>::decode(reader)?;
                let value = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::DataItemVec3 { type_, value }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "DataItemEntryValue",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisconnectMessages {
    DisconnectPacketMessages {
        message: wire::Str,
        filtered_message: wire::Str,
    },
    /// Naming overlay required: source placeholder `Empty1`.
    Empty,
}

impl DisconnectMessages {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::DisconnectPacketMessages { .. } => 0,
            Self::Empty => 1,
        }
    }
}

impl Default for DisconnectMessages {
    fn default() -> Self {
        Self::DisconnectPacketMessages {
            message: Default::default(),
            filtered_message: Default::default(),
        }
    }
}

impl wire::Encode for DisconnectMessages {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::DisconnectPacketMessages {
                message,
                filtered_message,
            } => {
                message.encode(writer)?;
                filtered_message.encode(writer)?;
            }
            Self::Empty => {}
        }
        Ok(())
    }
}

impl wire::Decode for DisconnectMessages {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let message = reader.read_str(
                    crate::fields::DISCONNECT_MESSAGES_DISCONNECT_PACKET_MESSAGES_MESSAGE,
                )?;
                let filtered_message = reader.read_str(
                    crate::fields::DISCONNECT_MESSAGES_DISCONNECT_PACKET_MESSAGES_FILTERED_MESSAGE,
                )?;
                Self::DisconnectPacketMessages {
                    message,
                    filtered_message,
                }
            }
            1 => Self::Empty,
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "DisconnectMessages",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum GameRuleValue {
    /// Naming overlay required: source placeholder `Empty0`.
    #[default]
    Empty,
    Bool(bool),
    Int32(wire::I32LE),
    Float(wire::F32LE),
}

impl GameRuleValue {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Bool(..) => 1,
            Self::Int32(..) => 2,
            Self::Float(..) => 3,
        }
    }
}

impl wire::Encode for GameRuleValue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Empty => {}
            Self::Bool(value) => {
                value.encode(writer)?;
            }
            Self::Int32(value) => {
                value.encode(writer)?;
            }
            Self::Float(value) => {
                value.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for GameRuleValue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::Empty,
            1 => Self::Bool(<bool as wire::Decode>::decode(reader)?),
            2 => Self::Int32(<wire::I32LE as wire::Decode>::decode(reader)?),
            3 => Self::Float(<wire::F32LE as wire::Decode>::decode(reader)?),
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "GameRuleValue",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InventoryTransactionPacketData {
    NormalTransactionData {
        actions: InventoryTransactionData,
    },
    InventoryMismatchData {
        actions: InventoryTransactionData,
    },
    ItemUseInventoryTransaction(Box<ItemUseInventoryTransaction>),
    ItemUseOnActorInventoryTransaction {
        actions: InventoryTransactionData,
        runtime_id: ActorRuntimeID,
        action_type: ItemUseOnActorInventoryTransactionActionType,
        slot: wire::ZigZag32,
        item: NetworkItemStackDescriptorSerializedData,
        from_position: glam::Vec3,
        hit_position: glam::Vec3,
    },
    ItemReleaseInventoryTransaction {
        actions: InventoryTransactionData,
        action_type: ItemReleaseInventoryTransactionActionType,
        slot: wire::ZigZag32,
        item: NetworkItemStackDescriptorSerializedData,
        from_position: glam::Vec3,
    },
}

impl InventoryTransactionPacketData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::NormalTransactionData { .. } => 0,
            Self::InventoryMismatchData { .. } => 1,
            Self::ItemUseInventoryTransaction(..) => 2,
            Self::ItemUseOnActorInventoryTransaction { .. } => 3,
            Self::ItemReleaseInventoryTransaction { .. } => 4,
        }
    }
}

impl Default for InventoryTransactionPacketData {
    fn default() -> Self {
        Self::NormalTransactionData {
            actions: Default::default(),
        }
    }
}

impl wire::Encode for InventoryTransactionPacketData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::NormalTransactionData { actions } => {
                actions.encode(writer)?;
            }
            Self::InventoryMismatchData { actions } => {
                actions.encode(writer)?;
            }
            Self::ItemUseInventoryTransaction(value) => {
                value.encode(writer)?;
            }
            Self::ItemUseOnActorInventoryTransaction {
                actions,
                runtime_id,
                action_type,
                slot,
                item,
                from_position,
                hit_position,
            } => {
                actions.encode(writer)?;
                runtime_id.encode(writer)?;
                action_type.encode(writer)?;
                slot.encode(writer)?;
                item.encode(writer)?;
                from_position.encode(writer)?;
                hit_position.encode(writer)?;
            }
            Self::ItemReleaseInventoryTransaction {
                actions,
                action_type,
                slot,
                item,
                from_position,
            } => {
                actions.encode(writer)?;
                action_type.encode(writer)?;
                slot.encode(writer)?;
                item.encode(writer)?;
                from_position.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for InventoryTransactionPacketData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let actions = <InventoryTransactionData as wire::Decode>::decode(reader)?;
                Self::NormalTransactionData { actions }
            }
            1 => {
                let actions = <InventoryTransactionData as wire::Decode>::decode(reader)?;
                Self::InventoryMismatchData { actions }
            }
            2 => Self::ItemUseInventoryTransaction(Box::new(
                <ItemUseInventoryTransaction as wire::Decode>::decode(reader)?,
            )),
            3 => {
                let actions = <InventoryTransactionData as wire::Decode>::decode(reader)?;
                let runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
                let action_type =
                    <ItemUseOnActorInventoryTransactionActionType as wire::Decode>::decode(reader)?;
                let slot = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let item =
                    <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
                let from_position = <glam::Vec3 as wire::Decode>::decode(reader)?;
                let hit_position = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::ItemUseOnActorInventoryTransaction {
                    actions,
                    runtime_id,
                    action_type,
                    slot,
                    item,
                    from_position,
                    hit_position,
                }
            }
            4 => {
                let actions = <InventoryTransactionData as wire::Decode>::decode(reader)?;
                let action_type =
                    <ItemReleaseInventoryTransactionActionType as wire::Decode>::decode(reader)?;
                let slot = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let item =
                    <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
                let from_position = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::ItemReleaseInventoryTransaction {
                    actions,
                    action_type,
                    slot,
                    item,
                    from_position,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "InventoryTransactionPacketData",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum PrimitiveShapeExtraShapeData {
    /// Naming overlay required: source placeholder `Empty0`.
    #[default]
    Empty,
    ArrowData {
        /// `arrow_end_location` is the arrow end location of the shape.
        /// Wire presence: optional value is preceded by a presence marker.
        arrow_end_location: Option<glam::Vec3>,
        /// `arrow_head_length` is the arrow head length of the shape.
        /// Wire presence: optional value is preceded by a presence marker.
        arrow_head_length: Option<wire::F32LE>,
        /// `arrow_head_radius` is the arrow head radius of the shape.
        /// Wire presence: optional value is preceded by a presence marker.
        arrow_head_radius: Option<wire::F32LE>,
        /// Segments is the segments that used for the debug arrow's head.
        /// Wire presence: optional value is preceded by a presence marker.
        num_segments: Option<wire::U8>,
    },
    TextData {
        /// `text` is the text of the debug text shape.
        text: wire::Str,
        /// `use_rotation` is if the text should use the provided rotation, meaning it will be static and does not
        /// follow the camera. Use false for default behaviour.
        use_rotation: bool,
        /// `background_color` is the RGBA colour to use for the text background. This is a translucent black colour
        /// by default.
        /// Wire presence: optional value is preceded by a presence marker.
        background_color: Option<MceColor>,
        /// `line_gap_height` is the gap to leave between each line of multiline text. If not set, the client uses its
        /// default gap.
        line_gap_height: wire::F32LE,
        /// `depth_test` is whether the text should show through walls. Use true for default behaviour.
        depth_test: bool,
        /// `show_backface` is if the background should render on the back side of the shape. This only has a visible
        /// effect when UseRotation is true since you cannot see the back side of the text otherwise. Use true for
        /// default behaviour.
        show_backface: bool,
        /// `show_text_backface` is if the text should render on the back side of the shape. This only has a visible
        /// effect when UseRotation is true since you cannot see the back side of the text otherwise. Use true for
        /// default behaviour.
        show_text_backface: bool,
    },
    BoxData {
        box_bound: glam::Vec3,
    },
    LineData {
        line_end_location: glam::Vec3,
    },
    SphereData {
        num_segments: wire::U8,
    },
    CylinderData {
        /// `radius_x` is the radius of the cylinder along the X axis.
        radius_x: glam::Vec2,
        /// `radius_z` is the radius of the cylinder along the Z axis.
        radius_z: glam::Vec2,
        /// `height` is the height of the cylinder.
        height: wire::F32LE,
        /// `num_segments` is the number of segments used for the cylinder.
        num_segments: wire::U8,
    },
    PyramidData {
        /// `width` is the width along the X axis of the pyramid base.
        width: wire::F32LE,
        /// `depth` is the optional depth along the Z axis of the pyramid base. It defaults to Width if unset.
        /// Wire presence: optional value is preceded by a presence marker.
        depth: Option<wire::F32LE>,
        /// `height` is the height of the pyramid.
        height: wire::F32LE,
    },
    EllipsoidData {
        /// `radii` are the radii of the ellipsoid along the X, Y and Z axes.
        radii: glam::Vec3,
        /// `segments_per_axis` is the number of segments used per axis for the ellipsoid.
        segments_per_axis: wire::U8,
    },
    ConeData {
        /// `radii` are the radii along the X/Z axes of the cone base.
        radii: glam::Vec2,
        /// `height` is the height of the cone.
        height: wire::F32LE,
        /// `num_segments` is the number of segments used for the cone.
        num_segments: wire::U8,
    },
}

impl PrimitiveShapeExtraShapeData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::ArrowData { .. } => 1,
            Self::TextData { .. } => 2,
            Self::BoxData { .. } => 3,
            Self::LineData { .. } => 4,
            Self::SphereData { .. } => 5,
            Self::CylinderData { .. } => 6,
            Self::PyramidData { .. } => 7,
            Self::EllipsoidData { .. } => 8,
            Self::ConeData { .. } => 9,
        }
    }
}

impl wire::Encode for PrimitiveShapeExtraShapeData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Empty => {}
            Self::ArrowData {
                arrow_end_location,
                arrow_head_length,
                arrow_head_radius,
                num_segments,
            } => {
                match &arrow_end_location {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                match &arrow_head_length {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                match &arrow_head_radius {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                match &num_segments {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
            }
            Self::TextData {
                text,
                use_rotation,
                background_color,
                line_gap_height,
                depth_test,
                show_backface,
                show_text_backface,
            } => {
                text.encode(writer)?;
                use_rotation.encode(writer)?;
                match &background_color {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                line_gap_height.encode(writer)?;
                depth_test.encode(writer)?;
                show_backface.encode(writer)?;
                show_text_backface.encode(writer)?;
            }
            Self::BoxData { box_bound } => {
                box_bound.encode(writer)?;
            }
            Self::LineData { line_end_location } => {
                line_end_location.encode(writer)?;
            }
            Self::SphereData { num_segments } => {
                num_segments.encode(writer)?;
            }
            Self::CylinderData {
                radius_x,
                radius_z,
                height,
                num_segments,
            } => {
                radius_x.encode(writer)?;
                radius_z.encode(writer)?;
                height.encode(writer)?;
                num_segments.encode(writer)?;
            }
            Self::PyramidData {
                width,
                depth,
                height,
            } => {
                width.encode(writer)?;
                match &depth {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
                height.encode(writer)?;
            }
            Self::EllipsoidData {
                radii,
                segments_per_axis,
            } => {
                radii.encode(writer)?;
                segments_per_axis.encode(writer)?;
            }
            Self::ConeData {
                radii,
                height,
                num_segments,
            } => {
                radii.encode(writer)?;
                height.encode(writer)?;
                num_segments.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for PrimitiveShapeExtraShapeData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::Empty,
            1 => {
                let arrow_end_location = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
                    }
                };
                let arrow_head_length = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::F32LE as wire::Decode>::decode(reader)?)
                    }
                };
                let arrow_head_radius = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::F32LE as wire::Decode>::decode(reader)?)
                    }
                };
                let num_segments = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::U8 as wire::Decode>::decode(reader)?)
                    }
                };
                Self::ArrowData {
                    arrow_end_location,
                    arrow_head_length,
                    arrow_head_radius,
                    num_segments,
                }
            }
            2 => {
                let text = reader
                    .read_str(crate::fields::PRIMITIVE_SHAPE_EXTRA_SHAPE_DATA_TEXT_DATA_TEXT)?;
                let use_rotation = <bool as wire::Decode>::decode(reader)?;
                let background_color = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<MceColor as wire::Decode>::decode(reader)?)
                    }
                };
                let line_gap_height = <wire::F32LE as wire::Decode>::decode(reader)?;
                let depth_test = <bool as wire::Decode>::decode(reader)?;
                let show_backface = <bool as wire::Decode>::decode(reader)?;
                let show_text_backface = <bool as wire::Decode>::decode(reader)?;
                Self::TextData {
                    text,
                    use_rotation,
                    background_color,
                    line_gap_height,
                    depth_test,
                    show_backface,
                    show_text_backface,
                }
            }
            3 => {
                let box_bound = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::BoxData { box_bound }
            }
            4 => {
                let line_end_location = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::LineData { line_end_location }
            }
            5 => {
                let num_segments = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::SphereData { num_segments }
            }
            6 => {
                let radius_x = <glam::Vec2 as wire::Decode>::decode(reader)?;
                let radius_z = <glam::Vec2 as wire::Decode>::decode(reader)?;
                let height = <wire::F32LE as wire::Decode>::decode(reader)?;
                let num_segments = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::CylinderData {
                    radius_x,
                    radius_z,
                    height,
                    num_segments,
                }
            }
            7 => {
                let width = <wire::F32LE as wire::Decode>::decode(reader)?;
                let depth = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(<wire::F32LE as wire::Decode>::decode(reader)?)
                    }
                };
                let height = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::PyramidData {
                    width,
                    depth,
                    height,
                }
            }
            8 => {
                let radii = <glam::Vec3 as wire::Decode>::decode(reader)?;
                let segments_per_axis = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::EllipsoidData {
                    radii,
                    segments_per_axis,
                }
            }
            9 => {
                let radii = <glam::Vec2 as wire::Decode>::decode(reader)?;
                let height = <wire::F32LE as wire::Decode>::decode(reader)?;
                let num_segments = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::ConeData {
                    radii,
                    height,
                    num_segments,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "PrimitiveShapeExtraShapeData",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerboundPackSettingChangePackSettingValue {
    Float(wire::F32LE),
    Bool(bool),
    String(wire::Str),
    Case3(Vec<wire::Str>),
}

impl ServerboundPackSettingChangePackSettingValue {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Float(..) => 0,
            Self::Bool(..) => 1,
            Self::String(..) => 2,
            Self::Case3(..) => 3,
        }
    }
}

impl Default for ServerboundPackSettingChangePackSettingValue {
    fn default() -> Self {
        Self::Float(Default::default())
    }
}

impl wire::Encode for ServerboundPackSettingChangePackSettingValue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Float(value) => {
                value.encode(writer)?;
            }
            Self::Bool(value) => {
                value.encode(writer)?;
            }
            Self::String(value) => {
                value.encode(writer)?;
            }
            Self::Case3(value) => {
                writer.write_len_var_u32(value.len())?;
                for item in value.iter() {
                    item.encode(writer)?;
                }
            }
        }
        Ok(())
    }
}

impl wire::Decode for ServerboundPackSettingChangePackSettingValue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::Float(<wire::F32LE as wire::Decode>::decode(reader)?),
            1 => Self::Bool(<bool as wire::Decode>::decode(reader)?),
            2 => Self::String(reader.read_str(
                crate::fields::SERVERBOUND_PACK_SETTING_CHANGE_PACK_SETTING_VALUE_STRING,
            )?),
            3 => Self::Case3({
                let declared = u64::from(reader.read_var_u32()?);
                let count = reader.checked_count(
                    crate::fields::SERVERBOUND_PACK_SETTING_CHANGE_PACK_SETTING_VALUE_CASE3,
                    declared,
                    1,
                )?;
                let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
                for _ in 0..count {
                    wire::push(&mut out, reader.read_str(crate::fields::SERVERBOUND_PACK_SETTING_CHANGE_PACK_SETTING_VALUE_CASE3_ITEM)?)?;
                }
                out
            }),
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "ServerboundPackSettingChangePackSettingValue",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SetScoreInfoItem {
    RemoveScore {
        action: wire::Str,
        scoreboard_id: ScoreboardId,
        /// Wire presence: optional value is preceded by a presence marker.
        objective_name: Option<wire::Str>,
    },
    ChangePlayerScore {
        action: wire::Str,
        scoreboard_id: ScoreboardId,
        objective_name: wire::Str,
        score_value: wire::I32LE,
        player_unique_id: PlayerScoreboardId,
    },
    ChangeEntityScore {
        action: wire::Str,
        scoreboard_id: ScoreboardId,
        objective_name: wire::Str,
        score_value: wire::I32LE,
        actor_id: ActorUniqueID,
    },
    ChangeFakePlayerScore {
        action: wire::Str,
        scoreboard_id: ScoreboardId,
        objective_name: wire::Str,
        score_value: wire::I32LE,
        fake_player_name: wire::Str,
    },
}

impl SetScoreInfoItem {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::RemoveScore { .. } => 0,
            Self::ChangePlayerScore { .. } => 1,
            Self::ChangeEntityScore { .. } => 2,
            Self::ChangeFakePlayerScore { .. } => 3,
        }
    }
}

impl Default for SetScoreInfoItem {
    fn default() -> Self {
        Self::RemoveScore {
            action: Default::default(),
            scoreboard_id: Default::default(),
            objective_name: Default::default(),
        }
    }
}

impl wire::Encode for SetScoreInfoItem {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::RemoveScore {
                action,
                scoreboard_id,
                objective_name,
            } => {
                action.encode(writer)?;
                scoreboard_id.encode(writer)?;
                match &objective_name {
                    Some(value) => {
                        writer.write_u8(1)?;
                        value.encode(writer)?;
                    }
                    None => writer.write_u8(0)?,
                }
            }
            Self::ChangePlayerScore {
                action,
                scoreboard_id,
                objective_name,
                score_value,
                player_unique_id,
            } => {
                action.encode(writer)?;
                scoreboard_id.encode(writer)?;
                objective_name.encode(writer)?;
                score_value.encode(writer)?;
                player_unique_id.encode(writer)?;
            }
            Self::ChangeEntityScore {
                action,
                scoreboard_id,
                objective_name,
                score_value,
                actor_id,
            } => {
                action.encode(writer)?;
                scoreboard_id.encode(writer)?;
                objective_name.encode(writer)?;
                score_value.encode(writer)?;
                actor_id.encode(writer)?;
            }
            Self::ChangeFakePlayerScore {
                action,
                scoreboard_id,
                objective_name,
                score_value,
                fake_player_name,
            } => {
                action.encode(writer)?;
                scoreboard_id.encode(writer)?;
                objective_name.encode(writer)?;
                score_value.encode(writer)?;
                fake_player_name.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for SetScoreInfoItem {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let action =
                    reader.read_str(crate::fields::SET_SCORE_INFO_ITEM_REMOVE_SCORE_ACTION)?;
                let scoreboard_id = <ScoreboardId as wire::Decode>::decode(reader)?;
                let objective_name = {
                    if reader.read_u8()? == 0 {
                        None
                    } else {
                        Some(reader.read_str(
                            crate::fields::SET_SCORE_INFO_ITEM_REMOVE_SCORE_OBJECTIVE_NAME,
                        )?)
                    }
                };
                Self::RemoveScore {
                    action,
                    scoreboard_id,
                    objective_name,
                }
            }
            1 => {
                let action = reader
                    .read_str(crate::fields::SET_SCORE_INFO_ITEM_CHANGE_PLAYER_SCORE_ACTION)?;
                let scoreboard_id = <ScoreboardId as wire::Decode>::decode(reader)?;
                let objective_name = reader.read_str(
                    crate::fields::SET_SCORE_INFO_ITEM_CHANGE_PLAYER_SCORE_OBJECTIVE_NAME,
                )?;
                let score_value = <wire::I32LE as wire::Decode>::decode(reader)?;
                let player_unique_id = <PlayerScoreboardId as wire::Decode>::decode(reader)?;
                Self::ChangePlayerScore {
                    action,
                    scoreboard_id,
                    objective_name,
                    score_value,
                    player_unique_id,
                }
            }
            2 => {
                let action = reader
                    .read_str(crate::fields::SET_SCORE_INFO_ITEM_CHANGE_ENTITY_SCORE_ACTION)?;
                let scoreboard_id = <ScoreboardId as wire::Decode>::decode(reader)?;
                let objective_name = reader.read_str(
                    crate::fields::SET_SCORE_INFO_ITEM_CHANGE_ENTITY_SCORE_OBJECTIVE_NAME,
                )?;
                let score_value = <wire::I32LE as wire::Decode>::decode(reader)?;
                let actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
                Self::ChangeEntityScore {
                    action,
                    scoreboard_id,
                    objective_name,
                    score_value,
                    actor_id,
                }
            }
            3 => {
                let action = reader
                    .read_str(crate::fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_ACTION)?;
                let scoreboard_id = <ScoreboardId as wire::Decode>::decode(reader)?;
                let objective_name = reader.read_str(
                    crate::fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_OBJECTIVE_NAME,
                )?;
                let score_value = <wire::I32LE as wire::Decode>::decode(reader)?;
                let fake_player_name = reader.read_str(
                    crate::fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_FAKE_PLAYER_NAME,
                )?;
                Self::ChangeFakePlayerScore {
                    action,
                    scoreboard_id,
                    objective_name,
                    score_value,
                    fake_player_name,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "SetScoreInfoItem",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: inventory

/// InventoryAction represents a single action that took place during an inventory transaction. On itself,
/// this inventory action is always unbalanced: It must be combined with other actions in an inventory
/// transaction to form a balanced transaction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryAction {
    pub source: InventorySource,
    /// InventorySlot is the slot in which the action took place. Each action only describes the change of item in
    /// a single slot.
    pub slot: wire::VarUInt,
    pub from_item: NetworkItemStackDescriptorSerializedData,
    pub to_item: NetworkItemStackDescriptorSerializedData,
}

impl wire::Encode for InventoryAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.source.encode(writer)?;
        self.slot.encode(writer)?;
        wire::check_number_limits(self.slot.0, Some(0), None)?;
        self.from_item.encode(writer)?;
        self.to_item.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let source = <InventorySource as wire::Decode>::decode(reader)?;
        let slot = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let from_item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let to_item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        Ok(Self {
            source,
            slot,
            from_item,
            to_item,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryOptions {
    /// `left_inventory_tab` is the tab that is selected on the left side of the inventory. This is usually for
    /// the creative inventory. It is one of the InventoryLeftTab constants above.
    pub left_inventory_tab: InventoryLeftTabIndex,
    /// `right_inventory_tab` is the tab that is selected on the right side of the inventory. This is usually for
    /// the player's own inventory. It is one of the InventoryRightTab constants above.
    pub right_inventory_tab: InventoryRightTabIndex,
    /// `filtering` is whether the player has enabled the filtering between recipes they have unlocked or not.
    pub filtering: bool,
    /// InventoryLayout is the layout of the inventory. It is one of the InventoryLayout constants above.
    pub layout_inv: InventoryLayout,
    /// CraftingLayout is the layout of the crafting inventory. It is one of the InventoryLayout constants above.
    pub layout_craft: InventoryLayout,
}

impl wire::Encode for InventoryOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.left_inventory_tab.encode(writer)?;
        self.right_inventory_tab.encode(writer)?;
        self.filtering.encode(writer)?;
        self.layout_inv.encode(writer)?;
        self.layout_craft.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let left_inventory_tab = <InventoryLeftTabIndex as wire::Decode>::decode(reader)?;
        let right_inventory_tab = <InventoryRightTabIndex as wire::Decode>::decode(reader)?;
        let filtering = <bool as wire::Decode>::decode(reader)?;
        let layout_inv = <InventoryLayout as wire::Decode>::decode(reader)?;
        let layout_craft = <InventoryLayout as wire::Decode>::decode(reader)?;
        Ok(Self {
            left_inventory_tab,
            right_inventory_tab,
            filtering,
            layout_inv,
            layout_craft,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventorySource {
    pub source_type: InventorySourceType,
    /// Wire presence: optional value is preceded by a presence marker.
    pub container_id: Option<wire::I8>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub bit_flags: Option<InventorySourceInventorySourceFlags>,
}

impl wire::Encode for InventorySource {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.source_type.encode(writer)?;
        match &self.container_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.bit_flags {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for InventorySource {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let source_type = <InventorySourceType as wire::Decode>::decode(reader)?;
        let container_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I8 as wire::Decode>::decode(reader)?)
            }
        };
        let bit_flags = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<InventorySourceInventorySourceFlags as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            source_type,
            container_id,
            bit_flags,
        })
    }
}

/// InventoryTransactionData represents an object that holds data specific to an inventory transaction type.
/// The data it holds depends on the type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryTransactionData {
    pub actions: Vec<InventoryAction>,
}

impl wire::Encode for InventoryTransactionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.actions.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for InventoryTransactionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actions = wire::decode_collection::<InventoryAction>(
            reader,
            crate::fields::INVENTORY_TRANSACTION_DATA_ACTIONS,
            20,
        )?;
        Ok(Self { actions })
    }
}

// Domain: item

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemData {
    pub item_name: wire::Str,
    pub item_id: wire::I16LE,
    pub is_component_based: bool,
    pub item_version: ItemVersion,
    pub item_component_data: wire::NetworkNbt,
}

impl wire::Encode for ItemData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_name.encode(writer)?;
        self.item_id.encode(writer)?;
        self.is_component_based.encode(writer)?;
        self.item_version.encode(writer)?;
        self.item_component_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_name = reader.read_str(crate::fields::ITEM_DATA_ITEM_NAME)?;
        let item_id = <wire::I16LE as wire::Decode>::decode(reader)?;
        let is_component_based = <bool as wire::Decode>::decode(reader)?;
        let item_version = <ItemVersion as wire::Decode>::decode(reader)?;
        let item_component_data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_name,
            item_id,
            is_component_based,
            item_version,
            item_component_data,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemEnchantOption {
    pub cost: wire::U8,
    pub enchants: ItemEnchants,
    pub enchant_name: wire::Str,
    pub enchant_net_id: RecipeNetID,
}

impl wire::Encode for ItemEnchantOption {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.cost.encode(writer)?;
        wire::check_number_limits(self.cost.0, Some(0), Some(255))?;
        self.enchants.encode(writer)?;
        wire::encode_string_limits(writer, &self.enchant_name, 1, 256)?;
        self.enchant_net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemEnchantOption {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cost = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let enchants = <ItemEnchants as wire::Decode>::decode(reader)?;
        let enchant_name = wire::decode_string_limits(
            reader,
            crate::fields::ITEM_ENCHANT_OPTION_ENCHANT_NAME,
            1,
            256,
        )?;
        let enchant_net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            cost,
            enchants,
            enchant_name,
            enchant_net_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemEnchants {
    pub slot: wire::I32LE,
    pub item_enchants: [Vec<EnchantmentInstance>; 3],
}

impl wire::Encode for ItemEnchants {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.slot.encode(writer)?;
        for item in self.item_enchants.iter() {
            wire::encode_collection(writer, item.as_slice())?;
        }
        Ok(())
    }
}

impl wire::Decode for ItemEnchants {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let slot = <wire::I32LE as wire::Decode>::decode(reader)?;
        let item_enchants = [
            wire::decode_collection::<EnchantmentInstance>(
                reader,
                crate::fields::ITEM_ENCHANTS_ITEM_ENCHANTS_ITEM,
                2,
            )?,
            wire::decode_collection::<EnchantmentInstance>(
                reader,
                crate::fields::ITEM_ENCHANTS_ITEM_ENCHANTS_ITEM,
                2,
            )?,
            wire::decode_collection::<EnchantmentInstance>(
                reader,
                crate::fields::ITEM_ENCHANTS_ITEM_ENCHANTS_ITEM,
                2,
            )?,
        ];
        Ok(Self {
            slot,
            item_enchants,
        })
    }
}

/// ItemInstance represents a unique instance of an item stack. These instances carry a specific network ID
/// that is persistent for the stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemInstance {
    pub item_descriptor: ItemDescriptor,
    pub stack_size: wire::U16LE,
    pub block_runtime_id: wire::VarUInt,
    pub user_data_buffer: bytes::Bytes,
}

impl wire::Encode for ItemInstance {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_descriptor.encode(writer)?;
        self.stack_size.encode(writer)?;
        wire::check_number_limits(self.stack_size.0, Some(1), Some(64))?;
        self.block_runtime_id.encode(writer)?;
        self.user_data_buffer.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemInstance {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_descriptor = <ItemDescriptor as wire::Decode>::decode(reader)?;
        let stack_size = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(1), Some(64))?;
            value
        };
        let block_runtime_id = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let user_data_buffer =
            reader.read_shared_bytes(crate::fields::ITEM_INSTANCE_USER_DATA_BUFFER)?;
        Ok(Self {
            item_descriptor,
            stack_size,
            block_runtime_id,
            user_data_buffer,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemUseInventoryTransaction {
    pub actions: InventoryTransactionData,
    pub action_type: ItemUseInventoryTransactionActionType,
    pub trigger_type: ItemUseInventoryTransactionTriggerType,
    pub position: BlockPos,
    pub face: wire::U8,
    pub slot: wire::ZigZag32,
    pub hand: HandSlot,
    pub item: NetworkItemStackDescriptorSerializedData,
    pub from_position: glam::Vec3,
    pub click_position: glam::Vec3,
    pub target_block_id: wire::VarUInt,
    pub client_interact_prediction: ItemUseInventoryTransactionPredictedResult,
    pub client_cooldown_state: ItemUseInventoryTransactionClientCooldownState,
}

impl wire::Encode for ItemUseInventoryTransaction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actions.encode(writer)?;
        self.action_type.encode(writer)?;
        self.trigger_type.encode(writer)?;
        self.position.encode(writer)?;
        self.face.encode(writer)?;
        wire::check_number_limits(self.face.0, Some(0), Some(255))?;
        self.slot.encode(writer)?;
        self.hand.encode(writer)?;
        self.item.encode(writer)?;
        self.from_position.encode(writer)?;
        self.click_position.encode(writer)?;
        self.target_block_id.encode(writer)?;
        wire::check_number_limits(self.target_block_id.0, Some(0), None)?;
        self.client_interact_prediction.encode(writer)?;
        self.client_cooldown_state.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemUseInventoryTransaction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actions = <InventoryTransactionData as wire::Decode>::decode(reader)?;
        let action_type = <ItemUseInventoryTransactionActionType as wire::Decode>::decode(reader)?;
        let trigger_type =
            <ItemUseInventoryTransactionTriggerType as wire::Decode>::decode(reader)?;
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let face = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let slot = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let hand = <HandSlot as wire::Decode>::decode(reader)?;
        let item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let from_position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let click_position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let target_block_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let client_interact_prediction =
            <ItemUseInventoryTransactionPredictedResult as wire::Decode>::decode(reader)?;
        let client_cooldown_state =
            <ItemUseInventoryTransactionClientCooldownState as wire::Decode>::decode(reader)?;
        Ok(Self {
            actions,
            action_type,
            trigger_type,
            position,
            face,
            slot,
            hand,
            item,
            from_position,
            click_position,
            target_block_id,
            client_interact_prediction,
            client_cooldown_state,
        })
    }
}

// Domain: item_stack

#[derive(Clone, Debug, PartialEq)]
pub enum ItemDescriptor {
    EmptyItemDescriptorData {
        descriptor_type: ItemDescriptorType,
    },
    ItemNameDescriptorData {
        descriptor_type: ItemDescriptorType,
        /// Name is the identifier of the item, such as minecraft:stone.
        full_name: wire::Str,
        aux_value: wire::ZigZag32,
    },
    MolangItemDescriptorData {
        descriptor_type: ItemDescriptorType,
        /// Expression represents the MoLang expression used to identify the item/it's associated tag.
        tag_expression: wire::Str,
        /// Version represents the version of MoLang to use.
        molang_version: MoLangVersion,
    },
    ItemTagDescriptorData {
        descriptor_type: ItemDescriptorType,
        item_tag: wire::Str,
    },
}

impl ItemDescriptor {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::EmptyItemDescriptorData { .. } => 0,
            Self::ItemNameDescriptorData { .. } => 1,
            Self::MolangItemDescriptorData { .. } => 2,
            Self::ItemTagDescriptorData { .. } => 3,
        }
    }
}

impl Default for ItemDescriptor {
    fn default() -> Self {
        Self::EmptyItemDescriptorData {
            descriptor_type: Default::default(),
        }
    }
}

impl wire::Encode for ItemDescriptor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::EmptyItemDescriptorData { descriptor_type } => {
                descriptor_type.encode(writer)?;
            }
            Self::ItemNameDescriptorData {
                descriptor_type,
                full_name,
                aux_value,
            } => {
                descriptor_type.encode(writer)?;
                wire::encode_string_limits(writer, full_name, 1, 18446744073709551615)?;
                aux_value.encode(writer)?;
                wire::check_number_limits(aux_value.0, Some(0), Some(32767))?;
            }
            Self::MolangItemDescriptorData {
                descriptor_type,
                tag_expression,
                molang_version,
            } => {
                descriptor_type.encode(writer)?;
                wire::encode_string_limits(writer, tag_expression, 1, 18446744073709551615)?;
                molang_version.encode(writer)?;
            }
            Self::ItemTagDescriptorData {
                descriptor_type,
                item_tag,
            } => {
                descriptor_type.encode(writer)?;
                wire::encode_string_limits(writer, item_tag, 1, 18446744073709551615)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for ItemDescriptor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let descriptor_type = <ItemDescriptorType as wire::Decode>::decode(reader)?;
                Self::EmptyItemDescriptorData { descriptor_type }
            }
            1 => {
                let descriptor_type = <ItemDescriptorType as wire::Decode>::decode(reader)?;
                let full_name = wire::decode_string_limits(
                    reader,
                    crate::fields::ITEM_DESCRIPTOR_ITEM_NAME_DESCRIPTOR_DATA_FULL_NAME,
                    1,
                    18446744073709551615,
                )?;
                let aux_value = {
                    let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(32767))?;
                    value
                };
                Self::ItemNameDescriptorData {
                    descriptor_type,
                    full_name,
                    aux_value,
                }
            }
            2 => {
                let descriptor_type = <ItemDescriptorType as wire::Decode>::decode(reader)?;
                let tag_expression = wire::decode_string_limits(
                    reader,
                    crate::fields::ITEM_DESCRIPTOR_MOLANG_ITEM_DESCRIPTOR_DATA_TAG_EXPRESSION,
                    1,
                    18446744073709551615,
                )?;
                let molang_version = <MoLangVersion as wire::Decode>::decode(reader)?;
                Self::MolangItemDescriptorData {
                    descriptor_type,
                    tag_expression,
                    molang_version,
                }
            }
            3 => {
                let descriptor_type = <ItemDescriptorType as wire::Decode>::decode(reader)?;
                let item_tag = wire::decode_string_limits(
                    reader,
                    crate::fields::ITEM_DESCRIPTOR_ITEM_TAG_DESCRIPTOR_DATA_ITEM_TAG,
                    1,
                    18446744073709551615,
                )?;
                Self::ItemTagDescriptorData {
                    descriptor_type,
                    item_tag,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "ItemDescriptor",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemStackLegacyRequestID(pub i32);

impl wire::Encode for ItemStackLegacyRequestID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackLegacyRequestID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag32 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemStackNetID(pub i32);

impl wire::Encode for ItemStackNetID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackNetID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag32 as wire::Decode>::decode(reader)?.0))
    }
}

/// ItemStackRequest represents a single request present in an ItemStackRequest packet sent by the client to
/// change an item in an inventory. Item stack requests are either approved or rejected by the server using
/// the ItemStackResponse packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackRequestData {
    pub client_request_id: ItemStackRequestID,
    /// `actions` is a list of actions performed by the client. The actual type of the actions depends on which ID
    /// was present, and is one of the concrete types below.
    pub actions: Vec<StackRequestAction>,
    pub strings_to_filter: Vec<wire::Str>,
    pub strings_to_filter_origin: TextProcessingEventOrigin,
}

impl wire::Encode for ItemStackRequestData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.client_request_id.encode(writer)?;
        wire::encode_collection_limits(writer, self.actions.as_slice(), 1, 100)?;
        writer.write_len_var_u32(self.strings_to_filter.len())?;
        for item in self.strings_to_filter.iter() {
            wire::encode_string_limits(writer, item, 0, 1000)?;
        }
        self.strings_to_filter_origin.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequestData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let client_request_id = <ItemStackRequestID as wire::Decode>::decode(reader)?;
        let actions = wire::decode_collection_limits::<StackRequestAction>(
            reader,
            crate::fields::ITEM_STACK_REQUEST_DATA_ACTIONS,
            2,
            1,
            100,
        )?;
        let strings_to_filter = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::ITEM_STACK_REQUEST_DATA_STRINGS_TO_FILTER,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    wire::decode_string_limits(
                        reader,
                        crate::fields::ITEM_STACK_REQUEST_DATA_STRINGS_TO_FILTER_ITEM,
                        0,
                        1000,
                    )?,
                )?;
            }
            out
        };
        let strings_to_filter_origin = <TextProcessingEventOrigin as wire::Decode>::decode(reader)?;
        Ok(Self {
            client_request_id,
            actions,
            strings_to_filter,
            strings_to_filter_origin,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemStackRequestID(pub i32);

impl wire::Encode for ItemStackRequestID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequestID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag32 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackRequestPacketData {
    pub client_request_id: ItemStackRequestID,
    pub actions: Vec<StackRequestAction>,
    pub strings_to_filter: Vec<wire::Str>,
    pub strings_to_filter_origin: TextProcessingEventOrigin,
}

impl wire::Encode for ItemStackRequestPacketData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.client_request_id.encode(writer)?;
        wire::encode_collection_limits(writer, self.actions.as_slice(), 1, 100)?;
        writer.write_len_var_u32(self.strings_to_filter.len())?;
        for item in self.strings_to_filter.iter() {
            wire::encode_string_limits(writer, item, 0, 1000)?;
        }
        self.strings_to_filter_origin.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequestPacketData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let client_request_id = <ItemStackRequestID as wire::Decode>::decode(reader)?;
        let actions = wire::decode_collection_limits::<StackRequestAction>(
            reader,
            crate::fields::ITEM_STACK_REQUEST_PACKET_DATA_ACTIONS,
            2,
            1,
            100,
        )?;
        let strings_to_filter = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::ITEM_STACK_REQUEST_PACKET_DATA_STRINGS_TO_FILTER,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    wire::decode_string_limits(
                        reader,
                        crate::fields::ITEM_STACK_REQUEST_PACKET_DATA_STRINGS_TO_FILTER_ITEM,
                        0,
                        1000,
                    )?,
                )?;
            }
            out
        };
        let strings_to_filter_origin = <TextProcessingEventOrigin as wire::Decode>::decode(reader)?;
        Ok(Self {
            client_request_id,
            actions,
            strings_to_filter,
            strings_to_filter_origin,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackResponseContainerInfo {
    pub full_container_name: FullContainerName,
    pub slots: Vec<ItemStackResponseSlotInfo>,
}

impl wire::Encode for ItemStackResponseContainerInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.full_container_name.encode(writer)?;
        wire::encode_collection(writer, self.slots.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for ItemStackResponseContainerInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let full_container_name = <FullContainerName as wire::Decode>::decode(reader)?;
        let slots = wire::decode_collection::<ItemStackResponseSlotInfo>(
            reader,
            crate::fields::ITEM_STACK_RESPONSE_CONTAINER_INFO_SLOTS,
            7,
        )?;
        Ok(Self {
            full_container_name,
            slots,
        })
    }
}

/// ItemStackResponse is a response to an individual ItemStackRequest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackResponseInfo {
    pub result: ItemStackNetResult,
    pub client_request_id: ItemStackRequestID,
    /// Wire presence: optional value is preceded by a presence marker.
    pub containers: Option<Vec<ItemStackResponseContainerInfo>>,
}

impl wire::Encode for ItemStackResponseInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.result.encode(writer)?;
        self.client_request_id.encode(writer)?;
        match &self.containers {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection(writer, value.as_slice())?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ItemStackResponseInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let result = <ItemStackNetResult as wire::Decode>::decode(reader)?;
        let client_request_id = <ItemStackRequestID as wire::Decode>::decode(reader)?;
        let containers = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection::<ItemStackResponseContainerInfo>(
                    reader,
                    crate::fields::ITEM_STACK_RESPONSE_INFO_CONTAINERS,
                    3,
                )?)
            }
        };
        Ok(Self {
            result,
            client_request_id,
            containers,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackResponseSlotInfo {
    pub requested_slot: wire::U8,
    pub slot: wire::U8,
    pub amount: wire::U8,
    /// Wire presence: optional value is preceded by a presence marker.
    pub item_stack_net_id: Option<ItemStackNetID>,
    pub custom_name: BedrockSafetyRedactableString,
    pub durability_correction: wire::ZigZag32,
}

impl wire::Encode for ItemStackResponseSlotInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.requested_slot.encode(writer)?;
        self.slot.encode(writer)?;
        self.amount.encode(writer)?;
        match &self.item_stack_net_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.custom_name.encode(writer)?;
        self.durability_correction.encode(writer)?;
        wire::check_number_limits(self.durability_correction.0, Some(-32768), Some(32767))?;
        Ok(())
    }
}

impl wire::Decode for ItemStackResponseSlotInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let requested_slot = <wire::U8 as wire::Decode>::decode(reader)?;
        let slot = <wire::U8 as wire::Decode>::decode(reader)?;
        let amount = <wire::U8 as wire::Decode>::decode(reader)?;
        let item_stack_net_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ItemStackNetID as wire::Decode>::decode(reader)?)
            }
        };
        let custom_name = <BedrockSafetyRedactableString as wire::Decode>::decode(reader)?;
        let durability_correction = {
            let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(-32768), Some(32767))?;
            value
        };
        Ok(Self {
            requested_slot,
            slot,
            amount,
            item_stack_net_id,
            custom_name,
            durability_correction,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StackRequestAction {
    TakeActionData {
        action_type: ItemStackRequestActionType,
        amount: wire::U8,
        source: StackRequestSlotInfo,
        destination: StackRequestSlotInfo,
    },
    PlaceActionData {
        action_type: ItemStackRequestActionType,
        amount: wire::U8,
        source: StackRequestSlotInfo,
        destination: StackRequestSlotInfo,
    },
    SwapActionData {
        action_type: ItemStackRequestActionType,
        /// `source` and Destination point to the source slot from which Count of the item stack were taken and the
        /// destination slot to which this item was moved.
        source: StackRequestSlotInfo,
        /// Source and Destination point to the source slot from which Count of the item stack were taken and the
        /// destination slot to which this item was moved.
        destination: StackRequestSlotInfo,
    },
    DropActionData {
        action_type: ItemStackRequestActionType,
        amount: wire::U8,
        /// `source` is the source slot from which items were dropped to the ground.
        source: StackRequestSlotInfo,
        /// `randomly` seems to be set to false in most cases. I'm not entirely sure what this does, but this is what
        /// vanilla calls this field.
        randomly: bool,
    },
    DestroyActionData {
        action_type: ItemStackRequestActionType,
        amount: wire::U8,
        /// `source` is the source slot from which items came that were destroyed by moving them into the creative
        /// inventory.
        source: StackRequestSlotInfo,
    },
    ConsumeActionData {
        action_type: ItemStackRequestActionType,
        amount: wire::U8,
        source: StackRequestSlotInfo,
    },
    CreateActionData {
        action_type: ItemStackRequestActionType,
        results_index: wire::U8,
    },
    LabTableCombineActionData {
        action_type: ItemStackRequestActionType,
    },
    BeaconPaymentActionData {
        action_type: ItemStackRequestActionType,
        /// PrimaryEffect and SecondaryEffect are the effects that were selected from the beacon.
        primary_effect_id: wire::ZigZag32,
        /// PrimaryEffect and SecondaryEffect are the effects that were selected from the beacon.
        secondary_effect_id: wire::ZigZag32,
    },
    MineBlockActionData {
        action_type: ItemStackRequestActionType,
        /// HotbarSlot is the slot held by the player while mining a block.
        slot: wire::ZigZag32,
        /// `predicted_durability` is the durability of the item that the client assumes to be present at the time.
        predicted_durability: wire::ZigZag32,
        net_id_variant: wire::I32LE,
    },
    CraftRecipeActionData {
        action_type: ItemStackRequestActionType,
        recipe_net_id: RecipeNetID,
        number_of_requested_crafts: wire::U8,
    },
    CraftRecipeAutoActionData {
        action_type: ItemStackRequestActionType,
        recipe_net_id: RecipeNetID,
        number_of_requested_crafts: wire::U8,
        /// `ingredients` is a slice of ItemDescriptorCount that contains the ingredients that were used to craft the
        /// recipe. It is not exactly clear what this is used for, but it is sent by the vanilla client.
        ingredients: Vec<RecipeIngredient>,
    },
    CraftCreativeActionData {
        action_type: ItemStackRequestActionType,
        creative_item_net_id: wire::VarUInt,
        number_of_requested_crafts: wire::U8,
    },
    CraftRecipeOptionalActionData {
        action_type: ItemStackRequestActionType,
        recipe_net_id: RecipeNetID,
        filtered_string_index: wire::I32LE,
    },
    CraftRepairAndDisenchantActionData {
        action_type: ItemStackRequestActionType,
        recipe_net_id: wire::I32LE,
        number_of_requested_crafts: wire::U8,
        repair_cost: wire::ZigZag32,
    },
    CraftLoomActionData {
        action_type: ItemStackRequestActionType,
        pattern_name_id: wire::Str,
        num_crafts: wire::U8,
    },
    CraftNonImplementedActionData {
        action_type: ItemStackRequestActionType,
    },
    CraftResultsActionData {
        action_type: ItemStackRequestActionType,
        craft_results: Vec<ItemInstance>,
        num_crafts: wire::U8,
    },
}

impl StackRequestAction {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::TakeActionData { .. } => 0,
            Self::PlaceActionData { .. } => 1,
            Self::SwapActionData { .. } => 2,
            Self::DropActionData { .. } => 3,
            Self::DestroyActionData { .. } => 4,
            Self::ConsumeActionData { .. } => 5,
            Self::CreateActionData { .. } => 6,
            Self::LabTableCombineActionData { .. } => 7,
            Self::BeaconPaymentActionData { .. } => 8,
            Self::MineBlockActionData { .. } => 9,
            Self::CraftRecipeActionData { .. } => 10,
            Self::CraftRecipeAutoActionData { .. } => 11,
            Self::CraftCreativeActionData { .. } => 12,
            Self::CraftRecipeOptionalActionData { .. } => 13,
            Self::CraftRepairAndDisenchantActionData { .. } => 14,
            Self::CraftLoomActionData { .. } => 15,
            Self::CraftNonImplementedActionData { .. } => 16,
            Self::CraftResultsActionData { .. } => 17,
        }
    }
}

impl Default for StackRequestAction {
    fn default() -> Self {
        Self::TakeActionData {
            action_type: Default::default(),
            amount: Default::default(),
            source: Default::default(),
            destination: Default::default(),
        }
    }
}

impl wire::Encode for StackRequestAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::TakeActionData {
                action_type,
                amount,
                source,
                destination,
            } => {
                action_type.encode(writer)?;
                amount.encode(writer)?;
                wire::check_number_limits(amount.0, Some(1), Some(64))?;
                source.encode(writer)?;
                destination.encode(writer)?;
            }
            Self::PlaceActionData {
                action_type,
                amount,
                source,
                destination,
            } => {
                action_type.encode(writer)?;
                amount.encode(writer)?;
                wire::check_number_limits(amount.0, Some(1), Some(64))?;
                source.encode(writer)?;
                destination.encode(writer)?;
            }
            Self::SwapActionData {
                action_type,
                source,
                destination,
            } => {
                action_type.encode(writer)?;
                source.encode(writer)?;
                destination.encode(writer)?;
            }
            Self::DropActionData {
                action_type,
                amount,
                source,
                randomly,
            } => {
                action_type.encode(writer)?;
                amount.encode(writer)?;
                wire::check_number_limits(amount.0, Some(1), Some(64))?;
                source.encode(writer)?;
                randomly.encode(writer)?;
            }
            Self::DestroyActionData {
                action_type,
                amount,
                source,
            } => {
                action_type.encode(writer)?;
                amount.encode(writer)?;
                wire::check_number_limits(amount.0, Some(1), Some(64))?;
                source.encode(writer)?;
            }
            Self::ConsumeActionData {
                action_type,
                amount,
                source,
            } => {
                action_type.encode(writer)?;
                amount.encode(writer)?;
                wire::check_number_limits(amount.0, Some(1), Some(64))?;
                source.encode(writer)?;
            }
            Self::CreateActionData {
                action_type,
                results_index,
            } => {
                action_type.encode(writer)?;
                results_index.encode(writer)?;
            }
            Self::LabTableCombineActionData { action_type } => {
                action_type.encode(writer)?;
            }
            Self::BeaconPaymentActionData {
                action_type,
                primary_effect_id,
                secondary_effect_id,
            } => {
                action_type.encode(writer)?;
                primary_effect_id.encode(writer)?;
                wire::check_number_limits(primary_effect_id.0, Some(0), Some(37))?;
                secondary_effect_id.encode(writer)?;
                wire::check_number_limits(secondary_effect_id.0, Some(0), Some(37))?;
            }
            Self::MineBlockActionData {
                action_type,
                slot,
                predicted_durability,
                net_id_variant,
            } => {
                action_type.encode(writer)?;
                slot.encode(writer)?;
                predicted_durability.encode(writer)?;
                net_id_variant.encode(writer)?;
            }
            Self::CraftRecipeActionData {
                action_type,
                recipe_net_id,
                number_of_requested_crafts,
            } => {
                action_type.encode(writer)?;
                recipe_net_id.encode(writer)?;
                number_of_requested_crafts.encode(writer)?;
                wire::check_number_limits(number_of_requested_crafts.0, Some(1), None)?;
            }
            Self::CraftRecipeAutoActionData {
                action_type,
                recipe_net_id,
                number_of_requested_crafts,
                ingredients,
            } => {
                action_type.encode(writer)?;
                recipe_net_id.encode(writer)?;
                number_of_requested_crafts.encode(writer)?;
                wire::check_number_limits(number_of_requested_crafts.0, Some(1), None)?;
                wire::encode_collection(writer, ingredients.as_slice())?;
            }
            Self::CraftCreativeActionData {
                action_type,
                creative_item_net_id,
                number_of_requested_crafts,
            } => {
                action_type.encode(writer)?;
                creative_item_net_id.encode(writer)?;
                wire::check_number_limits(creative_item_net_id.0, Some(1), None)?;
                number_of_requested_crafts.encode(writer)?;
                wire::check_number_limits(number_of_requested_crafts.0, Some(1), None)?;
            }
            Self::CraftRecipeOptionalActionData {
                action_type,
                recipe_net_id,
                filtered_string_index,
            } => {
                action_type.encode(writer)?;
                recipe_net_id.encode(writer)?;
                filtered_string_index.encode(writer)?;
            }
            Self::CraftRepairAndDisenchantActionData {
                action_type,
                recipe_net_id,
                number_of_requested_crafts,
                repair_cost,
            } => {
                action_type.encode(writer)?;
                recipe_net_id.encode(writer)?;
                number_of_requested_crafts.encode(writer)?;
                wire::check_number_limits(number_of_requested_crafts.0, Some(1), None)?;
                repair_cost.encode(writer)?;
                wire::check_number_limits(repair_cost.0, Some(0), None)?;
            }
            Self::CraftLoomActionData {
                action_type,
                pattern_name_id,
                num_crafts,
            } => {
                action_type.encode(writer)?;
                pattern_name_id.encode(writer)?;
                num_crafts.encode(writer)?;
                wire::check_number_limits(num_crafts.0, Some(1), None)?;
            }
            Self::CraftNonImplementedActionData { action_type } => {
                action_type.encode(writer)?;
            }
            Self::CraftResultsActionData {
                action_type,
                craft_results,
                num_crafts,
            } => {
                action_type.encode(writer)?;
                wire::encode_collection_limits(
                    writer,
                    craft_results.as_slice(),
                    1,
                    18446744073709551615,
                )?;
                num_crafts.encode(writer)?;
                wire::check_number_limits(num_crafts.0, Some(1), None)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for StackRequestAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let amount = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), Some(64))?;
                    value
                };
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                let destination = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                Self::TakeActionData {
                    action_type,
                    amount,
                    source,
                    destination,
                }
            }
            1 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let amount = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), Some(64))?;
                    value
                };
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                let destination = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                Self::PlaceActionData {
                    action_type,
                    amount,
                    source,
                    destination,
                }
            }
            2 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                let destination = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                Self::SwapActionData {
                    action_type,
                    source,
                    destination,
                }
            }
            3 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let amount = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), Some(64))?;
                    value
                };
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                let randomly = <bool as wire::Decode>::decode(reader)?;
                Self::DropActionData {
                    action_type,
                    amount,
                    source,
                    randomly,
                }
            }
            4 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let amount = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), Some(64))?;
                    value
                };
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                Self::DestroyActionData {
                    action_type,
                    amount,
                    source,
                }
            }
            5 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let amount = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), Some(64))?;
                    value
                };
                let source = <StackRequestSlotInfo as wire::Decode>::decode(reader)?;
                Self::ConsumeActionData {
                    action_type,
                    amount,
                    source,
                }
            }
            6 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let results_index = <wire::U8 as wire::Decode>::decode(reader)?;
                Self::CreateActionData {
                    action_type,
                    results_index,
                }
            }
            7 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                Self::LabTableCombineActionData { action_type }
            }
            8 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let primary_effect_id = {
                    let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(37))?;
                    value
                };
                let secondary_effect_id = {
                    let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(37))?;
                    value
                };
                Self::BeaconPaymentActionData {
                    action_type,
                    primary_effect_id,
                    secondary_effect_id,
                }
            }
            9 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let slot = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let predicted_durability = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                let net_id_variant = <wire::I32LE as wire::Decode>::decode(reader)?;
                Self::MineBlockActionData {
                    action_type,
                    slot,
                    predicted_durability,
                    net_id_variant,
                }
            }
            10 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let recipe_net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
                let number_of_requested_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                Self::CraftRecipeActionData {
                    action_type,
                    recipe_net_id,
                    number_of_requested_crafts,
                }
            }
            11 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let recipe_net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
                let number_of_requested_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                let ingredients = wire::decode_collection::<RecipeIngredient>(
                    reader,
                    crate::fields::STACK_REQUEST_ACTION_CRAFT_RECIPE_AUTO_ACTION_DATA_INGREDIENTS,
                    4,
                )?;
                Self::CraftRecipeAutoActionData {
                    action_type,
                    recipe_net_id,
                    number_of_requested_crafts,
                    ingredients,
                }
            }
            12 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let creative_item_net_id = {
                    let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                let number_of_requested_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                Self::CraftCreativeActionData {
                    action_type,
                    creative_item_net_id,
                    number_of_requested_crafts,
                }
            }
            13 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let recipe_net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
                let filtered_string_index = <wire::I32LE as wire::Decode>::decode(reader)?;
                Self::CraftRecipeOptionalActionData {
                    action_type,
                    recipe_net_id,
                    filtered_string_index,
                }
            }
            14 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let recipe_net_id = <wire::I32LE as wire::Decode>::decode(reader)?;
                let number_of_requested_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                let repair_cost = {
                    let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), None)?;
                    value
                };
                Self::CraftRepairAndDisenchantActionData {
                    action_type,
                    recipe_net_id,
                    number_of_requested_crafts,
                    repair_cost,
                }
            }
            15 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let pattern_name_id = reader.read_str(
                    crate::fields::STACK_REQUEST_ACTION_CRAFT_LOOM_ACTION_DATA_PATTERN_NAME_ID,
                )?;
                let num_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                Self::CraftLoomActionData {
                    action_type,
                    pattern_name_id,
                    num_crafts,
                }
            }
            16 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                Self::CraftNonImplementedActionData { action_type }
            }
            17 => {
                let action_type = <ItemStackRequestActionType as wire::Decode>::decode(reader)?;
                let craft_results = wire::decode_collection_limits::<ItemInstance>(
                    reader,
                    crate::fields::STACK_REQUEST_ACTION_CRAFT_RESULTS_ACTION_DATA_CRAFT_RESULTS,
                    6,
                    1,
                    18446744073709551615,
                )?;
                let num_crafts = {
                    let value = <wire::U8 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(1), None)?;
                    value
                };
                Self::CraftResultsActionData {
                    action_type,
                    craft_results,
                    num_crafts,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "StackRequestAction",
                    value: value as i64,
                });
            }
        })
    }
}

/// StackRequestSlotInfo holds information on a specific slot client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackRequestSlotInfo {
    /// Container is the FullContainerName that describes the container that the slot is in.
    pub full_container_name: FullContainerName,
    /// `slot` is the index of the slot within the container with the ContainerID above.
    pub slot: wire::U8,
    /// StackNetworkID is the unique stack ID that the client assumes to be present in this slot. The server must
    /// check if these IDs match. If they do not match, servers should reject the stack request that the action
    /// holding this info was in.
    pub net_id_variant: wire::I32LE,
}

impl wire::Encode for StackRequestSlotInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.full_container_name.encode(writer)?;
        self.slot.encode(writer)?;
        self.net_id_variant.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StackRequestSlotInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let full_container_name = <FullContainerName as wire::Decode>::decode(reader)?;
        let slot = <wire::U8 as wire::Decode>::decode(reader)?;
        let net_id_variant = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            full_container_name,
            slot,
            net_id_variant,
        })
    }
}

// Domain: map

/// MapDecoration is a fixed decoration on a map: Its position or other properties do not change automatically
/// client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapDecoration {
    /// Type is the type of the map decoration. The type specifies the shape (and sometimes the colour) that the
    /// map decoration gets. It is one of the MapDecorationType constants above.
    pub image_type: MapDecorationType,
    /// `rotation` is the rotation of the map decoration. It is byte due to the 16 fixed directions that the map
    /// decoration may face.
    pub rotation: wire::U8,
    /// `x` is the offset on the X axis in pixels of the decoration.
    pub x: wire::U8,
    /// `y` is the offset on the Y axis in pixels of the decoration.
    pub y: wire::U8,
    /// `label` is the name of the map decoration. This name may be of any value.
    pub label: wire::Str,
    /// Colour is the colour of the map decoration. Some map decoration types have a specific colour set
    /// automatically, whereas others may be changed.
    pub color: MceColor,
}

impl wire::Encode for MapDecoration {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.image_type.encode(writer)?;
        self.rotation.encode(writer)?;
        wire::check_number_limits(self.rotation.0, Some(0), Some(255))?;
        self.x.encode(writer)?;
        wire::check_number_limits(self.x.0, Some(0), Some(255))?;
        self.y.encode(writer)?;
        wire::check_number_limits(self.y.0, Some(0), Some(255))?;
        self.label.encode(writer)?;
        self.color.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapDecoration {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let image_type = <MapDecorationType as wire::Decode>::decode(reader)?;
        let rotation = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let x = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let y = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let label = reader.read_str(crate::fields::MAP_DECORATION_LABEL)?;
        let color = <MceColor as wire::Decode>::decode(reader)?;
        Ok(Self {
            image_type,
            rotation,
            x,
            y,
            label,
            color,
        })
    }
}

/// MapTrackedObject is an object on a map that is 'tracked' by the client, such as an entity or a block. This
/// object may move, which is handled client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapItemTrackedActorUniqueId {
    /// `type_` is the type of the tracked object. It is either MapObjectTypeEntity or MapObjectTypeBlock.
    pub type_: MapItemTrackedActorType,
    /// EntityUniqueID is the optional unique ID of the tracked entity.
    /// Wire presence: optional value is preceded by a presence marker.
    pub entity_id: Option<ActorUniqueID>,
    /// `block_position` is the optional position of the tracked block.
    /// Wire presence: optional value is preceded by a presence marker.
    pub block_position: Option<BlockPos>,
}

impl wire::Encode for MapItemTrackedActorUniqueId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        match &self.entity_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.block_position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for MapItemTrackedActorUniqueId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = <MapItemTrackedActorType as wire::Decode>::decode(reader)?;
        let entity_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ActorUniqueID as wire::Decode>::decode(reader)?)
            }
        };
        let block_position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<BlockPos as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            type_,
            entity_id,
            block_position,
        })
    }
}

/// PixelRequest is the request for the colour of a pixel in a MapInfoRequest packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PixelRequest {
    pub pixel: wire::U32LE,
    pub index: wire::U16LE,
}

impl wire::Encode for PixelRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pixel.encode(writer)?;
        self.index.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PixelRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pixel = <wire::U32LE as wire::Decode>::decode(reader)?;
        let index = <wire::U16LE as wire::Decode>::decode(reader)?;
        Ok(Self { pixel, index })
    }
}

// Domain: memory_category

/// MemoryCategoryCounter represents a memory usage counter for a specific category.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryCategoryCounter {
    /// `category` is the memory category. It is one of the MemoryCategory constants above.
    pub category: MemoryCategory,
    /// Bytes is the number of bytes used by this category.
    pub current_bytes: wire::U64LE,
}

impl wire::Encode for MemoryCategoryCounter {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.category.encode(writer)?;
        self.current_bytes.encode(writer)?;
        wire::check_number_limits(self.current_bytes.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for MemoryCategoryCounter {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let category = <MemoryCategory as wire::Decode>::decode(reader)?;
        let current_bytes = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            category,
            current_bytes,
        })
    }
}

// Domain: misc

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdventureSettings {
    /// `no_pvm` is a boolean indicating whether the player is allowed to fight mobs or not.
    pub no_pvm: bool,
    /// `no_mvp` is a boolean indicating whether mobs are allowed to fight the player or not. It is unclear why
    /// this is sent to the client.
    pub no_mvp: bool,
    /// `immutable_world` is a boolean indicating whether the player is allowed to modify the world or not.
    pub immutable_world: bool,
    /// `show_name_tags` is a boolean indicating whether player name tags are shown or not.
    pub show_name_tags: bool,
    /// `auto_jump` is a boolean indicating whether the player is allowed to jump automatically or not.
    pub auto_jump: bool,
}

impl wire::Encode for AdventureSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.no_pvm.encode(writer)?;
        self.no_mvp.encode(writer)?;
        self.immutable_world.encode(writer)?;
        self.show_name_tags.encode(writer)?;
        self.auto_jump.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AdventureSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let no_pvm = <bool as wire::Decode>::decode(reader)?;
        let no_mvp = <bool as wire::Decode>::decode(reader)?;
        let immutable_world = <bool as wire::Decode>::decode(reader)?;
        let show_name_tags = <bool as wire::Decode>::decode(reader)?;
        let auto_jump = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            no_pvm,
            no_mvp,
            immutable_world,
            show_name_tags,
            auto_jump,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatedImageData {
    pub skin_image: SkinImage,
    pub animated_texture_type: PersonaAnimatedTextureType,
    pub frames: wire::F32LE,
    pub animation_expression: PersonaAnimationExpression,
}

impl wire::Encode for AnimatedImageData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.skin_image.encode(writer)?;
        self.animated_texture_type.encode(writer)?;
        self.frames.encode(writer)?;
        self.animation_expression.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnimatedImageData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let skin_image = <SkinImage as wire::Decode>::decode(reader)?;
        let animated_texture_type = <PersonaAnimatedTextureType as wire::Decode>::decode(reader)?;
        let frames = <wire::F32LE as wire::Decode>::decode(reader)?;
        let animation_expression = <PersonaAnimationExpression as wire::Decode>::decode(reader)?;
        Ok(Self {
            skin_image,
            animated_texture_type,
            frames,
            animation_expression,
        })
    }
}

/// PlayerArmourDamageEntry represents an entry for a single piece of armour that should be damaged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArmorSlotAndDamagePair {
    /// ArmourSlot is the index of the armour slot to damage.
    pub armor_slot: LegacyArmorSlot,
    /// `damage` is the amount of damage to apply to the armour in the specified slot.
    pub damage: wire::I16LE,
}

impl wire::Encode for ArmorSlotAndDamagePair {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.armor_slot.encode(writer)?;
        self.damage.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ArmorSlotAndDamagePair {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let armor_slot = <LegacyArmorSlot as wire::Decode>::decode(reader)?;
        let damage = <wire::I16LE as wire::Decode>::decode(reader)?;
        Ok(Self { armor_slot, damage })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BedrockDDUI {
    DataStoreUpdate(BedrockDDUIDataStoreUpdate),
    DataStoreChange {
        /// `data_store_name` is the name of the data store.
        data_store_name: wire::Str,
        /// `property` is the property that changed.
        property: wire::Str,
        /// `update_count` is the update count.
        update_count: wire::U32LE,
        /// NewValue is the new property value.
        the_new_property_value: DynamicValue,
    },
    DataStoreRemoval {
        data_store_name: wire::Str,
    },
}

impl BedrockDDUI {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::DataStoreUpdate(..) => 0,
            Self::DataStoreChange { .. } => 1,
            Self::DataStoreRemoval { .. } => 2,
        }
    }
}

impl Default for BedrockDDUI {
    fn default() -> Self {
        Self::DataStoreUpdate(Default::default())
    }
}

impl wire::Encode for BedrockDDUI {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::DataStoreUpdate(value) => {
                value.encode(writer)?;
            }
            Self::DataStoreChange {
                data_store_name,
                property,
                update_count,
                the_new_property_value,
            } => {
                wire::encode_string_limits(writer, data_store_name, 1, 1000)?;
                wire::encode_string_limits(writer, property, 1, 1000)?;
                update_count.encode(writer)?;
                wire::check_number_limits(update_count.0, Some(0), Some(4294967294))?;
                the_new_property_value.encode(writer)?;
            }
            Self::DataStoreRemoval { data_store_name } => {
                wire::encode_string_limits(writer, data_store_name, 1, 1000)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for BedrockDDUI {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::DataStoreUpdate(<BedrockDDUIDataStoreUpdate as wire::Decode>::decode(
                reader,
            )?),
            1 => {
                let data_store_name = wire::decode_string_limits(
                    reader,
                    crate::fields::BEDROCK_DDUI_DATA_STORE_CHANGE_DATA_STORE_NAME,
                    1,
                    1000,
                )?;
                let property = wire::decode_string_limits(
                    reader,
                    crate::fields::BEDROCK_DDUI_DATA_STORE_CHANGE_PROPERTY,
                    1,
                    1000,
                )?;
                let update_count = {
                    let value = <wire::U32LE as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(0), Some(4294967294))?;
                    value
                };
                let the_new_property_value = <DynamicValue as wire::Decode>::decode(reader)?;
                Self::DataStoreChange {
                    data_store_name,
                    property,
                    update_count,
                    the_new_property_value,
                }
            }
            2 => {
                let data_store_name = wire::decode_string_limits(
                    reader,
                    crate::fields::BEDROCK_DDUI_DATA_STORE_REMOVAL_DATA_STORE_NAME,
                    1,
                    1000,
                )?;
                Self::DataStoreRemoval { data_store_name }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "BedrockDDUI",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BedrockDDUIDataStoreUpdate {
    pub data_store_name: wire::Str,
    pub property: wire::Str,
    pub path: wire::Str,
    pub data: BedrockDDUIDataStoreUpdateData,
    pub property_update_count: wire::U32LE,
    pub path_update_count: wire::U32LE,
}

impl wire::Encode for BedrockDDUIDataStoreUpdate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.data_store_name, 1, 1000)?;
        wire::encode_string_limits(writer, &self.property, 1, 1000)?;
        wire::encode_string_limits(writer, &self.path, 0, 1000)?;
        self.data.encode(writer)?;
        self.property_update_count.encode(writer)?;
        wire::check_number_limits(self.property_update_count.0, None, Some(4294967294))?;
        self.path_update_count.encode(writer)?;
        wire::check_number_limits(self.path_update_count.0, None, Some(4294967294))?;
        Ok(())
    }
}

impl wire::Decode for BedrockDDUIDataStoreUpdate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let data_store_name = wire::decode_string_limits(
            reader,
            crate::fields::BEDROCK_DDUI_DATA_STORE_UPDATE_DATA_STORE_NAME,
            1,
            1000,
        )?;
        let property = wire::decode_string_limits(
            reader,
            crate::fields::BEDROCK_DDUI_DATA_STORE_UPDATE_PROPERTY,
            1,
            1000,
        )?;
        let path = wire::decode_string_limits(
            reader,
            crate::fields::BEDROCK_DDUI_DATA_STORE_UPDATE_PATH,
            0,
            1000,
        )?;
        let data = <BedrockDDUIDataStoreUpdateData as wire::Decode>::decode(reader)?;
        let property_update_count = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(4294967294))?;
            value
        };
        let path_update_count = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(4294967294))?;
            value
        };
        Ok(Self {
            data_store_name,
            property,
            path,
            data,
            property_update_count,
            path_update_count,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContentIdentity {
    pub identity: wire::Str,
}

impl wire::Encode for ContentIdentity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.identity.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContentIdentity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let identity = reader.read_str(crate::fields::CONTENT_IDENTITY_IDENTITY)?;
        Ok(Self { identity })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataItemEntry {
    pub id: wire::VarUInt,
    pub payload: DataItemEntryValue,
}

impl wire::Encode for DataItemEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        wire::check_number_limits(self.id.0, Some(0), None)?;
        self.payload.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DataItemEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let payload = <DataItemEntryValue as wire::Decode>::decode(reader)?;
        Ok(Self { id, payload })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugMarkerData {
    pub text: wire::Str,
    pub position: glam::Vec3,
    pub color: MceColor,
    pub duration: wire::U64LE,
}

impl wire::Encode for DebugMarkerData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.text, 0, 4096)?;
        self.position.encode(writer)?;
        self.color.encode(writer)?;
        self.duration.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DebugMarkerData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let text =
            wire::decode_string_limits(reader, crate::fields::DEBUG_MARKER_DATA_TEXT, 0, 4096)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let color = <MceColor as wire::Decode>::decode(reader)?;
        let duration = <wire::U64LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            text,
            position,
            color,
            duration,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DimensionType(pub i32);

impl wire::Encode for DimensionType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DimensionType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag32 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum DynamicValue {
    #[default]
    None,
    Bool(bool),
    Int64(wire::I64LE),
    Double(wire::F64LE),
    String(wire::Str),
    List(Vec<DynamicValue>),
    Map(Vec<(wire::Str, DynamicValue)>),
}

impl DynamicValue {
    pub fn discriminant(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Bool(..) => 1,
            Self::Int64(..) => 2,
            Self::Double(..) => 3,
            Self::String(..) => 4,
            Self::List(..) => 5,
            Self::Map(..) => 6,
        }
    }
}

impl wire::Encode for DynamicValue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.discriminant()).encode(writer)?;
        match self {
            Self::None => {}
            Self::Bool(value) => {
                value.encode(writer)?;
            }
            Self::Int64(value) => {
                value.encode(writer)?;
            }
            Self::Double(value) => {
                value.encode(writer)?;
            }
            Self::String(value) => {
                value.encode(writer)?;
            }
            Self::List(value) => {
                wire::encode_collection(writer, value.as_slice())?;
            }
            Self::Map(value) => {
                writer.write_len_var_u32(value.len())?;
                for (key, value) in value.iter() {
                    key.encode(writer)?;
                    value.encode(writer)?;
                }
            }
        }
        Ok(())
    }
}

impl wire::Decode for DynamicValue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::I32LE as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::None,
            1 => Self::Bool(<bool as wire::Decode>::decode(reader)?),
            2 => Self::Int64(<wire::I64LE as wire::Decode>::decode(reader)?),
            3 => Self::Double(<wire::F64LE as wire::Decode>::decode(reader)?),
            4 => Self::String(reader.read_str(crate::fields::DYNAMIC_VALUE_STRING)?),
            5 => Self::List({
                let declared = u64::from(reader.read_var_u32()?);
                let count = reader.checked_count(crate::fields::DYNAMIC_VALUE_LIST, declared, 1)?;
                let mut out: Vec<DynamicValue> = wire::alloc_vec(count)?;
                for _ in 0..count {
                    wire::push(
                        &mut out,
                        reader.with_recursive_value(|reader| {
                            <DynamicValue as wire::Decode>::decode(reader)
                        })?,
                    )?;
                }
                out
            }),
            6 => Self::Map({
                let declared = u64::from(reader.read_var_u32()?);
                let count = reader.checked_count(crate::fields::DYNAMIC_VALUE_MAP, declared, 2)?;
                let mut out: Vec<(wire::Str, DynamicValue)> = wire::alloc_vec(count)?;
                for _ in 0..count {
                    let key = reader.read_str(crate::fields::DYNAMIC_VALUE_MAP_KEY)?;
                    let value = reader.with_recursive_value(|reader| {
                        <DynamicValue as wire::Decode>::decode(reader)
                    })?;
                    wire::push(&mut out, (key, value))?;
                }
                out
            }),
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "DynamicValue",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EASAttributeLayerData {
    pub name: wire::Str,
    /// Wire presence: optional value is preceded by a presence marker.
    pub noise_name: Option<wire::Str>,
    pub dimension: DimensionType,
    pub settings: EASAttributeLayerSettings,
    pub attributes: Vec<EASEnvironmentAttributeData>,
}

impl wire::Encode for EASAttributeLayerData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.name, 0, 128)?;
        match &self.noise_name {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 0, 128)?;
            }
            None => writer.write_u8(0)?,
        }
        self.dimension.encode(writer)?;
        self.settings.encode(writer)?;
        wire::encode_collection_limits(writer, self.attributes.as_slice(), 0, 1024)?;
        Ok(())
    }
}

impl wire::Decode for EASAttributeLayerData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = wire::decode_string_limits(
            reader,
            crate::fields::EAS_ATTRIBUTE_LAYER_DATA_NAME,
            0,
            128,
        )?;
        let noise_name = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(
                    reader,
                    crate::fields::EAS_ATTRIBUTE_LAYER_DATA_NOISE_NAME,
                    0,
                    128,
                )?)
            }
        };
        let dimension = <DimensionType as wire::Decode>::decode(reader)?;
        let settings = <EASAttributeLayerSettings as wire::Decode>::decode(reader)?;
        let attributes = wire::decode_collection_limits::<EASEnvironmentAttributeData>(
            reader,
            crate::fields::EAS_ATTRIBUTE_LAYER_DATA_ATTRIBUTES,
            22,
            0,
            1024,
        )?;
        Ok(Self {
            name,
            noise_name,
            dimension,
            settings,
            attributes,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EASAttributeLayerSettings {
    pub priority: wire::I32LE,
    pub weight: wire::F32LE,
    pub enabled: bool,
    pub transitions_paused: bool,
}

impl wire::Encode for EASAttributeLayerSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.priority.encode(writer)?;
        self.weight.encode(writer)?;
        self.enabled.encode(writer)?;
        self.transitions_paused.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EASAttributeLayerSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let priority = <wire::I32LE as wire::Decode>::decode(reader)?;
        let weight = <wire::F32LE as wire::Decode>::decode(reader)?;
        let enabled = <bool as wire::Decode>::decode(reader)?;
        let transitions_paused = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            priority,
            weight,
            enabled,
            transitions_paused,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EASEnvironmentAttributeData {
    pub attribute_name: wire::Str,
    /// Wire presence: optional value is preceded by a presence marker.
    pub from_attribute: Option<EAS>,
    pub attribute: EAS,
    /// Wire presence: optional value is preceded by a presence marker.
    pub to_attribute: Option<EAS>,
    pub current_transition_ticks: wire::U32LE,
    pub total_transition_ticks: wire::U32LE,
    pub easing: wire::Str,
    pub local_transition_ticks: wire::U32LE,
    pub noise_transition: bool,
    pub noise_alignment: EASNoiseAlignment,
}

impl wire::Encode for EASEnvironmentAttributeData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.attribute_name, 0, 128)?;
        match &self.from_attribute {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.attribute.encode(writer)?;
        match &self.to_attribute {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.current_transition_ticks.encode(writer)?;
        wire::check_number_limits(self.current_transition_ticks.0, Some(0), None)?;
        self.total_transition_ticks.encode(writer)?;
        wire::check_number_limits(self.total_transition_ticks.0, Some(0), None)?;
        self.easing.encode(writer)?;
        self.local_transition_ticks.encode(writer)?;
        wire::check_number_limits(self.local_transition_ticks.0, Some(0), None)?;
        self.noise_transition.encode(writer)?;
        self.noise_alignment.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EASEnvironmentAttributeData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let attribute_name = wire::decode_string_limits(
            reader,
            crate::fields::EAS_ENVIRONMENT_ATTRIBUTE_DATA_ATTRIBUTE_NAME,
            0,
            128,
        )?;
        let from_attribute = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<EAS as wire::Decode>::decode(reader)?)
            }
        };
        let attribute = <EAS as wire::Decode>::decode(reader)?;
        let to_attribute = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<EAS as wire::Decode>::decode(reader)?)
            }
        };
        let current_transition_ticks = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let total_transition_ticks = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let easing = reader.read_str(crate::fields::EAS_ENVIRONMENT_ATTRIBUTE_DATA_EASING)?;
        let local_transition_ticks = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let noise_transition = <bool as wire::Decode>::decode(reader)?;
        let noise_alignment = <EASNoiseAlignment as wire::Decode>::decode(reader)?;
        Ok(Self {
            attribute_name,
            from_attribute,
            attribute,
            to_attribute,
            current_transition_ticks,
            total_transition_ticks,
            easing,
            local_transition_ticks,
            noise_transition,
            noise_alignment,
        })
    }
}

/// EntityDiagnosticTimingInfo represents diagnostics for a specific entity type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ECSProfilingDiagnosticsEntityDiagnosticTimingInfo {
    /// `display_name` is the name to display for this timing entry.
    pub display_name: wire::Str,
    /// `entity` is the identifier of the entity that is being timed.
    pub entity: wire::Str,
    /// DurationNanos is whole long the timing entry has lasted, in nanoseconds.
    pub time_in_ns: wire::U64LE,
    /// `percent_of_total` is the percentage of time that this timing entry has used compared to others.
    pub percent_of_total: wire::U8,
    /// `position` is the position of the entity that is being timed.
    /// Wire presence: optional value is preceded by a presence marker.
    pub position: Option<glam::Vec3>,
    /// `dimension` is the name of the dimension that the entity being timed is in.
    /// Wire presence: optional value is preceded by a presence marker.
    pub dimension: Option<wire::Str>,
}

impl wire::Encode for ECSProfilingDiagnosticsEntityDiagnosticTimingInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.display_name.encode(writer)?;
        self.entity.encode(writer)?;
        self.time_in_ns.encode(writer)?;
        self.percent_of_total.encode(writer)?;
        match &self.position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.dimension {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ECSProfilingDiagnosticsEntityDiagnosticTimingInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let display_name = reader.read_str(
            crate::fields::ECS_PROFILING_DIAGNOSTICS_ENTITY_DIAGNOSTIC_TIMING_INFO_DISPLAY_NAME,
        )?;
        let entity = reader.read_str(
            crate::fields::ECS_PROFILING_DIAGNOSTICS_ENTITY_DIAGNOSTIC_TIMING_INFO_ENTITY,
        )?;
        let time_in_ns = <wire::U64LE as wire::Decode>::decode(reader)?;
        let percent_of_total = <wire::U8 as wire::Decode>::decode(reader)?;
        let position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let dimension = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::ECS_PROFILING_DIAGNOSTICS_ENTITY_DIAGNOSTIC_TIMING_INFO_DIMENSION)?)
            }
        };
        Ok(Self {
            display_name,
            entity,
            time_in_ns,
            percent_of_total,
            position,
            dimension,
        })
    }
}

/// SystemCategory maps a diagnostics category name to a system index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ECSProfilingDiagnosticsSystemCategory {
    pub category_name: wire::Str,
    pub system_index: wire::U64LE,
}

impl wire::Encode for ECSProfilingDiagnosticsSystemCategory {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.category_name.encode(writer)?;
        self.system_index.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ECSProfilingDiagnosticsSystemCategory {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let category_name = reader
            .read_str(crate::fields::ECS_PROFILING_DIAGNOSTICS_SYSTEM_CATEGORY_CATEGORY_NAME)?;
        let system_index = <wire::U64LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            category_name,
            system_index,
        })
    }
}

/// SystemDiagnosticTimingInfo represents diagnostics for a specific system index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ECSProfilingDiagnosticsSystemDiagnosticTimingInfo {
    /// `display_name` is the name to display for this timing entry.
    pub display_name: wire::Str,
    /// `system_index` is the index of the system that is being timed.
    pub system_index: wire::U64LE,
    /// DurationNanos is whole long the timing entry has lasted, in nanoseconds.
    pub time_in_ns: wire::U64LE,
    /// `percent_of_total` is the percentage of time that this timing entry has used compared to others.
    pub percent_of_total: wire::U8,
}

impl wire::Encode for ECSProfilingDiagnosticsSystemDiagnosticTimingInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.display_name.encode(writer)?;
        self.system_index.encode(writer)?;
        self.time_in_ns.encode(writer)?;
        self.percent_of_total.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ECSProfilingDiagnosticsSystemDiagnosticTimingInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let display_name = reader.read_str(
            crate::fields::ECS_PROFILING_DIAGNOSTICS_SYSTEM_DIAGNOSTIC_TIMING_INFO_DISPLAY_NAME,
        )?;
        let system_index = <wire::U64LE as wire::Decode>::decode(reader)?;
        let time_in_ns = <wire::U64LE as wire::Decode>::decode(reader)?;
        let percent_of_total = <wire::U8 as wire::Decode>::decode(reader)?;
        Ok(Self {
            display_name,
            system_index,
            time_in_ns,
            percent_of_total,
        })
    }
}

/// EducationSharedResourceURI is an education edition feature that is used for transmitting education
/// resource settings to clients. It contains a button name and a link URL.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EduSharedUriResource {
    /// `button_name` is the button name of the resource URI.
    pub button_name: wire::Str,
    /// `link_uri` is the link URI for the resource URI.
    pub link_uri: wire::Str,
}

impl wire::Encode for EduSharedUriResource {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.button_name.encode(writer)?;
        self.link_uri.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EduSharedUriResource {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let button_name = reader.read_str(crate::fields::EDU_SHARED_URI_RESOURCE_BUTTON_NAME)?;
        let link_uri = reader.read_str(crate::fields::EDU_SHARED_URI_RESOURCE_LINK_URI)?;
        Ok(Self {
            button_name,
            link_uri,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Experiments {
    pub toggles: Vec<ExperimentToggle>,
    pub experiments_ever_toggled: bool,
}

impl wire::Encode for Experiments {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_u32le(writer, self.toggles.as_slice())?;
        self.experiments_ever_toggled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Experiments {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let toggles = wire::decode_collection_u32le::<ExperimentToggle>(
            reader,
            crate::fields::EXPERIMENTS_TOGGLES,
            2,
        )?;
        let experiments_ever_toggled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            toggles,
            experiments_ever_toggled,
        })
    }
}

/// EducationExternalLinkSettings ...
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExternalLinkSettings {
    /// `url` is the external link URL.
    pub url: wire::Str,
    /// `display_name` is the display name in game.
    pub display_name: wire::Str,
}

impl wire::Encode for ExternalLinkSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.url.encode(writer)?;
        self.display_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ExternalLinkSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let url = reader.read_str(crate::fields::EXTERNAL_LINK_SETTINGS_URL)?;
        let display_name = reader.read_str(crate::fields::EXTERNAL_LINK_SETTINGS_DISPLAY_NAME)?;
        Ok(Self { url, display_name })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureRegistryFeatureBinaryJsonFormat {
    pub feature_name: wire::Str,
    pub binary_json_output: bytes::Bytes,
}

impl wire::Encode for FeatureRegistryFeatureBinaryJsonFormat {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.feature_name.encode(writer)?;
        self.binary_json_output.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for FeatureRegistryFeatureBinaryJsonFormat {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let feature_name = reader
            .read_str(crate::fields::FEATURE_REGISTRY_FEATURE_BINARY_JSON_FORMAT_FEATURE_NAME)?;
        let binary_json_output = reader.read_shared_bytes(
            crate::fields::FEATURE_REGISTRY_FEATURE_BINARY_JSON_FORMAT_BINARY_JSON_OUTPUT,
        )?;
        Ok(Self {
            feature_name,
            binary_json_output,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameRulesChangedData {
    pub rules_list: Vec<GameRule>,
}

impl wire::Encode for GameRulesChangedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.rules_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for GameRulesChangedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rules_list = wire::decode_collection::<GameRule>(
            reader,
            crate::fields::GAME_RULES_CHANGED_DATA_RULES_LIST,
            3,
        )?;
        Ok(Self { rules_list })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightmapData {
    pub height_map_type: HeightMapDataType,
    /// Wire presence: optional value is preceded by a presence marker.
    pub subchunk_height_map: Option<[Vec<wire::I8>; 16]>,
    pub render_height_map_type: HeightMapDataType,
    /// Wire presence: optional value is preceded by a presence marker.
    pub subchunk_render_height_map: Option<[Vec<wire::I8>; 16]>,
}

impl wire::Encode for HeightmapData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.height_map_type.encode(writer)?;
        match &self.subchunk_height_map {
            Some(value) => {
                writer.write_u8(1)?;
                for item in value.iter() {
                    wire::encode_collection(writer, item.as_slice())?;
                }
            }
            None => writer.write_u8(0)?,
        }
        self.render_height_map_type.encode(writer)?;
        match &self.subchunk_render_height_map {
            Some(value) => {
                writer.write_u8(1)?;
                for item in value.iter() {
                    wire::encode_collection(writer, item.as_slice())?;
                }
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for HeightmapData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let height_map_type = <HeightMapDataType as wire::Decode>::decode(reader)?;
        let subchunk_height_map = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some([
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                ])
            }
        };
        let render_height_map_type = <HeightMapDataType as wire::Decode>::decode(reader)?;
        let subchunk_render_height_map = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some([
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                    wire::decode_collection::<wire::I8>(
                        reader,
                        crate::fields::HEIGHTMAP_DATA_SUBCHUNK_RENDER_HEIGHT_MAP_ITEM,
                        1,
                    )?,
                ])
            }
        };
        Ok(Self {
            height_map_type,
            subchunk_height_map,
            render_height_map_type,
            subchunk_render_height_map,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacySetSlot {
    pub container_enum: ContainerEnumName,
    pub slots: bytes::Bytes,
}

impl wire::Encode for LegacySetSlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_enum.encode(writer)?;
        self.slots.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacySetSlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_enum = <ContainerEnumName as wire::Decode>::decode(reader)?;
        let slots = reader.read_shared_bytes(crate::fields::LEGACY_SET_SLOT_SLOTS)?;
        Ok(Self {
            container_enum,
            slots,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelSettings {
    pub seed: wire::U64LE,
    pub spawn_settings: SpawnSettings,
    pub generator_type: GeneratorType,
    pub game_type: GameType,
    pub is_hardcore: bool,
    pub game_difficulty: LegacyDifficulty,
    pub default_spawn_block_position: BlockPos,
    pub achievements_disabled: bool,
    pub editor_world_type: EditorWorldType,
    pub is_created_in_editor: bool,
    pub is_exported_from_editor: bool,
    pub day_cycle_stop_time: wire::ZigZag32,
    pub education_edition_offer: EducationEditionOffer,
    pub education_features_enabled: bool,
    pub education_product_id: wire::Str,
    pub rain_level: wire::F32LE,
    pub lightning_level: wire::F32LE,
    pub has_confirmed_platform_locked_content: bool,
    pub multiplayer_game_intent: bool,
    pub lan_broadcast_intent: bool,
    pub xbox_live_broadcast_setting: SocialGamePublishSetting,
    pub platform_broadcast_setting: SocialGamePublishSetting,
    pub commands_enabled: bool,
    pub texture_packs_required: bool,
    pub rule_data: GameRulesChangedData,
    pub experiments: Experiments,
    pub has_bonus_chest_enabled: bool,
    pub start_with_map_enabled: bool,
    pub player_permissions: PlayerPermissionLevel,
    pub server_chunk_tick_range: wire::I32LE,
    pub has_locked_behavior_pack: bool,
    pub has_locked_resource_pack: bool,
    pub is_from_locked_template: bool,
    pub use_msa_gamertags_only: bool,
    pub is_from_world_template: bool,
    pub is_world_template_option_locked: bool,
    pub only_spawn_v1_villagers: bool,
    pub persona_disabled: bool,
    pub custom_skins_disabled: bool,
    pub emote_chat_muted: bool,
    pub base_game_version: wire::Str,
    pub limited_world_width: wire::I32LE,
    pub limited_world_depth: wire::I32LE,
    pub nether_type: bool,
    pub edu_shared_uri_resource: EduSharedUriResource,
    /// Wire presence: optional value is preceded by a presence marker.
    pub override_force_experimental_gameplay: Option<bool>,
    pub chat_restriction_level: ChatRestrictionLevel,
    pub disable_player_interactions: bool,
    pub server_editor_connection_policy: ServerEditorConnectionPolicy,
    pub allow_anonymous_block_drops_in_editor_worlds: bool,
}

impl wire::Encode for LevelSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.seed.encode(writer)?;
        wire::check_number_limits(self.seed.0, Some(0), None)?;
        self.spawn_settings.encode(writer)?;
        self.generator_type.encode(writer)?;
        self.game_type.encode(writer)?;
        self.is_hardcore.encode(writer)?;
        self.game_difficulty.encode(writer)?;
        self.default_spawn_block_position.encode(writer)?;
        self.achievements_disabled.encode(writer)?;
        self.editor_world_type.encode(writer)?;
        self.is_created_in_editor.encode(writer)?;
        self.is_exported_from_editor.encode(writer)?;
        self.day_cycle_stop_time.encode(writer)?;
        self.education_edition_offer.encode(writer)?;
        self.education_features_enabled.encode(writer)?;
        self.education_product_id.encode(writer)?;
        self.rain_level.encode(writer)?;
        self.lightning_level.encode(writer)?;
        self.has_confirmed_platform_locked_content.encode(writer)?;
        self.multiplayer_game_intent.encode(writer)?;
        self.lan_broadcast_intent.encode(writer)?;
        self.xbox_live_broadcast_setting.encode(writer)?;
        self.platform_broadcast_setting.encode(writer)?;
        self.commands_enabled.encode(writer)?;
        self.texture_packs_required.encode(writer)?;
        self.rule_data.encode(writer)?;
        self.experiments.encode(writer)?;
        self.has_bonus_chest_enabled.encode(writer)?;
        self.start_with_map_enabled.encode(writer)?;
        self.player_permissions.encode(writer)?;
        self.server_chunk_tick_range.encode(writer)?;
        self.has_locked_behavior_pack.encode(writer)?;
        self.has_locked_resource_pack.encode(writer)?;
        self.is_from_locked_template.encode(writer)?;
        self.use_msa_gamertags_only.encode(writer)?;
        self.is_from_world_template.encode(writer)?;
        self.is_world_template_option_locked.encode(writer)?;
        self.only_spawn_v1_villagers.encode(writer)?;
        self.persona_disabled.encode(writer)?;
        self.custom_skins_disabled.encode(writer)?;
        self.emote_chat_muted.encode(writer)?;
        self.base_game_version.encode(writer)?;
        self.limited_world_width.encode(writer)?;
        self.limited_world_depth.encode(writer)?;
        self.nether_type.encode(writer)?;
        self.edu_shared_uri_resource.encode(writer)?;
        match &self.override_force_experimental_gameplay {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.chat_restriction_level.encode(writer)?;
        self.disable_player_interactions.encode(writer)?;
        self.server_editor_connection_policy.encode(writer)?;
        self.allow_anonymous_block_drops_in_editor_worlds
            .encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let seed = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let spawn_settings = <SpawnSettings as wire::Decode>::decode(reader)?;
        let generator_type = <GeneratorType as wire::Decode>::decode(reader)?;
        let game_type = <GameType as wire::Decode>::decode(reader)?;
        let is_hardcore = <bool as wire::Decode>::decode(reader)?;
        let game_difficulty = <LegacyDifficulty as wire::Decode>::decode(reader)?;
        let default_spawn_block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let achievements_disabled = <bool as wire::Decode>::decode(reader)?;
        let editor_world_type = <EditorWorldType as wire::Decode>::decode(reader)?;
        let is_created_in_editor = <bool as wire::Decode>::decode(reader)?;
        let is_exported_from_editor = <bool as wire::Decode>::decode(reader)?;
        let day_cycle_stop_time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let education_edition_offer = <EducationEditionOffer as wire::Decode>::decode(reader)?;
        let education_features_enabled = <bool as wire::Decode>::decode(reader)?;
        let education_product_id =
            reader.read_str(crate::fields::LEVEL_SETTINGS_EDUCATION_PRODUCT_ID)?;
        let rain_level = <wire::F32LE as wire::Decode>::decode(reader)?;
        let lightning_level = <wire::F32LE as wire::Decode>::decode(reader)?;
        let has_confirmed_platform_locked_content = <bool as wire::Decode>::decode(reader)?;
        let multiplayer_game_intent = <bool as wire::Decode>::decode(reader)?;
        let lan_broadcast_intent = <bool as wire::Decode>::decode(reader)?;
        let xbox_live_broadcast_setting =
            <SocialGamePublishSetting as wire::Decode>::decode(reader)?;
        let platform_broadcast_setting =
            <SocialGamePublishSetting as wire::Decode>::decode(reader)?;
        let commands_enabled = <bool as wire::Decode>::decode(reader)?;
        let texture_packs_required = <bool as wire::Decode>::decode(reader)?;
        let rule_data = <GameRulesChangedData as wire::Decode>::decode(reader)?;
        let experiments = <Experiments as wire::Decode>::decode(reader)?;
        let has_bonus_chest_enabled = <bool as wire::Decode>::decode(reader)?;
        let start_with_map_enabled = <bool as wire::Decode>::decode(reader)?;
        let player_permissions = <PlayerPermissionLevel as wire::Decode>::decode(reader)?;
        let server_chunk_tick_range = <wire::I32LE as wire::Decode>::decode(reader)?;
        let has_locked_behavior_pack = <bool as wire::Decode>::decode(reader)?;
        let has_locked_resource_pack = <bool as wire::Decode>::decode(reader)?;
        let is_from_locked_template = <bool as wire::Decode>::decode(reader)?;
        let use_msa_gamertags_only = <bool as wire::Decode>::decode(reader)?;
        let is_from_world_template = <bool as wire::Decode>::decode(reader)?;
        let is_world_template_option_locked = <bool as wire::Decode>::decode(reader)?;
        let only_spawn_v1_villagers = <bool as wire::Decode>::decode(reader)?;
        let persona_disabled = <bool as wire::Decode>::decode(reader)?;
        let custom_skins_disabled = <bool as wire::Decode>::decode(reader)?;
        let emote_chat_muted = <bool as wire::Decode>::decode(reader)?;
        let base_game_version = reader.read_str(crate::fields::LEVEL_SETTINGS_BASE_GAME_VERSION)?;
        let limited_world_width = <wire::I32LE as wire::Decode>::decode(reader)?;
        let limited_world_depth = <wire::I32LE as wire::Decode>::decode(reader)?;
        let nether_type = <bool as wire::Decode>::decode(reader)?;
        let edu_shared_uri_resource = <EduSharedUriResource as wire::Decode>::decode(reader)?;
        let override_force_experimental_gameplay = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let chat_restriction_level = <ChatRestrictionLevel as wire::Decode>::decode(reader)?;
        let disable_player_interactions = <bool as wire::Decode>::decode(reader)?;
        let server_editor_connection_policy =
            <ServerEditorConnectionPolicy as wire::Decode>::decode(reader)?;
        let allow_anonymous_block_drops_in_editor_worlds = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            seed,
            spawn_settings,
            generator_type,
            game_type,
            is_hardcore,
            game_difficulty,
            default_spawn_block_position,
            achievements_disabled,
            editor_world_type,
            is_created_in_editor,
            is_exported_from_editor,
            day_cycle_stop_time,
            education_edition_offer,
            education_features_enabled,
            education_product_id,
            rain_level,
            lightning_level,
            has_confirmed_platform_locked_content,
            multiplayer_game_intent,
            lan_broadcast_intent,
            xbox_live_broadcast_setting,
            platform_broadcast_setting,
            commands_enabled,
            texture_packs_required,
            rule_data,
            experiments,
            has_bonus_chest_enabled,
            start_with_map_enabled,
            player_permissions,
            server_chunk_tick_range,
            has_locked_behavior_pack,
            has_locked_resource_pack,
            is_from_locked_template,
            use_msa_gamertags_only,
            is_from_world_template,
            is_world_template_option_locked,
            only_spawn_v1_villagers,
            persona_disabled,
            custom_skins_disabled,
            emote_chat_muted,
            base_game_version,
            limited_world_width,
            limited_world_depth,
            nether_type,
            edu_shared_uri_resource,
            override_force_experimental_gameplay,
            chat_restriction_level,
            disable_player_interactions,
            server_editor_connection_policy,
            allow_anonymous_block_drops_in_editor_worlds,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialReducerDataEntry {
    pub from_item_key: wire::ZigZag32,
    pub item_ids_and_counts: Vec<MaterialReducerEntryOutput>,
}

impl wire::Encode for MaterialReducerDataEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.from_item_key.encode(writer)?;
        wire::encode_collection(writer, self.item_ids_and_counts.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for MaterialReducerDataEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let from_item_key = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let item_ids_and_counts = wire::decode_collection::<MaterialReducerEntryOutput>(
            reader,
            crate::fields::MATERIAL_REDUCER_DATA_ENTRY_ITEM_IDS_AND_COUNTS,
            2,
        )?;
        Ok(Self {
            from_item_key,
            item_ids_and_counts,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialReducerEntryOutput {
    pub item_id: wire::ZigZag32,
    pub item_count: wire::ZigZag32,
}

impl wire::Encode for MaterialReducerEntryOutput {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_id.encode(writer)?;
        self.item_count.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MaterialReducerEntryOutput {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let item_count = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_id,
            item_count,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MceColor(pub i32);

impl wire::Encode for MceColor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::I32LE(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MceColor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::I32LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissingBlobData {
    pub blob_id: wire::U64LE,
    pub blob_data: bytes::Bytes,
}

impl wire::Encode for MissingBlobData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.blob_id.encode(writer)?;
        self.blob_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MissingBlobData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let blob_id = <wire::U64LE as wire::Decode>::decode(reader)?;
        let blob_data = reader.read_shared_bytes(crate::fields::MISSING_BLOB_DATA_BLOB_DATA)?;
        Ok(Self { blob_id, blob_data })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveActorAbsoluteData {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub actor_runtime_id: ActorRuntimeID,
    pub header: wire::U8,
    /// `position` is the position to spawn the entity on. If the entity is on a distance that the player cannot
    /// see it, the entity will still show up if the player moves closer.
    pub position: glam::Vec3,
    pub rotation_x: wire::U8,
    pub rotation_y: wire::U8,
    pub rotation_y_head: wire::U8,
}

impl wire::Encode for MoveActorAbsoluteData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_runtime_id.encode(writer)?;
        self.header.encode(writer)?;
        self.position.encode(writer)?;
        self.rotation_x.encode(writer)?;
        self.rotation_y.encode(writer)?;
        self.rotation_y_head.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoveActorAbsoluteData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let header = <wire::U8 as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation_x = <wire::U8 as wire::Decode>::decode(reader)?;
        let rotation_y = <wire::U8 as wire::Decode>::decode(reader)?;
        let rotation_y_head = <wire::U8 as wire::Decode>::decode(reader)?;
        Ok(Self {
            actor_runtime_id,
            header,
            position,
            rotation_x,
            rotation_y,
            rotation_y_head,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveActorDeltaData {
    /// EntityRuntimeID is the runtime ID of the entity that is being moved. The packet works provided a
    /// non-player entity with this runtime ID is present.
    pub actor_runtime_id: ActorRuntimeID,
    /// Position is the new position that the entity was moved to.
    /// Wire presence: optional value is preceded by a presence marker.
    pub new_position_x: Option<wire::F32LE>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub new_position_y: Option<wire::F32LE>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub new_position_z: Option<wire::F32LE>,
    /// Rotation is the new absolute rotation. Unlike the position, it is not actually a delta. If any of the
    /// values of this rotation are not sent, these values are 0 and no flag for them is present.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rotation_x: Option<wire::I8>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub rotation_y: Option<wire::I8>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub rotation_y_head: Option<wire::I8>,
    /// OnGround specifies whether the entity is on the ground after applying the update.
    pub is_on_ground: bool,
    /// `force_move` specifies whether the client should snap the entity to its new position without
    /// interpolation.
    pub force_move: bool,
    /// `force_move_local_entity` specifies whether the client should also snap an entity it owns locally.
    pub force_move_local_entity: bool,
    /// `force_completion` specifies whether the client should complete any in-progress local movement before
    /// applying the update.
    pub force_completion: bool,
    /// `ticks` is the number of ticks over which the movement should be interpolated by the client.
    pub ticks: wire::VarULong,
}

impl wire::Encode for MoveActorDeltaData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_runtime_id.encode(writer)?;
        match &self.new_position_x {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.new_position_y {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.new_position_z {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rotation_x {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rotation_y {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rotation_y_head {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.is_on_ground.encode(writer)?;
        self.force_move.encode(writer)?;
        self.force_move_local_entity.encode(writer)?;
        self.force_completion.encode(writer)?;
        self.ticks.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoveActorDeltaData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let new_position_x = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let new_position_y = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let new_position_z = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let rotation_x = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I8 as wire::Decode>::decode(reader)?)
            }
        };
        let rotation_y = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I8 as wire::Decode>::decode(reader)?)
            }
        };
        let rotation_y_head = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I8 as wire::Decode>::decode(reader)?)
            }
        };
        let is_on_ground = <bool as wire::Decode>::decode(reader)?;
        let force_move = <bool as wire::Decode>::decode(reader)?;
        let force_move_local_entity = <bool as wire::Decode>::decode(reader)?;
        let force_completion = <bool as wire::Decode>::decode(reader)?;
        let ticks = <wire::VarULong as wire::Decode>::decode(reader)?;
        Ok(Self {
            actor_runtime_id,
            new_position_x,
            new_position_y,
            new_position_z,
            rotation_x,
            rotation_y,
            rotation_y_head,
            is_on_ground,
            force_move,
            force_move_local_entity,
            force_completion,
            ticks,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovePlayerTeleportData {
    pub teleportation_cause: wire::I32LE,
    pub source_actor_type: wire::I32LE,
}

impl wire::Encode for MovePlayerTeleportData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.teleportation_cause.encode(writer)?;
        self.source_actor_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MovePlayerTeleportData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let teleportation_cause = <wire::I32LE as wire::Decode>::decode(reader)?;
        let source_actor_type = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            teleportation_cause,
            source_actor_type,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkItemInstanceDescriptorSerializedData {
    pub id: wire::ZigZag32,
    pub stack_size: wire::U16LE,
    pub aux_value: wire::VarUInt,
    pub block_runtime_id: wire::ZigZag32,
    pub user_data_buffer: bytes::Bytes,
}

impl wire::Encode for NetworkItemInstanceDescriptorSerializedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        wire::check_number_limits(self.id.0, Some(-32768), Some(32767))?;
        self.stack_size.encode(writer)?;
        self.aux_value.encode(writer)?;
        wire::check_number_limits(self.aux_value.0, Some(0), Some(32767))?;
        self.block_runtime_id.encode(writer)?;
        self.user_data_buffer.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NetworkItemInstanceDescriptorSerializedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = {
            let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(-32768), Some(32767))?;
            value
        };
        let stack_size = <wire::U16LE as wire::Decode>::decode(reader)?;
        let aux_value = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(32767))?;
            value
        };
        let block_runtime_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let user_data_buffer = reader.read_shared_bytes(
            crate::fields::NETWORK_ITEM_INSTANCE_DESCRIPTOR_SERIALIZED_DATA_USER_DATA_BUFFER,
        )?;
        Ok(Self {
            id,
            stack_size,
            aux_value,
            block_runtime_id,
            user_data_buffer,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkItemStackDescriptorSerializedData {
    pub id: wire::I16LE,
    pub stack_size: wire::U16LE,
    pub aux_value: wire::VarUInt,
    /// Wire presence: optional value is preceded by a presence marker.
    pub net_id_variant: Option<wire::ZigZag32>,
    pub block_runtime_id: wire::VarUInt,
    pub user_data_buffer: bytes::Bytes,
}

impl wire::Encode for NetworkItemStackDescriptorSerializedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        self.stack_size.encode(writer)?;
        wire::check_number_limits(self.stack_size.0, Some(0), None)?;
        self.aux_value.encode(writer)?;
        wire::check_number_limits(self.aux_value.0, Some(0), Some(32767))?;
        match &self.net_id_variant {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.block_runtime_id.encode(writer)?;
        wire::check_number_limits(self.block_runtime_id.0, Some(0), None)?;
        self.user_data_buffer.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NetworkItemStackDescriptorSerializedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = <wire::I16LE as wire::Decode>::decode(reader)?;
        let stack_size = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let aux_value = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(32767))?;
            value
        };
        let net_id_variant = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag32 as wire::Decode>::decode(reader)?)
            }
        };
        let block_runtime_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let user_data_buffer = reader.read_shared_bytes(
            crate::fields::NETWORK_ITEM_STACK_DESCRIPTOR_SERIALIZED_DATA_USER_DATA_BUFFER,
        )?;
        Ok(Self {
            id,
            stack_size,
            aux_value,
            net_id_variant,
            block_runtime_id,
            user_data_buffer,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkPermissions {
    pub server_auth_sound_enabled: bool,
}

impl wire::Encode for NetworkPermissions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.server_auth_sound_enabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NetworkPermissions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let server_auth_sound_enabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            server_auth_sound_enabled,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedItemUseLegacyInventoryTransaction {
    pub legacy_request_id: ItemStackLegacyRequestID,
    /// Wire presence: optional value is preceded by a presence marker.
    pub legacy_set_item_slots: Option<Vec<LegacySetSlot>>,
    pub item_use_transaction: ItemUseInventoryTransaction,
}

impl wire::Encode for PackedItemUseLegacyInventoryTransaction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.legacy_request_id.encode(writer)?;
        match &self.legacy_set_item_slots {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection(writer, value.as_slice())?;
            }
            None => writer.write_u8(0)?,
        }
        self.item_use_transaction.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PackedItemUseLegacyInventoryTransaction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let legacy_request_id = <ItemStackLegacyRequestID as wire::Decode>::decode(reader)?;
        let legacy_set_item_slots = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection::<LegacySetSlot>(reader, crate::fields::PACKED_ITEM_USE_LEGACY_INVENTORY_TRANSACTION_LEGACY_SET_ITEM_SLOTS, 2)?)
            }
        };
        let item_use_transaction = <ItemUseInventoryTransaction as wire::Decode>::decode(reader)?;
        Ok(Self {
            legacy_request_id,
            legacy_set_item_slots,
            item_use_transaction,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PotionMixDataEntry {
    pub from_potion_id: wire::ZigZag32,
    pub from_item_aux: wire::ZigZag32,
    pub reagent_item_id: wire::ZigZag32,
    pub reagent_item_aux: wire::ZigZag32,
    pub to_potion_id: wire::ZigZag32,
    pub to_item_aux: wire::ZigZag32,
}

impl wire::Encode for PotionMixDataEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.from_potion_id.encode(writer)?;
        self.from_item_aux.encode(writer)?;
        self.reagent_item_id.encode(writer)?;
        self.reagent_item_aux.encode(writer)?;
        self.to_potion_id.encode(writer)?;
        self.to_item_aux.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PotionMixDataEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let from_potion_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let from_item_aux = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let reagent_item_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let reagent_item_aux = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let to_potion_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let to_item_aux = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            from_potion_id,
            from_item_aux,
            reagent_item_id,
            reagent_item_aux,
            to_potion_id,
            to_item_aux,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertySyncData {
    pub int_entries_list: Vec<PropertySyncDataPropertySyncIntEntry>,
    pub float_entries_list: Vec<PropertySyncDataPropertySyncFloatEntry>,
}

impl wire::Encode for PropertySyncData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.int_entries_list.as_slice())?;
        wire::encode_collection(writer, self.float_entries_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for PropertySyncData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let int_entries_list = wire::decode_collection::<PropertySyncDataPropertySyncIntEntry>(
            reader,
            crate::fields::PROPERTY_SYNC_DATA_INT_ENTRIES_LIST,
            2,
        )?;
        let float_entries_list = wire::decode_collection::<PropertySyncDataPropertySyncFloatEntry>(
            reader,
            crate::fields::PROPERTY_SYNC_DATA_FLOAT_ENTRIES_LIST,
            5,
        )?;
        Ok(Self {
            int_entries_list,
            float_entries_list,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertySyncDataPropertySyncFloatEntry {
    pub property_index: wire::VarUInt,
    pub data: wire::F32LE,
}

impl wire::Encode for PropertySyncDataPropertySyncFloatEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.property_index.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PropertySyncDataPropertySyncFloatEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let property_index = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let data = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            property_index,
            data,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertySyncDataPropertySyncIntEntry {
    pub property_index: wire::VarUInt,
    pub data: wire::ZigZag32,
}

impl wire::Encode for PropertySyncDataPropertySyncIntEntry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.property_index.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PropertySyncDataPropertySyncIntEntry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let property_index = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let data = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            property_index,
            data,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemVersion {
    pub version: wire::Str,
}

impl wire::Encode for SemVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SemVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let version = reader.read_str(crate::fields::SEM_VERSION_VERSION)?;
        Ok(Self { version })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemVersionData {
    pub version: wire::Str,
}

impl wire::Encode for SemVersionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SemVersionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let version = reader.read_str(crate::fields::SEM_VERSION_DATA_VERSION)?;
        Ok(Self { version })
    }
}

/// AbilityData represents various data about the abilities of a player, such as ability layers or
/// permissions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedAbilitiesData {
    /// EntityUniqueID is a unique identifier of the player. It appears it is not required to fill this field out
    /// with a correct value. Simply writing 0 seems to work.
    pub target_player_raw_id: wire::I64LE,
    /// `player_permissions` is the permission level of the player as it shows up in the player list built up
    /// using the PlayerList packet.
    pub player_permissions: PlayerPermissionLevel,
    /// `command_permissions` is a set of permissions that specify what commands a player is allowed to execute.
    pub command_permissions: CommandPermissionLevel,
    /// `layers` contains all ability layers and their potential values. This should at least have one entry,
    /// being the base layer.
    pub layers: Vec<SerializedAbilitiesDataSerializedLayer>,
}

impl wire::Encode for SerializedAbilitiesData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_player_raw_id.encode(writer)?;
        self.player_permissions.encode(writer)?;
        self.command_permissions.encode(writer)?;
        wire::encode_collection(writer, self.layers.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for SerializedAbilitiesData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_player_raw_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        let player_permissions = <PlayerPermissionLevel as wire::Decode>::decode(reader)?;
        let command_permissions = <CommandPermissionLevel as wire::Decode>::decode(reader)?;
        let layers = wire::decode_collection::<SerializedAbilitiesDataSerializedLayer>(
            reader,
            crate::fields::SERIALIZED_ABILITIES_DATA_LAYERS,
            22,
        )?;
        Ok(Self {
            target_player_raw_id,
            player_permissions,
            command_permissions,
            layers,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedAbilitiesDataSerializedLayer {
    pub serialized_layer: wire::U16LE,
    pub abilities_set: wire::U32LE,
    pub ability_values: wire::U32LE,
    pub fly_speed: wire::F32LE,
    pub vertical_fly_speed: wire::F32LE,
    pub walk_speed: wire::F32LE,
}

impl wire::Encode for SerializedAbilitiesDataSerializedLayer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.serialized_layer.encode(writer)?;
        self.abilities_set.encode(writer)?;
        self.ability_values.encode(writer)?;
        self.fly_speed.encode(writer)?;
        self.vertical_fly_speed.encode(writer)?;
        self.walk_speed.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SerializedAbilitiesDataSerializedLayer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let serialized_layer = <wire::U16LE as wire::Decode>::decode(reader)?;
        let abilities_set = <wire::U32LE as wire::Decode>::decode(reader)?;
        let ability_values = <wire::U32LE as wire::Decode>::decode(reader)?;
        let fly_speed = <wire::F32LE as wire::Decode>::decode(reader)?;
        let vertical_fly_speed = <wire::F32LE as wire::Decode>::decode(reader)?;
        let walk_speed = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            serialized_layer,
            abilities_set,
            ability_values,
            fly_speed,
            vertical_fly_speed,
            walk_speed,
        })
    }
}

/// NoiseBlockSpecifier specifies a block placed by the gradient noise based on a threshold and range.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedNoiseBlockSpecifier {
    /// `noise` is the noise name.
    pub noise: wire::Str,
    /// `threshold` is the noise threshold above which the block is placed.
    pub threshold: wire::F32LE,
    /// `range` is the noise range within which the block is placed.
    pub range: FloatRange,
    /// `block` is the block runtime ID placed by this specifier.
    pub block: wire::U32LE,
}

impl wire::Encode for SerializedNoiseBlockSpecifier {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.noise.encode(writer)?;
        self.threshold.encode(writer)?;
        self.range.encode(writer)?;
        self.block.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SerializedNoiseBlockSpecifier {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let noise = reader.read_str(crate::fields::SERIALIZED_NOISE_BLOCK_SPECIFIER_NOISE)?;
        let threshold = <wire::F32LE as wire::Decode>::decode(reader)?;
        let range = <FloatRange as wire::Decode>::decode(reader)?;
        let block = <wire::U32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            noise,
            threshold,
            range,
            block,
        })
    }
}

/// PersonaPiece represents a piece of a persona skin. All pieces are sent separately.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedPersonaPieceHandle {
    /// PieceId is a UUID that identifies the piece itself, which is unique for each separate piece.
    pub piece_id: wire::Str,
    /// `piece_type` holds the type of the piece. This is one of the PieceType constants above.
    pub piece_type: PersonaPieceType,
    /// `pack_id` is a UUID that identifies the pack that the persona piece belongs to.
    pub pack_id: uuid::Uuid,
    /// Default specifies if the piece is one of the default pieces. This is true when the piece is one of those
    /// that a Steve or Alex skin have.
    pub is_default_piece: bool,
    /// `product_id` is a UUID that identifies the piece when it comes to purchases. It is empty for pieces that
    /// have the 'Default' field set to true.
    pub product_id: wire::Str,
}

impl wire::Encode for SerializedPersonaPieceHandle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.piece_id.encode(writer)?;
        self.piece_type.encode(writer)?;
        self.pack_id.encode(writer)?;
        self.is_default_piece.encode(writer)?;
        self.product_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SerializedPersonaPieceHandle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let piece_id = reader.read_str(crate::fields::SERIALIZED_PERSONA_PIECE_HANDLE_PIECE_ID)?;
        let piece_type = <PersonaPieceType as wire::Decode>::decode(reader)?;
        let pack_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let is_default_piece = <bool as wire::Decode>::decode(reader)?;
        let product_id =
            reader.read_str(crate::fields::SERIALIZED_PERSONA_PIECE_HANDLE_PRODUCT_ID)?;
        Ok(Self {
            piece_id,
            piece_type,
            pack_id,
            is_default_piece,
            product_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SerializedSkinRef {
    pub id: wire::Str,
    pub play_fab_id: wire::Str,
    pub resource_patch: wire::Str,
    pub image_data: SkinImage,
    pub animated_image_data: Vec<AnimatedImageData>,
    pub cape_image_data: SkinImage,
    pub geometry_data: wire::Str,
    pub geometry_data_min_engine_version: wire::Str,
    pub animation_data: wire::Str,
    pub cape_id: wire::Str,
    pub full_id: wire::Str,
    pub arm_size: PersonaArmSizeType,
    pub skin_color: MceColor,
    pub persona_pieces: Vec<SerializedPersonaPieceHandle>,
    pub piece_tint_colors: Vec<(wire::Str, TintMapColor)>,
    pub is_premium: bool,
    pub is_persona: bool,
    pub is_persona_cape_on_classic_skin: bool,
    pub is_primary_user: bool,
    pub overrides_player_appearance: bool,
    pub trusted_skin_flag: wire::Str,
    pub profile_hash: wire::Str,
}

impl wire::Encode for SerializedSkinRef {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.id.encode(writer)?;
        self.play_fab_id.encode(writer)?;
        self.resource_patch.encode(writer)?;
        self.image_data.encode(writer)?;
        wire::encode_collection(writer, self.animated_image_data.as_slice())?;
        self.cape_image_data.encode(writer)?;
        self.geometry_data.encode(writer)?;
        self.geometry_data_min_engine_version.encode(writer)?;
        self.animation_data.encode(writer)?;
        self.cape_id.encode(writer)?;
        self.full_id.encode(writer)?;
        self.arm_size.encode(writer)?;
        self.skin_color.encode(writer)?;
        wire::encode_collection(writer, self.persona_pieces.as_slice())?;
        writer.write_len_var_u32(self.piece_tint_colors.len())?;
        for (key, value) in self.piece_tint_colors.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
        }
        self.is_premium.encode(writer)?;
        self.is_persona.encode(writer)?;
        self.is_persona_cape_on_classic_skin.encode(writer)?;
        self.is_primary_user.encode(writer)?;
        self.overrides_player_appearance.encode(writer)?;
        self.trusted_skin_flag.encode(writer)?;
        self.profile_hash.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SerializedSkinRef {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let id = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_ID)?;
        let play_fab_id = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_PLAY_FAB_ID)?;
        let resource_patch = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_RESOURCE_PATCH)?;
        let image_data = <SkinImage as wire::Decode>::decode(reader)?;
        let animated_image_data = wire::decode_collection::<AnimatedImageData>(
            reader,
            crate::fields::SERIALIZED_SKIN_REF_ANIMATED_IMAGE_DATA,
            15,
        )?;
        let cape_image_data = <SkinImage as wire::Decode>::decode(reader)?;
        let geometry_data = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_GEOMETRY_DATA)?;
        let geometry_data_min_engine_version =
            reader.read_str(crate::fields::SERIALIZED_SKIN_REF_GEOMETRY_DATA_MIN_ENGINE_VERSION)?;
        let animation_data = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_ANIMATION_DATA)?;
        let cape_id = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_CAPE_ID)?;
        let full_id = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_FULL_ID)?;
        let arm_size = <PersonaArmSizeType as wire::Decode>::decode(reader)?;
        let skin_color = <MceColor as wire::Decode>::decode(reader)?;
        let persona_pieces = wire::decode_collection::<SerializedPersonaPieceHandle>(
            reader,
            crate::fields::SERIALIZED_SKIN_REF_PERSONA_PIECES,
            23,
        )?;
        let piece_tint_colors = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::SERIALIZED_SKIN_REF_PIECE_TINT_COLORS,
                declared,
                17,
            )?;
            let mut out: Vec<(wire::Str, TintMapColor)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key =
                    reader.read_str(crate::fields::SERIALIZED_SKIN_REF_PIECE_TINT_COLORS_KEY)?;
                let value = <TintMapColor as wire::Decode>::decode(reader)?;
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let is_premium = <bool as wire::Decode>::decode(reader)?;
        let is_persona = <bool as wire::Decode>::decode(reader)?;
        let is_persona_cape_on_classic_skin = <bool as wire::Decode>::decode(reader)?;
        let is_primary_user = <bool as wire::Decode>::decode(reader)?;
        let overrides_player_appearance = <bool as wire::Decode>::decode(reader)?;
        let trusted_skin_flag =
            reader.read_str(crate::fields::SERIALIZED_SKIN_REF_TRUSTED_SKIN_FLAG)?;
        let profile_hash = reader.read_str(crate::fields::SERIALIZED_SKIN_REF_PROFILE_HASH)?;
        Ok(Self {
            id,
            play_fab_id,
            resource_patch,
            image_data,
            animated_image_data,
            cape_image_data,
            geometry_data,
            geometry_data_min_engine_version,
            animation_data,
            cape_id,
            full_id,
            arm_size,
            skin_color,
            persona_pieces,
            piece_tint_colors,
            is_premium,
            is_persona,
            is_persona_cape_on_classic_skin,
            is_primary_user,
            overrides_player_appearance,
            trusted_skin_flag,
            profile_hash,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerBlockProperty {
    pub block_name: wire::Str,
    pub block_definition: wire::NetworkNbt,
}

impl wire::Encode for ServerBlockProperty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_name.encode(writer)?;
        self.block_definition.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerBlockProperty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_name = reader.read_str(crate::fields::SERVER_BLOCK_PROPERTY_BLOCK_NAME)?;
        let block_definition = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            block_name,
            block_definition,
        })
    }
}

/// StoreEntryPointInfo contains information about the store entry point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerConfigurationClientStoreEntryPointConfiguration {
    /// `store_id` is the store identifier.
    pub store_id: wire::Str,
    /// `store_name` is the store name.
    pub store_name: wire::Str,
}

impl wire::Encode for ServerConfigurationClientStoreEntryPointConfiguration {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.store_id.encode(writer)?;
        self.store_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerConfigurationClientStoreEntryPointConfiguration {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let store_id = reader.read_str(
            crate::fields::SERVER_CONFIGURATION_CLIENT_STORE_ENTRY_POINT_CONFIGURATION_STORE_ID,
        )?;
        let store_name = reader.read_str(
            crate::fields::SERVER_CONFIGURATION_CLIENT_STORE_ENTRY_POINT_CONFIGURATION_STORE_NAME,
        )?;
        Ok(Self {
            store_id,
            store_name,
        })
    }
}

/// GatheringJoinInfo contains information about the gathering (experience) the player is joining.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerConfigurationGatheringsConfigurationJoinInfo {
    /// `experience_id` is the UUID of the experience.
    pub experience_id: uuid::Uuid,
    /// `experience_name` is the name of the experience.
    pub experience_name: wire::Str,
    /// ExperienceWorldID is the UUID of the experience world.
    /// Wire presence: optional value is preceded by a presence marker.
    pub world_id: Option<uuid::Uuid>,
    /// ExperienceWorldName is the world name of the experience.
    /// Wire presence: optional value is preceded by a presence marker.
    pub world_name: Option<wire::Str>,
    /// `creator_id` is the ID of the creator.
    pub creator_id: wire::Str,
    /// `target_id` is the session ID of the experience.
    /// Wire presence: optional value is preceded by a presence marker.
    pub target_id: Option<uuid::Uuid>,
    /// `scenario_id` is the scenario ID of experience.
    /// Wire presence: optional value is preceded by a presence marker.
    pub scenario_id: Option<wire::Str>,
    /// `server_id` is the server identifier.
    /// Wire presence: optional value is preceded by a presence marker.
    pub server_id: Option<wire::Str>,
}

impl wire::Encode for ServerConfigurationGatheringsConfigurationJoinInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.experience_id.encode(writer)?;
        wire::encode_string_limits(writer, &self.experience_name, 1, 29)?;
        match &self.world_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.world_name {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 1, 29)?;
            }
            None => writer.write_u8(0)?,
        }
        wire::encode_string_limits(writer, &self.creator_id, 1, 60)?;
        match &self.target_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.scenario_id {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 1, 100)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.server_id {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 1, 100)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerConfigurationGatheringsConfigurationJoinInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let experience_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let experience_name = wire::decode_string_limits(
            reader,
            crate::fields::SERVER_CONFIGURATION_GATHERINGS_CONFIGURATION_JOIN_INFO_EXPERIENCE_NAME,
            1,
            29,
        )?;
        let world_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<uuid::Uuid as wire::Decode>::decode(reader)?)
            }
        };
        let world_name = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(reader, crate::fields::SERVER_CONFIGURATION_GATHERINGS_CONFIGURATION_JOIN_INFO_WORLD_NAME, 1, 29)?)
            }
        };
        let creator_id = wire::decode_string_limits(
            reader,
            crate::fields::SERVER_CONFIGURATION_GATHERINGS_CONFIGURATION_JOIN_INFO_CREATOR_ID,
            1,
            60,
        )?;
        let target_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<uuid::Uuid as wire::Decode>::decode(reader)?)
            }
        };
        let scenario_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(reader, crate::fields::SERVER_CONFIGURATION_GATHERINGS_CONFIGURATION_JOIN_INFO_SCENARIO_ID, 1, 100)?)
            }
        };
        let server_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(reader, crate::fields::SERVER_CONFIGURATION_GATHERINGS_CONFIGURATION_JOIN_INFO_SERVER_ID, 1, 100)?)
            }
        };
        Ok(Self {
            experience_id,
            experience_name,
            world_id,
            world_name,
            creator_id,
            target_id,
            scenario_id,
            server_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerConfigurationPresenceConfiguration {
    /// Wire presence: optional value is preceded by a presence marker.
    pub rich_presence_id: Option<wire::Str>,
}

impl wire::Encode for ServerConfigurationPresenceConfiguration {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.rich_presence_id {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 0, 50)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerConfigurationPresenceConfiguration {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rich_presence_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(
                    reader,
                    crate::fields::SERVER_CONFIGURATION_PRESENCE_CONFIGURATION_RICH_PRESENCE_ID,
                    0,
                    50,
                )?)
            }
        };
        Ok(Self { rich_presence_id })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerConfigurationServerConfigurationJoinInfo {
    /// Wire presence: optional value is preceded by a presence marker.
    pub gathering: Option<ServerConfigurationGatheringsConfigurationJoinInfo>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub client_store_entry_point: Option<ServerConfigurationClientStoreEntryPointConfiguration>,
    /// Wire presence: optional value is preceded by a presence marker.
    pub presence: Option<ServerConfigurationPresenceConfiguration>,
}

impl wire::Encode for ServerConfigurationServerConfigurationJoinInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.gathering {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.client_store_entry_point {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.presence {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerConfigurationServerConfigurationJoinInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let gathering = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(
                    <ServerConfigurationGatheringsConfigurationJoinInfo as wire::Decode>::decode(
                        reader,
                    )?,
                )
            }
        };
        let client_store_entry_point = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ServerConfigurationClientStoreEntryPointConfiguration as wire::Decode>::decode(reader)?)
            }
        };
        let presence = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ServerConfigurationPresenceConfiguration as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            gathering,
            client_store_entry_point,
            presence,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ServerSoundHandle(pub u64);

impl wire::Encode for ServerSoundHandle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U64LE(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerSoundHandle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::U64LE as wire::Decode>::decode(reader)?.0))
    }
}

/// Waypoint holds optional data for a locator bar waypoint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerWaypoint {
    /// `update_flag` is a bitmask indicating which optional fields are set.
    pub update_flag: wire::U32LE,
    /// Visible determines whether the waypoint is shown.
    /// Wire presence: optional value is preceded by a presence marker.
    pub is_visible: Option<bool>,
    /// `world_position` is the position and dimension of the waypoint.
    /// Wire presence: optional value is preceded by a presence marker.
    pub world_position: Option<WorldPosition>,
    /// `texture_path` is the resource path for the waypoint icon texture.
    /// Wire presence: optional value is preceded by a presence marker.
    pub texture_path: Option<wire::Str>,
    /// `icon_size` is the size of the waypoint icon.
    /// Wire presence: optional value is preceded by a presence marker.
    pub icon_size: Option<glam::Vec2>,
    /// Colour is the RGB colour used to tint the waypoint icon.
    /// Wire presence: optional value is preceded by a presence marker.
    pub color: Option<MceColor>,
    /// `client_position_authority` determines whether the client has authority over the waypoint position.
    /// Wire presence: optional value is preceded by a presence marker.
    pub client_position_authority: Option<bool>,
    /// `actor_unique_id` is the unique ID of the entity the waypoint tracks.
    /// Wire presence: optional value is preceded by a presence marker.
    pub actor_unique_id: Option<ActorUniqueID>,
}

impl wire::Encode for ServerWaypoint {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.update_flag.encode(writer)?;
        match &self.is_visible {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.world_position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.texture_path {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.icon_size {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.color {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.client_position_authority {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.actor_unique_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerWaypoint {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let update_flag = <wire::U32LE as wire::Decode>::decode(reader)?;
        let is_visible = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let world_position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<WorldPosition as wire::Decode>::decode(reader)?)
            }
        };
        let texture_path = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::SERVER_WAYPOINT_TEXTURE_PATH)?)
            }
        };
        let icon_size = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let color = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<MceColor as wire::Decode>::decode(reader)?)
            }
        };
        let client_position_authority = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        let actor_unique_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ActorUniqueID as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            update_flag,
            is_visible,
            world_position,
            texture_path,
            icon_size,
            color,
            client_position_authority,
            actor_unique_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SocialEventsServerTelemetryData {
    pub server_id: wire::Str,
    pub scenario_id: wire::Str,
    pub world_id: wire::Str,
    pub owner_id: wire::Str,
}

impl wire::Encode for SocialEventsServerTelemetryData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.server_id.encode(writer)?;
        self.scenario_id.encode(writer)?;
        self.world_id.encode(writer)?;
        self.owner_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SocialEventsServerTelemetryData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let server_id =
            reader.read_str(crate::fields::SOCIAL_EVENTS_SERVER_TELEMETRY_DATA_SERVER_ID)?;
        let scenario_id =
            reader.read_str(crate::fields::SOCIAL_EVENTS_SERVER_TELEMETRY_DATA_SCENARIO_ID)?;
        let world_id =
            reader.read_str(crate::fields::SOCIAL_EVENTS_SERVER_TELEMETRY_DATA_WORLD_ID)?;
        let owner_id =
            reader.read_str(crate::fields::SOCIAL_EVENTS_SERVER_TELEMETRY_DATA_OWNER_ID)?;
        Ok(Self {
            server_id,
            scenario_id,
            world_id,
            owner_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnSettings {
    pub spawn_biome_type: SpawnBiomeType,
    pub user_defined_biome_name: wire::Str,
    pub dimension: wire::ZigZag32,
}

impl wire::Encode for SpawnSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.spawn_biome_type.encode(writer)?;
        self.user_defined_biome_name.encode(writer)?;
        self.dimension.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let spawn_biome_type = <SpawnBiomeType as wire::Decode>::decode(reader)?;
        let user_defined_biome_name =
            reader.read_str(crate::fields::SPAWN_SETTINGS_USER_DEFINED_BIOME_NAME)?;
        let dimension = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            spawn_biome_type,
            user_defined_biome_name,
            dimension,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncedAttribute {
    pub attribute_name: wire::Str,
    pub min_value: wire::F32LE,
    pub current_value: wire::F32LE,
    pub max_value: wire::F32LE,
}

impl wire::Encode for SyncedAttribute {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.attribute_name.encode(writer)?;
        self.min_value.encode(writer)?;
        self.current_value.encode(writer)?;
        self.max_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SyncedAttribute {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let attribute_name = reader.read_str(crate::fields::SYNCED_ATTRIBUTE_ATTRIBUTE_NAME)?;
        let min_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let current_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let max_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            attribute_name,
            min_value,
            current_value,
            max_value,
        })
    }
}

/// PlayerMovementSettings represents the different server authoritative movement settings. These control how
/// the client will provide input to the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncedPlayerMovementSettings {
    /// `rewind_history_size` is the amount of history to keep at maximum.
    pub rewind_history_size: wire::ZigZag32,
    /// `server_authoritative_block_breaking` specifies if block breaking should be sent through
    /// packet.PlayerAuthInput or not.
    pub server_authoritative_block_breaking: bool,
}

impl wire::Encode for SyncedPlayerMovementSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.rewind_history_size.encode(writer)?;
        self.server_authoritative_block_breaking.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SyncedPlayerMovementSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rewind_history_size = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let server_authoritative_block_breaking = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            rewind_history_size,
            server_authoritative_block_breaking,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SynchedActorDataCopyableDataList {
    pub data: Vec<DataItemEntry>,
}

impl wire::Encode for SynchedActorDataCopyableDataList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.data.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for SynchedActorDataCopyableDataList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let data = wire::decode_collection::<DataItemEntry>(
            reader,
            crate::fields::SYNCHED_ACTOR_DATA_COPYABLE_DATA_LIST_DATA,
            4,
        )?;
        Ok(Self { data })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TintMapColor {
    pub colors: [MceColor; 4],
}

impl wire::Encode for TintMapColor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        for item in self.colors.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for TintMapColor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let colors = [
            <MceColor as wire::Decode>::decode(reader)?,
            <MceColor as wire::Decode>::decode(reader)?,
            <MceColor as wire::Decode>::decode(reader)?,
            <MceColor as wire::Decode>::decode(reader)?,
        ];
        Ok(Self { colors })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSubChunkBlocksChangedInfo {
    pub blocks_changed_standards: Vec<UpdateSubChunkNetworkBlockInfo>,
    pub blocks_changed_extras: Vec<UpdateSubChunkNetworkBlockInfo>,
}

impl wire::Encode for UpdateSubChunkBlocksChangedInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.blocks_changed_standards.as_slice())?;
        wire::encode_collection(writer, self.blocks_changed_extras.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for UpdateSubChunkBlocksChangedInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let blocks_changed_standards = wire::decode_collection::<UpdateSubChunkNetworkBlockInfo>(
            reader,
            crate::fields::UPDATE_SUB_CHUNK_BLOCKS_CHANGED_INFO_BLOCKS_CHANGED_STANDARDS,
            7,
        )?;
        let blocks_changed_extras = wire::decode_collection::<UpdateSubChunkNetworkBlockInfo>(
            reader,
            crate::fields::UPDATE_SUB_CHUNK_BLOCKS_CHANGED_INFO_BLOCKS_CHANGED_EXTRAS,
            7,
        )?;
        Ok(Self {
            blocks_changed_standards,
            blocks_changed_extras,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSubChunkNetworkBlockInfo {
    pub pos: BlockPos,
    pub runtime_id: wire::VarUInt,
    pub update_flags: wire::VarUInt,
    pub sync_message_entity_unique_id: wire::VarULong,
    pub sync_message_message: wire::VarUInt,
}

impl wire::Encode for UpdateSubChunkNetworkBlockInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos.encode(writer)?;
        self.runtime_id.encode(writer)?;
        self.update_flags.encode(writer)?;
        self.sync_message_entity_unique_id.encode(writer)?;
        self.sync_message_message.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateSubChunkNetworkBlockInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos = <BlockPos as wire::Decode>::decode(reader)?;
        let runtime_id = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let update_flags = <wire::VarUInt as wire::Decode>::decode(reader)?;
        let sync_message_entity_unique_id = <wire::VarULong as wire::Decode>::decode(reader)?;
        let sync_message_message = <wire::VarUInt as wire::Decode>::decode(reader)?;
        Ok(Self {
            pos,
            runtime_id,
            update_flags,
            sync_message_entity_unique_id,
            sync_message_message,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebSocketData {
    /// ServerURI is the URI to make the client connect to. It can be, for example, 'localhost:8000/ws' to connect
    /// to a websocket server on the localhost at port 8000.
    pub websocket_server_uri: wire::Str,
}

impl wire::Encode for WebSocketData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.websocket_server_uri.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for WebSocketData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let websocket_server_uri =
            reader.read_str(crate::fields::WEB_SOCKET_DATA_WEBSOCKET_SERVER_URI)?;
        Ok(Self {
            websocket_server_uri,
        })
    }
}

// Domain: pack

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackIDVersionData {
    pub pack_uuid: uuid::Uuid,
    pub pack_version: SemVersionData,
}

impl wire::Encode for PackIDVersionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pack_uuid.encode(writer)?;
        self.pack_version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PackIDVersionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pack_uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let pack_version = <SemVersionData as wire::Decode>::decode(reader)?;
        Ok(Self {
            pack_uuid,
            pack_version,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackIdVersion {
    pub pack_uuid: uuid::Uuid,
    pub pack_version: SemVersion,
}

impl wire::Encode for PackIdVersion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pack_uuid.encode(writer)?;
        self.pack_version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PackIdVersion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pack_uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let pack_version = <SemVersion as wire::Decode>::decode(reader)?;
        Ok(Self {
            pack_uuid,
            pack_version,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackInfoData {
    pub pack_id_version: PackIDVersionData,
    pub pack_size: wire::U64LE,
    pub content_key: wire::Str,
    pub subpack_name: wire::Str,
    pub content_identity: ContentIdentity,
    pub has_scripts: bool,
    pub is_addon_pack: bool,
    pub is_ray_tracing_capable: bool,
    pub cdn_url: wire::Str,
}

impl wire::Encode for PackInfoData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pack_id_version.encode(writer)?;
        self.pack_size.encode(writer)?;
        self.content_key.encode(writer)?;
        self.subpack_name.encode(writer)?;
        self.content_identity.encode(writer)?;
        self.has_scripts.encode(writer)?;
        self.is_addon_pack.encode(writer)?;
        self.is_ray_tracing_capable.encode(writer)?;
        self.cdn_url.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PackInfoData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pack_id_version = <PackIDVersionData as wire::Decode>::decode(reader)?;
        let pack_size = <wire::U64LE as wire::Decode>::decode(reader)?;
        let content_key = reader.read_str(crate::fields::PACK_INFO_DATA_CONTENT_KEY)?;
        let subpack_name = reader.read_str(crate::fields::PACK_INFO_DATA_SUBPACK_NAME)?;
        let content_identity = <ContentIdentity as wire::Decode>::decode(reader)?;
        let has_scripts = <bool as wire::Decode>::decode(reader)?;
        let is_addon_pack = <bool as wire::Decode>::decode(reader)?;
        let is_ray_tracing_capable = <bool as wire::Decode>::decode(reader)?;
        let cdn_url = reader.read_str(crate::fields::PACK_INFO_DATA_CDN_URL)?;
        Ok(Self {
            pack_id_version,
            pack_size,
            content_key,
            subpack_name,
            content_identity,
            has_scripts,
            is_addon_pack,
            is_ray_tracing_capable,
            cdn_url,
        })
    }
}

/// StackResourcePack represents a resource pack sent on the stack of the client. When sent, the client will
/// apply them in the order of the stack sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackInstanceId {
    /// UUID is the UUID of the resource pack. Each resource pack downloaded must have a different UUID in order
    /// for the client to be able to handle them properly.
    pub pack_id: wire::Str,
    /// `version` is the version of the resource pack. The client will cache resource packs sent by the server as
    /// long as they carry the same version. Sending a resource pack with a different version than previously will
    /// force the client to re-download it.
    pub version: wire::Str,
    /// `sub_pack_name` ...
    pub sub_pack_name: wire::Str,
}

impl wire::Encode for PackInstanceId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pack_id.encode(writer)?;
        self.version.encode(writer)?;
        self.sub_pack_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PackInstanceId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pack_id = reader.read_str(crate::fields::PACK_INSTANCE_ID_PACK_ID)?;
        let version = reader.read_str(crate::fields::PACK_INSTANCE_ID_VERSION)?;
        let sub_pack_name = reader.read_str(crate::fields::PACK_INSTANCE_ID_SUB_PACK_NAME)?;
        Ok(Self {
            pack_id,
            version,
            sub_pack_name,
        })
    }
}

// Domain: player

/// PlayerBlockAction ...
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerBlockActionData {
    /// Action is the action to be performed, and is one of the constants listed above.
    pub player_action_type: PlayerActionType,
    /// BlockPos is the position of the block that was interacted with.
    pub position: BlockPos,
    /// Face is the face of the block that was interacted with.
    pub facing: wire::ZigZag32,
}

impl wire::Encode for PlayerBlockActionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_action_type.encode(writer)?;
        self.position.encode(writer)?;
        self.facing.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerBlockActionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_action_type = <PlayerActionType as wire::Decode>::decode(reader)?;
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let facing = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_action_type,
            position,
            facing,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerInputTick(pub u64);

impl wire::Encode for PlayerInputTick {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarULong(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerInputTick {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::VarULong as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerListData {
    RemoveEntry {
        action: PlayerListPacketType,
        uuid: uuid::Uuid,
    },
    AddEntry {
        action: PlayerListPacketType,
        uuid: uuid::Uuid,
        actor_unique_id: ActorUniqueID,
        player_name: wire::Str,
        xbl_xuid: wire::Str,
        platform_online_id: wire::Str,
        build_platform: BuildPlatform,
        serialized_skin: Box<SerializedSkinRef>,
        is_teacher: bool,
        is_host: bool,
        is_sub_client: bool,
        player_color: MceColor,
    },
}

impl PlayerListData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::RemoveEntry { .. } => 0,
            Self::AddEntry { .. } => 1,
        }
    }
}

impl Default for PlayerListData {
    fn default() -> Self {
        Self::RemoveEntry {
            action: Default::default(),
            uuid: Default::default(),
        }
    }
}

impl wire::Encode for PlayerListData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::RemoveEntry { action, uuid } => {
                action.encode(writer)?;
                uuid.encode(writer)?;
            }
            Self::AddEntry {
                action,
                uuid,
                actor_unique_id,
                player_name,
                xbl_xuid,
                platform_online_id,
                build_platform,
                serialized_skin,
                is_teacher,
                is_host,
                is_sub_client,
                player_color,
            } => {
                action.encode(writer)?;
                uuid.encode(writer)?;
                actor_unique_id.encode(writer)?;
                player_name.encode(writer)?;
                xbl_xuid.encode(writer)?;
                platform_online_id.encode(writer)?;
                build_platform.encode(writer)?;
                serialized_skin.encode(writer)?;
                is_teacher.encode(writer)?;
                is_host.encode(writer)?;
                is_sub_client.encode(writer)?;
                player_color.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for PlayerListData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let action = <PlayerListPacketType as wire::Decode>::decode(reader)?;
                let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
                Self::RemoveEntry { action, uuid }
            }
            1 => {
                let action = <PlayerListPacketType as wire::Decode>::decode(reader)?;
                let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
                let actor_unique_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
                let player_name =
                    reader.read_str(crate::fields::PLAYER_LIST_DATA_ADD_ENTRY_PLAYER_NAME)?;
                let xbl_xuid =
                    reader.read_str(crate::fields::PLAYER_LIST_DATA_ADD_ENTRY_XBL_XUID)?;
                let platform_online_id = reader
                    .read_str(crate::fields::PLAYER_LIST_DATA_ADD_ENTRY_PLATFORM_ONLINE_ID)?;
                let build_platform = <BuildPlatform as wire::Decode>::decode(reader)?;
                let serialized_skin =
                    Box::new(<SerializedSkinRef as wire::Decode>::decode(reader)?);
                let is_teacher = <bool as wire::Decode>::decode(reader)?;
                let is_host = <bool as wire::Decode>::decode(reader)?;
                let is_sub_client = <bool as wire::Decode>::decode(reader)?;
                let player_color = <MceColor as wire::Decode>::decode(reader)?;
                Self::AddEntry {
                    action,
                    uuid,
                    actor_unique_id,
                    player_name,
                    xbl_xuid,
                    platform_online_id,
                    build_platform,
                    serialized_skin,
                    is_teacher,
                    is_host,
                    is_sub_client,
                    player_color,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "PlayerListData",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerLocationData {
    CoordinatesLocation {
        packet_type: PlayerLocationType,
        position: glam::Vec3,
    },
    HiddenLocation {
        packet_type: PlayerLocationType,
    },
}

impl PlayerLocationData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::CoordinatesLocation { .. } => 0,
            Self::HiddenLocation { .. } => 1,
        }
    }
}

impl Default for PlayerLocationData {
    fn default() -> Self {
        Self::CoordinatesLocation {
            packet_type: Default::default(),
            position: Default::default(),
        }
    }
}

impl wire::Encode for PlayerLocationData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::CoordinatesLocation {
                packet_type,
                position,
            } => {
                packet_type.encode(writer)?;
                position.encode(writer)?;
            }
            Self::HiddenLocation { packet_type } => {
                packet_type.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for PlayerLocationData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let packet_type = <PlayerLocationType as wire::Decode>::decode(reader)?;
                let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
                Self::CoordinatesLocation {
                    packet_type,
                    position,
                }
            }
            1 => {
                let packet_type = <PlayerLocationType as wire::Decode>::decode(reader)?;
                Self::HiddenLocation { packet_type }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "PlayerLocationData",
                    value: value as i64,
                });
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerPartyInfo {
    pub party_id: wire::Str,
    pub is_party_leader: bool,
}

impl wire::Encode for PlayerPartyInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.party_id, 0, 49)?;
        self.is_party_leader.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerPartyInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let party_id =
            wire::decode_string_limits(reader, crate::fields::PLAYER_PARTY_INFO_PARTY_ID, 0, 49)?;
        let is_party_leader = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            party_id,
            is_party_leader,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerScoreboardId(pub i64);

impl wire::Encode for PlayerScoreboardId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag64(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerScoreboardId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag64 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerVideoCaptureData {
    StartVideoCapture {
        frame_rate: wire::U32LE,
        file_prefix: wire::Str,
    },
    StopVideoCapture,
}

impl PlayerVideoCaptureData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::StartVideoCapture { .. } => 0,
            Self::StopVideoCapture => 1,
        }
    }
}

impl Default for PlayerVideoCaptureData {
    fn default() -> Self {
        Self::StartVideoCapture {
            frame_rate: Default::default(),
            file_prefix: Default::default(),
        }
    }
}

impl wire::Encode for PlayerVideoCaptureData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::StartVideoCapture {
                frame_rate,
                file_prefix,
            } => {
                frame_rate.encode(writer)?;
                file_prefix.encode(writer)?;
            }
            Self::StopVideoCapture => {}
        }
        Ok(())
    }
}

impl wire::Decode for PlayerVideoCaptureData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let frame_rate = <wire::U32LE as wire::Decode>::decode(reader)?;
                let file_prefix = reader.read_str(
                    crate::fields::PLAYER_VIDEO_CAPTURE_DATA_START_VIDEO_CAPTURE_FILE_PREFIX,
                )?;
                Self::StartVideoCapture {
                    frame_rate,
                    file_prefix,
                }
            }
            1 => Self::StopVideoCapture,
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "PlayerVideoCaptureData",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: position_tracking

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PositionTrackingId(pub i32);

impl wire::Encode for PositionTrackingId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag32(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag32 as wire::Decode>::decode(reader)?.0))
    }
}

// Domain: recipe

/// MultiRecipe serves as an 'enable' switch for multi-shape recipes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MultiRecipe {
    /// UUID is a UUID identifying the recipe. Since the CraftingEvent packet no longer exists, this can always be
    /// empty.
    pub multi_recipe_uuid: uuid::Uuid,
    pub net_id: RecipeNetID,
}

impl wire::Encode for MultiRecipe {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.multi_recipe_uuid.encode(writer)?;
        self.net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MultiRecipe {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let multi_recipe_uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            multi_recipe_uuid,
            net_id,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecipeIngredient {
    pub item_descriptor: ItemDescriptor,
    pub stack_size: wire::U16LE,
}

impl wire::Encode for RecipeIngredient {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_descriptor.encode(writer)?;
        self.stack_size.encode(writer)?;
        wire::check_number_limits(self.stack_size.0, Some(1), Some(65535))?;
        Ok(())
    }
}

impl wire::Decode for RecipeIngredient {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_descriptor = <ItemDescriptor as wire::Decode>::decode(reader)?;
        let stack_size = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(1), Some(65535))?;
            value
        };
        Ok(Self {
            item_descriptor,
            stack_size,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecipeIngredientSerializedData {
    pub descriptor: Vec<(wire::Str, wire::Str)>,
    pub aux_value: wire::ZigZag32,
    pub stack_size: wire::ZigZag32,
}

impl wire::Encode for RecipeIngredientSerializedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.descriptor.len())?;
        for (key, value) in self.descriptor.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
        }
        self.aux_value.encode(writer)?;
        self.stack_size.encode(writer)?;
        wire::check_number_limits(self.stack_size.0, Some(0), Some(64))?;
        Ok(())
    }
}

impl wire::Decode for RecipeIngredientSerializedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let descriptor = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::RECIPE_INGREDIENT_SERIALIZED_DATA_DESCRIPTOR,
                declared,
                2,
            )?;
            let mut out: Vec<(wire::Str, wire::Str)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader
                    .read_str(crate::fields::RECIPE_INGREDIENT_SERIALIZED_DATA_DESCRIPTOR_KEY)?;
                let value = reader
                    .read_str(crate::fields::RECIPE_INGREDIENT_SERIALIZED_DATA_DESCRIPTOR_VALUE)?;
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let aux_value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let stack_size = {
            let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(64))?;
            value
        };
        Ok(Self {
            descriptor,
            aux_value,
            stack_size,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RecipeNetID(pub u32);

impl wire::Encode for RecipeNetID {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RecipeNetID {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::VarUInt as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecipeUnlockRequirementSerializedData {
    pub unlocking_context: RecipeUnlockingRequirementUnlockingContext,
    /// Wire presence: optional value is preceded by a presence marker.
    pub unlocking_ingredients: Option<Vec<RecipeIngredientSerializedData>>,
}

impl wire::Encode for RecipeUnlockRequirementSerializedData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.unlocking_context.encode(writer)?;
        match &self.unlocking_ingredients {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 128)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for RecipeUnlockRequirementSerializedData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let unlocking_context =
            <RecipeUnlockingRequirementUnlockingContext as wire::Decode>::decode(reader)?;
        let unlocking_ingredients = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection_limits::<
                    RecipeIngredientSerializedData,
                >(
                    reader,
                    crate::fields::RECIPE_UNLOCK_REQUIREMENT_SERIALIZED_DATA_UNLOCKING_INGREDIENTS,
                    3,
                    0,
                    128,
                )?)
            }
        };
        Ok(Self {
            unlocking_context,
            unlocking_ingredients,
        })
    }
}

/// ShapedRecipe is a recipe that has a specific shape that must be used to craft the output of the recipe.
/// Trying to craft the item in any other shape will not work. The ShapedRecipe is of the same structure as
/// the ShapedChemistryRecipe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRecipe {
    /// `recipe_id` is a unique ID of the recipe. This ID must be unique amongst all other types of recipes too,
    /// but its functionality is not exactly known.
    pub recipe_id: wire::Str,
    /// `width` is the width of the recipe's shape.
    pub width: wire::ZigZag32,
    /// `height` is the height of the recipe's shape.
    pub height: wire::ZigZag32,
    pub ingredients: Vec<RecipeIngredientSerializedData>,
    pub results: Vec<NetworkItemInstanceDescriptorSerializedData>,
    /// `uuid` is a UUID identifying the recipe. Since the CraftingEvent packet no longer exists, this can always
    /// be empty.
    pub uuid: uuid::Uuid,
    pub tag: wire::Str,
    /// `priority` ...
    pub priority: wire::ZigZag32,
    /// `assume_symmetry` specifies if the recipe is symmetrical. If this is set to true, the recipe will be
    /// mirrored along the diagonal axis. This means that the recipe will be the same if rotated 180 degrees.
    pub assume_symmetry: bool,
    /// Wire presence: optional value is preceded by a presence marker.
    pub unlocking_requirement: Option<RecipeUnlockRequirementSerializedData>,
    pub net_id: RecipeNetID,
}

impl wire::Encode for ShapedRecipe {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.recipe_id.encode(writer)?;
        self.width.encode(writer)?;
        self.height.encode(writer)?;
        wire::encode_collection_limits(writer, self.ingredients.as_slice(), 0, 128)?;
        wire::encode_collection(writer, self.results.as_slice())?;
        self.uuid.encode(writer)?;
        self.tag.encode(writer)?;
        self.priority.encode(writer)?;
        self.assume_symmetry.encode(writer)?;
        match &self.unlocking_requirement {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShapedRecipe {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let recipe_id = reader.read_str(crate::fields::SHAPED_RECIPE_RECIPE_ID)?;
        let width = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let height = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let ingredients = wire::decode_collection_limits::<RecipeIngredientSerializedData>(
            reader,
            crate::fields::SHAPED_RECIPE_INGREDIENTS,
            3,
            0,
            128,
        )?;
        let results = wire::decode_collection::<NetworkItemInstanceDescriptorSerializedData>(
            reader,
            crate::fields::SHAPED_RECIPE_RESULTS,
            6,
        )?;
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let tag = reader.read_str(crate::fields::SHAPED_RECIPE_TAG)?;
        let priority = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let assume_symmetry = <bool as wire::Decode>::decode(reader)?;
        let unlocking_requirement = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<RecipeUnlockRequirementSerializedData as wire::Decode>::decode(reader)?)
            }
        };
        let net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            recipe_id,
            width,
            height,
            ingredients,
            results,
            uuid,
            tag,
            priority,
            assume_symmetry,
            unlocking_requirement,
            net_id,
        })
    }
}

/// ShapelessRecipe is a recipe that has no particular shape. Its functionality is shared with the
/// RecipeShulkerBox and RecipeShapelessChemistry types.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapelessRecipe {
    /// `recipe_id` is a unique ID of the recipe. This ID must be unique amongst all other types of recipes too,
    /// but its functionality is not exactly known.
    pub recipe_id: wire::Str,
    pub ingredients: Vec<RecipeIngredientSerializedData>,
    pub results: Vec<NetworkItemInstanceDescriptorSerializedData>,
    /// `uuid` is a UUID identifying the recipe. Since the CraftingEvent packet no longer exists, this can always
    /// be empty.
    pub uuid: uuid::Uuid,
    pub tag: wire::Str,
    /// `priority` ...
    pub priority: wire::ZigZag32,
    /// Wire presence: optional value is preceded by a presence marker.
    pub unlocking_requirement: Option<RecipeUnlockRequirementSerializedData>,
    pub net_id: RecipeNetID,
}

impl wire::Encode for ShapelessRecipe {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.recipe_id.encode(writer)?;
        wire::encode_collection_limits(writer, self.ingredients.as_slice(), 0, 128)?;
        wire::encode_collection(writer, self.results.as_slice())?;
        self.uuid.encode(writer)?;
        self.tag.encode(writer)?;
        self.priority.encode(writer)?;
        match &self.unlocking_requirement {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShapelessRecipe {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let recipe_id = reader.read_str(crate::fields::SHAPELESS_RECIPE_RECIPE_ID)?;
        let ingredients = wire::decode_collection_limits::<RecipeIngredientSerializedData>(
            reader,
            crate::fields::SHAPELESS_RECIPE_INGREDIENTS,
            3,
            0,
            128,
        )?;
        let results = wire::decode_collection::<NetworkItemInstanceDescriptorSerializedData>(
            reader,
            crate::fields::SHAPELESS_RECIPE_RESULTS,
            6,
        )?;
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let tag = reader.read_str(crate::fields::SHAPELESS_RECIPE_TAG)?;
        let priority = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let unlocking_requirement = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<RecipeUnlockRequirementSerializedData as wire::Decode>::decode(reader)?)
            }
        };
        let net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            recipe_id,
            ingredients,
            results,
            uuid,
            tag,
            priority,
            unlocking_requirement,
            net_id,
        })
    }
}

/// SmithingTransformRecipe is a recipe specifically used for smithing tables. It has three input items and
/// adds them together, resulting in a new item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmithingTransformRecipe {
    /// `recipe_id` is a unique ID of the recipe. This ID must be unique amongst all other types of recipes too,
    /// but its functionality is not exactly known.
    pub recipe_id: wire::Str,
    pub template_ingredient: RecipeIngredientSerializedData,
    pub base_ingredient: RecipeIngredientSerializedData,
    pub addition_ingredient: RecipeIngredientSerializedData,
    /// `result` is the resulting item from the two items being added together.
    pub result: NetworkItemInstanceDescriptorSerializedData,
    pub tag: wire::Str,
    pub net_id: RecipeNetID,
}

impl wire::Encode for SmithingTransformRecipe {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.recipe_id.encode(writer)?;
        self.template_ingredient.encode(writer)?;
        self.base_ingredient.encode(writer)?;
        self.addition_ingredient.encode(writer)?;
        self.result.encode(writer)?;
        self.tag.encode(writer)?;
        self.net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SmithingTransformRecipe {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let recipe_id = reader.read_str(crate::fields::SMITHING_TRANSFORM_RECIPE_RECIPE_ID)?;
        let template_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let base_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let addition_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let result = <NetworkItemInstanceDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let tag = reader.read_str(crate::fields::SMITHING_TRANSFORM_RECIPE_TAG)?;
        let net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            recipe_id,
            template_ingredient,
            base_ingredient,
            addition_ingredient,
            result,
            tag,
            net_id,
        })
    }
}

/// SmithingTrimRecipe is a recipe specifically used for applying armour trims to an armour piece inside a
/// smithing table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmithingTrimRecipe {
    /// `recipe_id` is a unique ID of the recipe. This ID must be unique amongst all other types of recipes too,
    /// but its functionality is not exactly known.
    pub recipe_id: wire::Str,
    pub template_ingredient: RecipeIngredientSerializedData,
    pub base_ingredient: RecipeIngredientSerializedData,
    pub addition_ingredient: RecipeIngredientSerializedData,
    pub tag: wire::Str,
    pub net_id: RecipeNetID,
}

impl wire::Encode for SmithingTrimRecipe {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.recipe_id.encode(writer)?;
        self.template_ingredient.encode(writer)?;
        self.base_ingredient.encode(writer)?;
        self.addition_ingredient.encode(writer)?;
        self.tag.encode(writer)?;
        self.net_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SmithingTrimRecipe {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let recipe_id = reader.read_str(crate::fields::SMITHING_TRIM_RECIPE_RECIPE_ID)?;
        let template_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let base_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let addition_ingredient = <RecipeIngredientSerializedData as wire::Decode>::decode(reader)?;
        let tag = reader.read_str(crate::fields::SMITHING_TRIM_RECIPE_TAG)?;
        let net_id = <RecipeNetID as wire::Decode>::decode(reader)?;
        Ok(Self {
            recipe_id,
            template_ingredient,
            base_ingredient,
            addition_ingredient,
            tag,
            net_id,
        })
    }
}

// Domain: resource_pack

#[derive(Clone, Debug, PartialEq)]
pub enum ResourcePackClientResponseData {
    Cancel {
        response_type: wire::Str,
    },
    Downloading {
        response_type: wire::Str,
        downloading_packs: Vec<wire::Str>,
    },
    DownloadingFinished {
        response_type: wire::Str,
    },
    ResourcePackStackFinished {
        response_type: wire::Str,
    },
}

impl ResourcePackClientResponseData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Cancel { .. } => 0,
            Self::Downloading { .. } => 1,
            Self::DownloadingFinished { .. } => 2,
            Self::ResourcePackStackFinished { .. } => 3,
        }
    }
}

impl Default for ResourcePackClientResponseData {
    fn default() -> Self {
        Self::Cancel {
            response_type: Default::default(),
        }
    }
}

impl wire::Encode for ResourcePackClientResponseData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Cancel { response_type } => {
                response_type.encode(writer)?;
            }
            Self::Downloading {
                response_type,
                downloading_packs,
            } => {
                response_type.encode(writer)?;
                wire::check_length(downloading_packs.len(), 0, 65535)?;
                writer.write_len_var_u32(downloading_packs.len())?;
                for item in downloading_packs.iter() {
                    item.encode(writer)?;
                }
            }
            Self::DownloadingFinished { response_type } => {
                response_type.encode(writer)?;
            }
            Self::ResourcePackStackFinished { response_type } => {
                response_type.encode(writer)?;
            }
        }
        Ok(())
    }
}

impl wire::Decode for ResourcePackClientResponseData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let response_type = reader.read_str(
                    crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_CANCEL_RESPONSE_TYPE,
                )?;
                Self::Cancel { response_type }
            }
            1 => {
                let response_type = reader.read_str(
                    crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_DOWNLOADING_RESPONSE_TYPE,
                )?;
                let downloading_packs = {
                    let declared = u64::from(reader.read_var_u32()?);
                    let count = reader.checked_count_limits(crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_DOWNLOADING_DOWNLOADING_PACKS, declared, 1, 0, 65535)?;
                    let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
                    for _ in 0..count {
                        wire::push(&mut out, reader.read_str(crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_DOWNLOADING_DOWNLOADING_PACKS_ITEM)?)?;
                    }
                    out
                };
                Self::Downloading {
                    response_type,
                    downloading_packs,
                }
            }
            2 => {
                let response_type = reader.read_str(crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_DOWNLOADING_FINISHED_RESPONSE_TYPE)?;
                Self::DownloadingFinished { response_type }
            }
            3 => {
                let response_type = reader.read_str(crate::fields::RESOURCE_PACK_CLIENT_RESPONSE_DATA_RESOURCE_PACK_STACK_FINISHED_RESPONSE_TYPE)?;
                Self::ResourcePackStackFinished { response_type }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "ResourcePackClientResponseData",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: scoreboard

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScoreboardId(pub i64);

impl wire::Encode for ScoreboardId {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::ZigZag64(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScoreboardId {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::ZigZag64 as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScoreboardIdentityPacketInfo {
    pub scoreboard_id: ScoreboardId,
    /// Wire presence: optional value is preceded by a presence marker.
    pub player_unique_id: Option<wire::ZigZag64>,
}

impl wire::Encode for ScoreboardIdentityPacketInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.scoreboard_id.encode(writer)?;
        match &self.player_unique_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ScoreboardIdentityPacketInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let scoreboard_id = <ScoreboardId as wire::Decode>::decode(reader)?;
        let player_unique_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag64 as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            scoreboard_id,
            player_unique_id,
        })
    }
}

// Domain: shape

/// PrimitiveShape defines a single shape to be rendered on the client. Each shape has a unique NetworkID and
/// a set of optional parameters depending on its type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveShape {
    /// `network_id` is the network ID of the shape.
    pub network_id: wire::VarULong,
    /// `shape_type` is the optional dimension ID where the shape is rendered.
    /// Wire presence: optional value is preceded by a presence marker.
    pub shape_type: Option<ScriptModuleMinecraftScriptPrimitiveShapeType>,
    /// `location` is the location of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub location: Option<glam::Vec3>,
    /// `scale` is the scale of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub scale: Option<wire::F32LE>,
    /// `rotation` is the rotation of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub rotation: Option<glam::Vec3>,
    /// `total_time_left` is the total time left of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub total_time_left: Option<wire::F32LE>,
    /// `maximum_render_distance` is the rotation of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub maximum_render_distance: Option<wire::F32LE>,
    /// `color` is the total time left of the shape.
    /// Wire presence: optional value is preceded by a presence marker.
    pub color: Option<MceColor>,
    /// `dimension_id` is the optional dimension ID where the shape is rendered.
    /// Wire presence: optional value is preceded by a presence marker.
    pub dimension_id: Option<DimensionType>,
    /// `attached_to_entity_id` is the optional runtime ID of the entity the shape is attached to.
    /// Wire presence: optional value is preceded by a presence marker.
    pub attached_to_entity_id: Option<ActorUniqueID>,
    /// `extra_shape_data` holding data specific to the type of shape (such as text string for the text shape).
    pub extra_shape_data: PrimitiveShapeExtraShapeData,
}

impl wire::Encode for PrimitiveShape {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.network_id.encode(writer)?;
        match &self.shape_type {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.location {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.scale {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.rotation {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.total_time_left {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.maximum_render_distance {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.color {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.dimension_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.attached_to_entity_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.extra_shape_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PrimitiveShape {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let network_id = <wire::VarULong as wire::Decode>::decode(reader)?;
        let shape_type = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(
                    <ScriptModuleMinecraftScriptPrimitiveShapeType as wire::Decode>::decode(
                        reader,
                    )?,
                )
            }
        };
        let location = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let scale = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let rotation = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let total_time_left = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let maximum_render_distance = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let color = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<MceColor as wire::Decode>::decode(reader)?)
            }
        };
        let dimension_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<DimensionType as wire::Decode>::decode(reader)?)
            }
        };
        let attached_to_entity_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ActorUniqueID as wire::Decode>::decode(reader)?)
            }
        };
        let extra_shape_data = <PrimitiveShapeExtraShapeData as wire::Decode>::decode(reader)?;
        Ok(Self {
            network_id,
            shape_type,
            location,
            scale,
            rotation,
            total_time_left,
            maximum_render_distance,
            color,
            dimension_id,
            attached_to_entity_id,
            extra_shape_data,
        })
    }
}

// Domain: skin

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinImage {
    pub width: wire::U32LE,
    pub height: wire::U32LE,
    pub image_bytes: bytes::Bytes,
}

impl wire::Encode for SkinImage {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.width.encode(writer)?;
        wire::check_number_limits(self.width.0, None, Some(4096))?;
        self.height.encode(writer)?;
        wire::check_number_limits(self.height.0, None, Some(4096))?;
        wire::encode_bytes_limits(writer, self.image_bytes.as_ref(), 0, 67108864)?;
        Ok(())
    }
}

impl wire::Decode for SkinImage {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let width = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(4096))?;
            value
        };
        let height = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(4096))?;
            value
        };
        let image_bytes =
            wire::decode_bytes_limits(reader, crate::fields::SKIN_IMAGE_IMAGE_BYTES, 0, 67108864)?;
        Ok(Self {
            width,
            height,
            image_bytes,
        })
    }
}

// Domain: sound

#[derive(Clone, Debug, PartialEq, Default)]
pub enum SoundDataEvent {
    #[default]
    Stop,
    SetVolume {
        volume: wire::F32LE,
    },
    SetPitch {
        pitch: wire::F32LE,
    },
    Fade {
        duration: wire::F32LE,
        target_volume: wire::F32LE,
    },
    SeekTo {
        seconds: wire::F32LE,
    },
    Pause,
    Resume,
}

impl SoundDataEvent {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::Stop => 0,
            Self::SetVolume { .. } => 1,
            Self::SetPitch { .. } => 2,
            Self::Fade { .. } => 3,
            Self::SeekTo { .. } => 4,
            Self::Pause => 5,
            Self::Resume => 6,
        }
    }
}

impl wire::Encode for SoundDataEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::Stop => {}
            Self::SetVolume { volume } => {
                volume.encode(writer)?;
            }
            Self::SetPitch { pitch } => {
                pitch.encode(writer)?;
            }
            Self::Fade {
                duration,
                target_volume,
            } => {
                duration.encode(writer)?;
                target_volume.encode(writer)?;
            }
            Self::SeekTo { seconds } => {
                seconds.encode(writer)?;
            }
            Self::Pause => {}
            Self::Resume => {}
        }
        Ok(())
    }
}

impl wire::Decode for SoundDataEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => Self::Stop,
            1 => {
                let volume = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::SetVolume { volume }
            }
            2 => {
                let pitch = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::SetPitch { pitch }
            }
            3 => {
                let duration = <wire::F32LE as wire::Decode>::decode(reader)?;
                let target_volume = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::Fade {
                    duration,
                    target_volume,
                }
            }
            4 => {
                let seconds = <wire::F32LE as wire::Decode>::decode(reader)?;
                Self::SeekTo { seconds }
            }
            5 => Self::Pause,
            6 => Self::Resume,
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "SoundDataEvent",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: structure

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureEditorData {
    pub structure_name: wire::Str,
    pub filtered_structure_name: wire::Str,
    pub data_field: wire::Str,
    pub should_include_players: bool,
    pub should_show_bounding_box: bool,
    pub structure_block_type: StructureBlockType,
    pub structure_settings: StructureSettings,
    pub redstone_save_mode: StructureRedstoneSaveMode,
}

impl wire::Encode for StructureEditorData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.structure_name.encode(writer)?;
        self.filtered_structure_name.encode(writer)?;
        self.data_field.encode(writer)?;
        self.should_include_players.encode(writer)?;
        self.should_show_bounding_box.encode(writer)?;
        self.structure_block_type.encode(writer)?;
        self.structure_settings.encode(writer)?;
        self.redstone_save_mode.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureEditorData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let structure_name =
            reader.read_str(crate::fields::STRUCTURE_EDITOR_DATA_STRUCTURE_NAME)?;
        let filtered_structure_name =
            reader.read_str(crate::fields::STRUCTURE_EDITOR_DATA_FILTERED_STRUCTURE_NAME)?;
        let data_field = reader.read_str(crate::fields::STRUCTURE_EDITOR_DATA_DATA_FIELD)?;
        let should_include_players = <bool as wire::Decode>::decode(reader)?;
        let should_show_bounding_box = <bool as wire::Decode>::decode(reader)?;
        let structure_block_type = <StructureBlockType as wire::Decode>::decode(reader)?;
        let structure_settings = <StructureSettings as wire::Decode>::decode(reader)?;
        let redstone_save_mode = <StructureRedstoneSaveMode as wire::Decode>::decode(reader)?;
        Ok(Self {
            structure_name,
            filtered_structure_name,
            data_field,
            should_include_players,
            should_show_bounding_box,
            structure_block_type,
            structure_settings,
            redstone_save_mode,
        })
    }
}

/// StructureSettings is a struct holding settings of a structure block. Its fields may be changed using the
/// in-game UI on the client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureSettings {
    /// PaletteName is the name of the palette used in the structure. Currently, it seems that this field is
    /// always 'default'.
    pub structure_palette_name: wire::Str,
    /// IgnoreEntities specifies if the structure should ignore entities or include them. If set to false,
    /// entities will also show up in the exported structure.
    pub should_ignore_entities: bool,
    /// IgnoreBlocks specifies if the structure should ignore blocks or include them. If set to false, blocks will
    /// show up in the exported structure.
    pub should_ignore_blocks: bool,
    /// AllowNonTickingChunks specifies if the structure should allow non-ticking chunks. If set to false, the
    /// structure will export non-ticking chunks.
    pub should_allow_non_ticking_player_and_ticking_area_chunks: bool,
    /// Size is the size of the area that is about to be exported. The area exported will start at the Position +
    /// Offset, and will extend as far as Size specifies.
    pub structure_size: BlockPos,
    /// Offset is the offset position that was set in the structure block. The area exported is offset by this
    /// position.
    pub structure_offset: BlockPos,
    /// LastEditingPlayerUniqueID is the unique ID of the player that last edited the structure block that these
    /// settings concern.
    pub last_edit_player: ActorUniqueID,
    /// `rotation` is the rotation that the structure block should obtain. See the constants above for available
    /// options.
    pub rotation: Rotation,
    /// `mirror` specifies the way the structure should be mirrored. It is either no mirror at all, mirror on the
    /// x/z axis or both.
    pub mirror: Mirror,
    /// `animation_mode` ...
    pub animation_mode: AnimationMode,
    /// AnimationDuration ...
    pub animation_seconds: wire::F32LE,
    /// Integrity is usually 1, but may be set to a number between 0 and 1 to omit blocks randomly, using the Seed
    /// that follows.
    pub integrity_value: wire::F32LE,
    /// Seed is the seed used to omit blocks if Integrity is not equal to one. If the Seed is 0, a random seed is
    /// selected to omit blocks.
    pub integrity_seed: wire::U32LE,
    /// Pivot is the pivot around which the structure may be rotated.
    pub rotation_pivot: glam::Vec3,
}

impl wire::Encode for StructureSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.structure_palette_name, 0, 256)?;
        self.should_ignore_entities.encode(writer)?;
        self.should_ignore_blocks.encode(writer)?;
        self.should_allow_non_ticking_player_and_ticking_area_chunks
            .encode(writer)?;
        self.structure_size.encode(writer)?;
        self.structure_offset.encode(writer)?;
        self.last_edit_player.encode(writer)?;
        self.rotation.encode(writer)?;
        self.mirror.encode(writer)?;
        self.animation_mode.encode(writer)?;
        self.animation_seconds.encode(writer)?;
        self.integrity_value.encode(writer)?;
        self.integrity_seed.encode(writer)?;
        self.rotation_pivot.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let structure_palette_name = wire::decode_string_limits(
            reader,
            crate::fields::STRUCTURE_SETTINGS_STRUCTURE_PALETTE_NAME,
            0,
            256,
        )?;
        let should_ignore_entities = <bool as wire::Decode>::decode(reader)?;
        let should_ignore_blocks = <bool as wire::Decode>::decode(reader)?;
        let should_allow_non_ticking_player_and_ticking_area_chunks =
            <bool as wire::Decode>::decode(reader)?;
        let structure_size = <BlockPos as wire::Decode>::decode(reader)?;
        let structure_offset = <BlockPos as wire::Decode>::decode(reader)?;
        let last_edit_player = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let rotation = <Rotation as wire::Decode>::decode(reader)?;
        let mirror = <Mirror as wire::Decode>::decode(reader)?;
        let animation_mode = <AnimationMode as wire::Decode>::decode(reader)?;
        let animation_seconds = <wire::F32LE as wire::Decode>::decode(reader)?;
        let integrity_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        let integrity_seed = <wire::U32LE as wire::Decode>::decode(reader)?;
        let rotation_pivot = <glam::Vec3 as wire::Decode>::decode(reader)?;
        Ok(Self {
            structure_palette_name,
            should_ignore_entities,
            should_ignore_blocks,
            should_allow_non_ticking_player_and_ticking_area_chunks,
            structure_size,
            structure_offset,
            last_edit_player,
            rotation,
            mirror,
            animation_mode,
            animation_seconds,
            integrity_value,
            integrity_seed,
            rotation_pivot,
        })
    }
}

// Domain: sub_chunk

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubChunkData {
    pub sub_chunk_pos_offset: SubChunkPosOffset,
    pub sub_chunk_request_result: SubChunkRequestResult,
    /// Wire presence: optional value is preceded by a presence marker.
    pub serialized_sub_chunk: Option<bytes::Bytes>,
    pub height_map_data: HeightmapData,
    /// Wire presence: optional value is preceded by a presence marker.
    pub blob_id: Option<wire::U64LE>,
}

impl wire::Encode for SubChunkData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sub_chunk_pos_offset.encode(writer)?;
        self.sub_chunk_request_result.encode(writer)?;
        match &self.serialized_sub_chunk {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.height_map_data.encode(writer)?;
        match &self.blob_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for SubChunkData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sub_chunk_pos_offset = <SubChunkPosOffset as wire::Decode>::decode(reader)?;
        let sub_chunk_request_result = <SubChunkRequestResult as wire::Decode>::decode(reader)?;
        let serialized_sub_chunk = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_shared_bytes(crate::fields::SUB_CHUNK_DATA_SERIALIZED_SUB_CHUNK)?)
            }
        };
        let height_map_data = <HeightmapData as wire::Decode>::decode(reader)?;
        let blob_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U64LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            sub_chunk_pos_offset,
            sub_chunk_request_result,
            serialized_sub_chunk,
            height_map_data,
            blob_id,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SubChunkMetadata(pub u64);

impl wire::Encode for SubChunkMetadata {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U64LE(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkMetadata {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::U64LE as wire::Decode>::decode(reader)?.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubChunkPosOffset {
    pub subchunk_offset_x: wire::I8,
    pub subchunk_offset_y: wire::I8,
    pub subchunk_offset_z: wire::I8,
}

impl wire::Encode for SubChunkPosOffset {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.subchunk_offset_x.encode(writer)?;
        self.subchunk_offset_y.encode(writer)?;
        self.subchunk_offset_z.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkPosOffset {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let subchunk_offset_x = <wire::I8 as wire::Decode>::decode(reader)?;
        let subchunk_offset_y = <wire::I8 as wire::Decode>::decode(reader)?;
        let subchunk_offset_z = <wire::I8 as wire::Decode>::decode(reader)?;
        Ok(Self {
            subchunk_offset_x,
            subchunk_offset_y,
            subchunk_offset_z,
        })
    }
}

// Domain: text

#[derive(Clone, Debug, PartialEq)]
pub enum TextData {
    MessageOnly {
        message_type: TextPacketType,
        message: wire::Str,
    },
    AuthorAndMessage {
        message_type: TextPacketType,
        player_name: wire::Str,
        message: wire::Str,
    },
    MessageAndParams {
        message_type: TextPacketType,
        message: wire::Str,
        parameter_list: Vec<wire::Str>,
    },
}

impl TextData {
    pub fn discriminant(&self) -> u32 {
        match self {
            Self::MessageOnly { .. } => 0,
            Self::AuthorAndMessage { .. } => 1,
            Self::MessageAndParams { .. } => 2,
        }
    }
}

impl Default for TextData {
    fn default() -> Self {
        Self::MessageOnly {
            message_type: Default::default(),
            message: Default::default(),
        }
    }
}

impl wire::Encode for TextData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::VarUInt(self.discriminant()).encode(writer)?;
        match self {
            Self::MessageOnly {
                message_type,
                message,
            } => {
                message_type.encode(writer)?;
                wire::encode_string_limits(writer, message, 1, 65536)?;
            }
            Self::AuthorAndMessage {
                message_type,
                player_name,
                message,
            } => {
                message_type.encode(writer)?;
                wire::encode_string_limits(writer, player_name, 0, 256)?;
                wire::encode_string_limits(writer, message, 1, 65536)?;
            }
            Self::MessageAndParams {
                message_type,
                message,
                parameter_list,
            } => {
                message_type.encode(writer)?;
                wire::encode_string_limits(writer, message, 1, 65536)?;
                wire::check_length(parameter_list.len(), 0, 4)?;
                writer.write_len_var_u32(parameter_list.len())?;
                for item in parameter_list.iter() {
                    item.encode(writer)?;
                }
            }
        }
        Ok(())
    }
}

impl wire::Decode for TextData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let discriminant = <wire::VarUInt as wire::Decode>::decode(reader)?.0;
        Ok(match discriminant {
            0 => {
                let message_type = <TextPacketType as wire::Decode>::decode(reader)?;
                let message = wire::decode_string_limits(
                    reader,
                    crate::fields::TEXT_DATA_MESSAGE_ONLY_MESSAGE,
                    1,
                    65536,
                )?;
                Self::MessageOnly {
                    message_type,
                    message,
                }
            }
            1 => {
                let message_type = <TextPacketType as wire::Decode>::decode(reader)?;
                let player_name = wire::decode_string_limits(
                    reader,
                    crate::fields::TEXT_DATA_AUTHOR_AND_MESSAGE_PLAYER_NAME,
                    0,
                    256,
                )?;
                let message = wire::decode_string_limits(
                    reader,
                    crate::fields::TEXT_DATA_AUTHOR_AND_MESSAGE_MESSAGE,
                    1,
                    65536,
                )?;
                Self::AuthorAndMessage {
                    message_type,
                    player_name,
                    message,
                }
            }
            2 => {
                let message_type = <TextPacketType as wire::Decode>::decode(reader)?;
                let message = wire::decode_string_limits(
                    reader,
                    crate::fields::TEXT_DATA_MESSAGE_AND_PARAMS_MESSAGE,
                    1,
                    65536,
                )?;
                let parameter_list = {
                    let declared = u64::from(reader.read_var_u32()?);
                    let count = reader.checked_count_limits(
                        crate::fields::TEXT_DATA_MESSAGE_AND_PARAMS_PARAMETER_LIST,
                        declared,
                        1,
                        0,
                        4,
                    )?;
                    let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
                    for _ in 0..count {
                        wire::push(
                            &mut out,
                            reader.read_str(
                                crate::fields::TEXT_DATA_MESSAGE_AND_PARAMS_PARAMETER_LIST_ITEM,
                            )?,
                        )?;
                    }
                    out
                };
                Self::MessageAndParams {
                    message_type,
                    message,
                    parameter_list,
                }
            }
            value => {
                return Err(wire::DecodeError::UnknownVariant {
                    type_name: "TextData",
                    value: value as i64,
                });
            }
        })
    }
}

// Domain: trim

/// TrimMaterial represents a material that can be used when applying an armour trim.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrimMaterial {
    /// `material_id` is the identifier of the material, for example 'netherite'.
    pub material_id: wire::Str,
    /// `color` is the colour code used for text formatting, for example '§j'.
    pub color: wire::Str,
    /// `item_name` is the identifier of the item that represents the material, for example,
    /// 'minecraft:netherite_ingot'.
    pub item_name: wire::Str,
}

impl wire::Encode for TrimMaterial {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.material_id.encode(writer)?;
        self.color.encode(writer)?;
        self.item_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TrimMaterial {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let material_id = reader.read_str(crate::fields::TRIM_MATERIAL_MATERIAL_ID)?;
        let color = reader.read_str(crate::fields::TRIM_MATERIAL_COLOR)?;
        let item_name = reader.read_str(crate::fields::TRIM_MATERIAL_ITEM_NAME)?;
        Ok(Self {
            material_id,
            color,
            item_name,
        })
    }
}

/// TrimPattern represents a pattern that can be applied to an armour piece in combination with a
/// TrimMaterial.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrimPattern {
    /// `item_name` is the identifier of the item that represents the pattern, for example
    /// 'minecraft:wayfinder_armor_trim_smithing_template'.
    pub item_name: wire::Str,
    /// `pattern_id` is the identifier of the pattern, for example, 'wayfinder'.
    pub pattern_id: wire::Str,
}

impl wire::Encode for TrimPattern {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_name.encode(writer)?;
        self.pattern_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TrimPattern {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_name = reader.read_str(crate::fields::TRIM_PATTERN_ITEM_NAME)?;
        let pattern_id = reader.read_str(crate::fields::TRIM_PATTERN_PATTERN_ID)?;
        Ok(Self {
            item_name,
            pattern_id,
        })
    }
}

// Domain: voxel

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VoxelShapesRegistryHandle(pub u16);

impl wire::Encode for VoxelShapesRegistryHandle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::U16LE(self.0).encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for VoxelShapesRegistryHandle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(Self(<wire::U16LE as wire::Decode>::decode(reader)?.0))
    }
}

/// VoxelCells represents a 3D grid of voxel cell data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxelShapesSerializableCells {
    /// `x_size` is the size of the grid along the X axis.
    pub x_size: wire::U8,
    /// `y_size` is the size of the grid along the Y axis.
    pub y_size: wire::U8,
    /// `z_size` is the size of the grid along the Z axis.
    pub z_size: wire::U8,
    /// `storage` is the raw cell data stored in the grid.
    pub storage: bytes::Bytes,
}

impl wire::Encode for VoxelShapesSerializableCells {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.x_size.encode(writer)?;
        wire::check_number_limits(self.x_size.0, None, Some(127))?;
        self.y_size.encode(writer)?;
        wire::check_number_limits(self.y_size.0, None, Some(127))?;
        self.z_size.encode(writer)?;
        wire::check_number_limits(self.z_size.0, None, Some(127))?;
        wire::encode_bytes_limits(writer, self.storage.as_ref(), 0, 256048)?;
        Ok(())
    }
}

impl wire::Decode for VoxelShapesSerializableCells {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let x_size = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(127))?;
            value
        };
        let y_size = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(127))?;
            value
        };
        let z_size = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, None, Some(127))?;
            value
        };
        let storage = wire::decode_bytes_limits(
            reader,
            crate::fields::VOXEL_SHAPES_SERIALIZABLE_CELLS_STORAGE,
            0,
            256048,
        )?;
        Ok(Self {
            x_size,
            y_size,
            z_size,
            storage,
        })
    }
}

/// VoxelShape represents a voxel shape with cells and coordinate axes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxelShapesSerializableVoxelShape {
    /// `cells` is the grid of cells representing solid and empty regions.
    pub cells: VoxelShapesSerializableCells,
    /// `x_coordinates` is a list of X axis coordinates for the shape.
    pub x_coordinates: Vec<wire::F32LE>,
    /// `y_coordinates` is a list of Y axis coordinates for the shape.
    pub y_coordinates: Vec<wire::F32LE>,
    /// `z_coordinates` is a list of Z axis coordinates for the shape.
    pub z_coordinates: Vec<wire::F32LE>,
}

impl wire::Encode for VoxelShapesSerializableVoxelShape {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.cells.encode(writer)?;
        wire::encode_collection_limits(writer, self.x_coordinates.as_slice(), 1, 128)?;
        wire::encode_collection_limits(writer, self.y_coordinates.as_slice(), 1, 128)?;
        wire::encode_collection_limits(writer, self.z_coordinates.as_slice(), 1, 128)?;
        Ok(())
    }
}

impl wire::Decode for VoxelShapesSerializableVoxelShape {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cells = <VoxelShapesSerializableCells as wire::Decode>::decode(reader)?;
        let x_coordinates = wire::decode_collection_limits::<wire::F32LE>(
            reader,
            crate::fields::VOXEL_SHAPES_SERIALIZABLE_VOXEL_SHAPE_X_COORDINATES,
            4,
            1,
            128,
        )?;
        let y_coordinates = wire::decode_collection_limits::<wire::F32LE>(
            reader,
            crate::fields::VOXEL_SHAPES_SERIALIZABLE_VOXEL_SHAPE_Y_COORDINATES,
            4,
            1,
            128,
        )?;
        let z_coordinates = wire::decode_collection_limits::<wire::F32LE>(
            reader,
            crate::fields::VOXEL_SHAPES_SERIALIZABLE_VOXEL_SHAPE_Z_COORDINATES,
            4,
            1,
            128,
        )?;
        Ok(Self {
            cells,
            x_coordinates,
            y_coordinates,
            z_coordinates,
        })
    }
}

// Domain: waypoint

/// LocatorBarWaypoint represents a waypoint entry in the locator bar packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocatorBarWaypoint {
    /// `group_handle` is the UUID handle for the waypoint group.
    pub group_handle: WaypointGroupWaypointHandle,
    /// Waypoint contains the waypoint data.
    pub server_waypoint_payload: ServerWaypoint,
    /// Action determines the action for this waypoint. It is one of the WaypointAction constants.
    pub action_flag: ServerWaypointGroupAction,
}

impl wire::Encode for LocatorBarWaypoint {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.group_handle.encode(writer)?;
        self.server_waypoint_payload.encode(writer)?;
        self.action_flag.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LocatorBarWaypoint {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let group_handle = <WaypointGroupWaypointHandle as wire::Decode>::decode(reader)?;
        let server_waypoint_payload = <ServerWaypoint as wire::Decode>::decode(reader)?;
        let action_flag = <ServerWaypointGroupAction as wire::Decode>::decode(reader)?;
        Ok(Self {
            group_handle,
            server_waypoint_payload,
            action_flag,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaypointGroupWaypointHandle {
    pub uuid: uuid::Uuid,
}

impl wire::Encode for WaypointGroupWaypointHandle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.uuid.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for WaypointGroupWaypointHandle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        Ok(Self { uuid })
    }
}

// Domain: world

/// DimensionDefinition contains information specifying dimension-specific properties, used for data-driven
/// dimensions. These include the range (the height min/max), generator variant, and more.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimensionDefinition {
    /// `minimum_y` is the lowest Y coordinate that exists in the dimension.
    pub minimum_y: wire::ZigZag32,
    /// `height_range` is the number of blocks above MinimumY that exist in the dimension, so that the highest Y
    /// coordinate in the dimension is MinimumY + HeightRange.
    pub height_range: wire::ZigZag32,
    /// Generator is the variant of generator that exists in the provided dimension. These can be one of the
    /// constants defined above. If this is set to GeneratorLegacy, the legacy horizontal world limits will be
    /// enforced.
    pub generator_type: GeneratorType,
    /// `dimension_type` is the numeric identifier of the dimension. This cannot override a vanilla dimension
    /// (0-2), but custom dimensions should start from 1000 like vanilla.
    pub dimension_type: DimensionType,
    /// `pack_id` is the UUID of the behaviour pack which has added the dimension.
    pub pack_id: uuid::Uuid,
    /// `default_biome` is the identifier of the biome that the dimension defaults to.
    pub default_biome: wire::Str,
}

impl wire::Encode for DimensionDefinition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.minimum_y.encode(writer)?;
        self.height_range.encode(writer)?;
        self.generator_type.encode(writer)?;
        self.dimension_type.encode(writer)?;
        self.pack_id.encode(writer)?;
        wire::encode_string_limits(writer, &self.default_biome, 0, 256)?;
        Ok(())
    }
}

impl wire::Decode for DimensionDefinition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let minimum_y = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let height_range = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let generator_type = <GeneratorType as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        let pack_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let default_biome = wire::decode_string_limits(
            reader,
            crate::fields::DIMENSION_DEFINITION_DEFAULT_BIOME,
            0,
            256,
        )?;
        Ok(Self {
            minimum_y,
            height_range,
            generator_type,
            dimension_type,
            pack_id,
            default_biome,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldPosition {
    pub position: glam::Vec3,
    pub dimension_type: DimensionType,
}

impl wire::Encode for WorldPosition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.position.encode(writer)?;
        self.dimension_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for WorldPosition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        Ok(Self {
            position,
            dimension_type,
        })
    }
}
