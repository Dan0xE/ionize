// SPDX-License-Identifier: MPL-2.0

/// A position in the layout coordinate space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// Constructs `(x, y)`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A two-dimensional extent in layout coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

impl Size {
    /// Constructs a `w` by `h` extent.
    #[must_use]
    pub const fn new(w: f64, h: f64) -> Self {
        Self { w, h }
    }
}

/// A rectangle described by its top-left corner and size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Top-left corner.
    pub pos: Point,
    /// Width and height.
    pub size: Size,
}

impl Rect {
    /// Combines a top-left corner and size.
    #[must_use]
    pub const fn new(pos: Point, size: Size) -> Self {
        Self { pos, size }
    }
}
