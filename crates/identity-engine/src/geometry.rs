use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A detector-space rectangle. Coordinates may be pixels or normalized units,
/// but every rectangle in a tracker must use the same coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundingBoxV1 {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBoxV1 {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Result<Self, GeometryError> {
        let value = Self {
            x,
            y,
            width,
            height,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), GeometryError> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Err(GeometryError::InvalidBoundingBox);
        }
        Ok(())
    }

    pub fn area(self) -> f32 {
        self.width * self.height
    }

    pub fn center(self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn intersection_over_union(self, other: Self) -> f32 {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let intersection = (right - left).max(0.0) * (bottom - top).max(0.0);
        let union = self.area() + other.area() - intersection;
        if union <= f32::EPSILON {
            0.0
        } else {
            (intersection / union).clamp(0.0, 1.0)
        }
    }

    /// Center distance measured in multiples of the larger actor-box diagonal.
    pub fn normalized_center_distance(self, other: Self) -> f32 {
        let (ax, ay) = self.center();
        let (bx, by) = other.center();
        let distance = (ax - bx).hypot(ay - by);
        let scale = self
            .width
            .max(other.width)
            .hypot(self.height.max(other.height))
            .max(f32::EPSILON);
        distance / scale
    }

    pub(crate) fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GeometryError {
    #[error("bounding boxes require finite coordinates and positive finite dimensions")]
    InvalidBoundingBox,
}
