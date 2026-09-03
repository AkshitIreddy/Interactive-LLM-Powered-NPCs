use serde::{Deserialize, Serialize};

/// A physical desktop-pixel point. Coordinates may be negative on multi-monitor desktops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PointPx {
    pub x: f32,
    pub y: f32,
}

impl PointPx {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A physical desktop-pixel size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SizePx {
    pub width: f32,
    pub height: f32,
}

impl SizePx {
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Axis-aligned rectangle in physical desktop pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RectPx {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectPx {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    #[must_use]
    pub fn center(self) -> PointPx {
        PointPx::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    #[must_use]
    pub fn area(self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }

    #[must_use]
    pub fn is_finite_positive(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }

    #[must_use]
    pub fn intersection_area(self, other: Self) -> f32 {
        self.intersection(other).map_or(0.0, Self::area)
    }

    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        // Desktop coordinates can be thousands of pixels negative, where f32 addition at an exact
        // edge can differ by a fraction of a physical pixel. Geometry is composited on a pixel
        // grid, so a 1/64 px comparison tolerance is both deterministic and visually exact.
        const EDGE_EPSILON_PX: f32 = 1.0 / 64.0;
        other.x + EDGE_EPSILON_PX >= self.x
            && other.y + EDGE_EPSILON_PX >= self.y
            && other.right() <= self.right() + EDGE_EPSILON_PX
            && other.bottom() <= self.bottom() + EDGE_EPSILON_PX
    }

    #[must_use]
    pub fn inset(self, insets: InsetsPx) -> Self {
        Self::new(
            self.x + insets.left,
            self.y + insets.top,
            (self.width - insets.left - insets.right).max(0.0),
            (self.height - insets.top - insets.bottom).max(0.0),
        )
    }

    #[must_use]
    pub fn clamp_inside(self, container: Self) -> Self {
        let width = self.width.min(container.width).max(0.0);
        let height = self.height.min(container.height).max(0.0);
        let max_x = (container.right() - width).max(container.x);
        let max_y = (container.bottom() - height).max(container.y);
        Self::new(
            self.x.clamp(container.x, max_x),
            self.y.clamp(container.y, max_y),
            width,
            height,
        )
    }

    #[must_use]
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    #[must_use]
    pub fn outset(self, outsets: InsetsPx) -> Self {
        Self::new(
            self.x - outsets.left,
            self.y - outsets.top,
            self.width + outsets.left + outsets.right,
            self.height + outsets.top + outsets.bottom,
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InsetsPx {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl InsetsPx {
    #[must_use]
    pub const fn uniform(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// Scale factor from density-independent pixels to physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DpiScale(f32);

impl DpiScale {
    pub const MIN: f32 = 0.5;
    pub const MAX: f32 = 4.0;

    pub fn new(scale: f32) -> Result<Self, InvalidDpiScale> {
        if scale.is_finite() && (Self::MIN..=Self::MAX).contains(&scale) {
            Ok(Self(scale))
        } else {
            Err(InvalidDpiScale(scale))
        }
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    #[must_use]
    pub fn px(self, dp: f32) -> f32 {
        dp * self.0
    }

    #[must_use]
    pub fn dp(self, px: f32) -> f32 {
        px / self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
#[error("DPI scale {0} is outside the supported finite range 0.5..=4.0")]
pub struct InvalidDpiScale(pub f32);
