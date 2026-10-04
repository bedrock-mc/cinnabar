/// Rejection reason for invalid UI geometry or scale values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryError {
    NonFinite,
    OutOfRange,
    InvertedRect,
    NegativeInset,
}

/// UI text scale. User preferences use `0.5..=4.0`; derived display scales
/// also account for platform DPI and magnified text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiScale(f32);

impl UiScale {
    pub const MIN: f32 = 0.5;
    pub const MAX: f32 = 4.0;
    pub(crate) const SCALE_DENOMINATOR: i64 = 1_024;
    /// The smallest nonzero scale representable by the text layout's fixed point.
    pub const DISPLAY_MIN: f32 = 1.0 / Self::SCALE_DENOMINATOR as f32;
    /// Upper bound for magnified display text (titles), beyond the user-selectable range.
    pub const DISPLAY_MAX: f32 = 16.0;

    pub fn new(value: f32) -> Result<Self, GeometryError> {
        finite_in_range(value, Self::MIN, Self::MAX).map(Self)
    }

    /// Accepts DPI-adjusted and magnified display text outside the preference range.
    pub fn new_display(value: f32) -> Result<Self, GeometryError> {
        finite_in_range(value, Self::DISPLAY_MIN, Self::DISPLAY_MAX).map(Self)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for UiScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Platform DPI scale used at the physical-to-logical input boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DpiScale(f32);

impl DpiScale {
    pub const MIN: f32 = 0.5;
    pub const MAX: f32 = 8.0;

    pub fn new(value: f32) -> Result<Self, GeometryError> {
        finite_in_range(value, Self::MIN, Self::MAX).map(Self)
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn physical_to_logical(self, value: f32) -> f32 {
        value / self.0
    }

    pub fn logical_point(self, physical: [f32; 2]) -> Result<UiPoint, GeometryError> {
        UiPoint::new(
            self.physical_to_logical(physical[0]),
            self.physical_to_logical(physical[1]),
        )
    }
}

/// A finite point or delta in logical UI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiPoint {
    x: f32,
    y: f32,
}

impl UiPoint {
    pub fn new(x: f32, y: f32) -> Result<Self, GeometryError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(GeometryError::NonFinite);
        }
        Ok(Self { x, y })
    }

    pub const fn x(self) -> f32 {
        self.x
    }

    pub const fn y(self) -> f32 {
        self.y
    }

    pub fn from_physical(physical: [f32; 2], dpi: DpiScale) -> Result<Self, GeometryError> {
        dpi.logical_point(physical)
    }
}

/// A finite, non-inverted rectangle in logical UI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiRect {
    min: UiPoint,
    max: UiPoint,
}

impl UiRect {
    pub fn new(min: UiPoint, max: UiPoint) -> Result<Self, GeometryError> {
        if min.x > max.x || min.y > max.y {
            return Err(GeometryError::InvertedRect);
        }
        Ok(Self { min, max })
    }

    pub fn from_physical(
        min: [f32; 2],
        max: [f32; 2],
        dpi: DpiScale,
    ) -> Result<Self, GeometryError> {
        Self::new(dpi.logical_point(min)?, dpi.logical_point(max)?)
    }

    pub const fn min(self) -> UiPoint {
        self.min
    }

    pub const fn max(self) -> UiPoint {
        self.max
    }

    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn contains(self, point: UiPoint) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
}

/// Nonnegative viewport insets in logical UI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SafeArea {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl SafeArea {
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Result<Self, GeometryError> {
        let values = [left, top, right, bottom];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(GeometryError::NonFinite);
        }
        if values.iter().any(|value| *value < 0.0) {
            return Err(GeometryError::NegativeInset);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    pub fn from_physical(
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        dpi: DpiScale,
    ) -> Result<Self, GeometryError> {
        Self::new(
            dpi.physical_to_logical(left),
            dpi.physical_to_logical(top),
            dpi.physical_to_logical(right),
            dpi.physical_to_logical(bottom),
        )
    }

    pub const fn left(self) -> f32 {
        self.left
    }

    pub const fn top(self) -> f32 {
        self.top
    }

    pub const fn right(self) -> f32 {
        self.right
    }

    pub const fn bottom(self) -> f32 {
        self.bottom
    }
}

impl Default for SafeArea {
    fn default() -> Self {
        Self::ZERO
    }
}

fn finite_in_range(value: f32, min: f32, max: f32) -> Result<f32, GeometryError> {
    if !value.is_finite() {
        return Err(GeometryError::NonFinite);
    }
    if !(min..=max).contains(&value) {
        return Err(GeometryError::OutOfRange);
    }
    Ok(value)
}

/// Vanilla desktop GUI scale values.
const DESKTOP_GUI_SCALE_VALUES: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// Physical pixels per GUI pixel: Bedrock's desktop rule
/// (`GuiData::calculateOptimalGuiScaleIndex`), `min(width/376, height/250)` in
/// 1..=8, or a fixed preference no larger than that.
#[must_use]
pub fn gui_scale(physical: [u32; 2], preference: Option<u8>) -> u32 {
    let index = (physical[0] / 376)
        .min(physical[1] / 250)
        .saturating_sub(1)
        .min(DESKTOP_GUI_SCALE_VALUES.len() as u32 - 1);
    let auto = u32::from(DESKTOP_GUI_SCALE_VALUES[index as usize]);
    match preference {
        None | Some(0) => auto,
        Some(fixed) => u32::from(fixed).clamp(1, auto),
    }
}

/// Desktop settings use a signed modifier relative to the optimal scale,
/// rather than an absolute scale. Desktop optimal and maximum scale share the
/// same 376-by-250 minimum viewport, with settings offering the upper half
/// of the supported physical scale range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopGuiScale {
    optimal: u8,
}

impl DesktopGuiScale {
    #[must_use]
    pub fn for_window(physical: [u32; 2]) -> Self {
        Self {
            optimal: gui_scale(physical, None) as u8,
        }
    }

    #[must_use]
    pub fn offsets(self) -> std::ops::RangeInclusive<i8> {
        let minimum = self.optimal.div_ceil(2);
        (minimum as i8 - self.optimal as i8)..=0
    }

    #[must_use]
    pub fn scale_for_offset(self, offset: i8) -> u8 {
        let minimum = *self.offsets().start();
        (self.optimal as i8 + offset.clamp(minimum, 0)) as u8
    }
}
