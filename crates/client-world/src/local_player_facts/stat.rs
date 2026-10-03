use protocol::ActorAttribute;

/// A normalized local-player attribute, retaining its established integer scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPlayerStat {
    current: u16,
    maximum: u16,
    scale: u16,
}

impl LocalPlayerStat {
    /// Validates and quantizes an attribute exactly as the former HUD adapter did.
    #[must_use]
    pub fn from_attribute(attribute: &ActorAttribute) -> Option<Self> {
        if !attribute.current.is_finite()
            || !attribute.max.is_finite()
            || attribute.max <= 0.0
            || attribute.current < 0.0
            || attribute.current > attribute.max
        {
            return None;
        }
        let scale = if attribute.max <= u16::MAX as f32 / 100.0 {
            100.0
        } else {
            1.0
        };
        let maximum = u16::try_from((attribute.max * scale).round() as u32).ok()?;
        let current = u16::try_from((attribute.current * scale).round() as u32).ok()?;
        if maximum == 0 || current > maximum {
            return None;
        }
        Some(Self {
            current,
            maximum,
            scale: scale as u16,
        })
    }

    /// Returns the quantized current value without changing its scale.
    #[must_use]
    pub const fn current(self) -> u16 {
        self.current
    }

    /// Returns the quantized maximum in the same units as the current value.
    #[must_use]
    pub const fn maximum(self) -> u16 {
        self.maximum
    }

    /// Returns the factor that converts original attribute units to retained units.
    #[must_use]
    pub const fn scale(self) -> u16 {
        self.scale
    }
}
