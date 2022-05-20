use super::{euclid, Px, PxSize};

pub use euclid::BoolVector2D;

/// Constrains on a pixel length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PxConstrains {
    /// Maximum allowed length.
    pub max: Px,
    /// Minimum allowed length.
    pub min: Px,
    /// If `max` is the *fill* length, otherwise `min` is.
    ///
    /// Note that this is ignored if the max [`is_unbounded`], use [`actually_fill`] for operations.
    ///
    /// [`is_unbounded`]: Self::is_unbounded
    /// [`actually_fill`]: Self::actually_fill
    pub fill: bool,
}
impl Default for PxConstrains {
    fn default() -> Self {
        Self {
            max: Px::MAX,
            min: Px(0),
            fill: false,
        }
    }
}
impl PxConstrains {
    /// No constrains, max is [`Px::MAX`], min is zero and fill is false, this the default value.
    pub fn unbounded() -> Self {
        Self::default()
    }

    /// Exact length constrains, both max and min are `px`, fill is false.
    pub fn exact(px: Px) -> Self {
        Self {
            max: px,
            min: px,
            fill: false,
        }
    }

    /// Returns if [`max`] is [`Px::MAX`]. Unbounded constrains do not fill even if [`fill`] is requested.
    ///
    /// [`max`]: Self::max
    /// [`fill`]: Self::fill
    pub fn is_unbounded(&self) -> bool {
        self.max == Px::MAX
    }

    /// Returns if [`max`] is equal to [`min`].
    ///
    /// [`max`]: Self::max
    /// [`min`]: Self::min
    pub fn is_exact(&self) -> bool {
        self.max == self.min
    }

    /// Returns `true` if fill is requested and the constrains are bounded.
    pub fn actually_fill(&self) -> bool {
        self.fill && !self.is_unbounded()
    }

    /// Returns the length to fill, that is [`max`] if [`actually_fill`] is `true`, otherwise returns [`min`].
    ///
    /// [`max`]: Self::max
    /// [`actual_fill`]: Self::actual_fill
    /// [`min`]: Self::min
    pub fn fill_length(&self) -> Px {
        if self.actually_fill() {
            self.max
        } else {
            self.min
        }
    }

    /// Clamp the `px` by min and max.
    pub fn clamp(&self, px: Px) -> Px {
        self.min.max(px).min(self.max)
    }

    /// Returns the fill length or the desired length
    pub fn fill_or(&self, desired_length: Px) -> Px {
        if self.actually_fill() {
            self.max
        } else {
            self.clamp(desired_length)
        }
    }

    /// Returns a constrain with `max`, adjusts min to be less or equal to the new `max`.
    pub fn with_max(mut self, max: Px) -> Self {
        self.max = max;
        self.min = self.min.min(self.max);
        self
    }

    /// Returns a constrain with `min`, adjusts max to be more or equal to the new `min`.
    pub fn with_min(mut self, min: Px) -> Self {
        self.min = min;
        self.max = self.max.max(min);
        self
    }

    /// Returns a constrain with fill config.
    pub fn with_fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Returns a constrain with max subtracted by `removed` and min adjusted to be less or equal to max.
    pub fn with_less(mut self, removed: Px) -> Self {
        if !self.is_unbounded() {
            self.max -= removed;
            self.min = self.min.min(self.max);
        }
        self
    }

    /// Returns a constrain with max added by `added`.
    pub fn with_more(mut self, added: Px) -> Self {
        self.max += added;
        self
    }

    /// Returns a constrain with max set to the unbounded value, [`Px::MAX`].
    pub fn with_unbounded(mut self) -> Self {
        self.max = Px::MAX;
        self
    }
}

/// Constrains on a pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PxSizeConstrains {
    /// Maximum allowed size.
    pub max: PxSize,
    /// Minimum allowed size.
    pub min: PxSize,
    /// If `max` size is the *fill* size, otherwise `min` is.
    ///
    /// Note that this is ignored if the max [`is_unbounded`], use [`actually_fill`] for operations.
    ///
    /// [`is_unbounded`]: Self::is_unbounded
    /// [`actually_fill`]: Self::actually_fill
    pub fill: BoolVector2D,
}
impl Default for PxSizeConstrains {
    fn default() -> Self {
        PxSizeConstrains {
            max: PxSize::new(Px::MAX, Px::MAX),
            min: PxSize::zero(),
            fill: BoolVector2D { x: false, y: false },
        }
    }
}
impl PxSizeConstrains {
    /// No constrains, max is [`Px::MAX`], min is zero and fill is false, this the default value.
    pub fn unbounded() -> Self {
        Self::default()
    }

    /// Exact size constrains, both max and min are `size`, fill is false.
    pub fn exact(size: PxSize) -> Self {
        Self {
            max: size,
            min: size,
            fill: BoolVector2D { x: false, y: false },
        }
    }

    /// Returns if [`max`] is [`Px::MAX`]. Unbounded constrains do not fill even if [`fill`] is requested.
    ///
    /// [`max`]: Self::max
    /// [`fill`]: Self::fill
    pub fn is_unbounded(&self) -> BoolVector2D {
        BoolVector2D {
            x: self.max.width == Px::MAX,
            y: self.max.height == Px::MAX,
        }
    }

    /// Returns if [`max`] is equal to [`min`].
    ///
    /// [`max`]: Self::max
    /// [`min`]: Self::min
    pub fn is_exact(&self) -> BoolVector2D {
        BoolVector2D {
            x: self.max.width == self.min.width,
            y: self.max.height == self.min.height,
        }
    }

    /// Returns if `max.width` is equal to `min.width`.
    pub fn is_exact_width(&self) -> bool {
        self.max.width == self.min.width
    }
    /// Returns if `max.height` is equal to `min.height`.
    pub fn is_exact_height(&self) -> bool {
        self.max.height == self.min.height
    }

    /// Returns `true` if fill is requested and the constrains are bounded.
    pub fn actually_fill(&self) -> BoolVector2D {
        self.fill.and(self.is_unbounded().not())
    }

    /// Returns the size to fill all available space.
    pub fn fill_size(&self) -> PxSize {
        self.actually_fill().select_size(self.max, self.min)
    }

    /// Returns a size both dimensions are fill or an exact length.
    pub fn fill_or_exact(&self) -> Option<PxSize> {
        let fill = self.actually_fill();

        let width = if fill.x || self.is_exact_width() {
            self.max.width
        } else {
            return None;
        };
        let height = if fill.y || self.is_exact_height() {
            self.max.height
        } else {
            return None;
        };

        Some(PxSize::new(width, height))
    }

    /// Returns the width that fills the X-axis.
    pub fn fill_width(&self) -> Px {
        if self.actually_fill().x {
            self.max.width
        } else {
            self.min.width
        }
    }

    /// Returns the height that fills the Y-axis.
    pub fn fill_height(&self) -> Px {
        if self.actually_fill().y {
            self.max.height
        } else {
            self.min.height
        }
    }

    /// Clamp the `size` by min and max.
    pub fn clamp(&self, size: PxSize) -> PxSize {
        self.min.max(size).min(self.max)
    }

    /// Returns the fill size, or the desired size clamped.
    pub fn fill_or(&self, desired_size: PxSize) -> PxSize {
        let fill = self.actually_fill();
        let width = if fill.x {
            self.max.width
        } else {
            desired_size.width.max(self.min.width).min(self.max.width)
        };
        let height = if fill.y {
            self.max.height
        } else {
            desired_size.height.max(self.min.height).min(self.max.height)
        };
        PxSize::new(width, height)
    }

    /// X-axis constrains.
    pub fn x_constrains(&self) -> PxConstrains {
        PxConstrains {
            max: self.max.width,
            min: self.min.width,
            fill: self.fill.x,
        }
    }

    /// Y-axis constrains.
    pub fn y_constrains(&self) -> PxConstrains {
        PxConstrains {
            max: self.max.height,
            min: self.min.height,
            fill: self.fill.y,
        }
    }

    /// Returns a constrain with `max` size and `min` adjusted to be less-or-equal to `max`.
    pub fn with_max(mut self, max: PxSize) -> Self {
        self.max = max;
        self.min = self.min.min(self.max);
        self
    }

    /// Returns a constrain with `max` size, `min` adjusted to be less-or-equal to `max` and fill set to both.
    pub fn with_max_fill(self, max: PxSize) -> Self {
        self.with_max(max).with_fill(true, true)
    }

    /// Returns a constrain with `min` size and `max` adjusted to be more-or-equal to `min`.
    pub fn with_min(mut self, min: PxSize) -> Self {
        self.min = min;
        self.max = self.max.max(self.min);
        self
    }

    /// Returns a constrain with `max.width` size and `min.width` adjusted to be less-or-equal to `max.width`.
    pub fn with_max_width(mut self, max_width: Px) -> Self {
        self.max.width = max_width;
        self.min.width = self.min.width.min(self.max.width);
        self
    }

    /// Returns a constrain with `max.width` size, `min.width` adjusted to be less-or-equal to `max.width` and `fill.x` set.
    pub fn with_width_fill(self, max_width: Px) -> Self {
        self.with_max_width(max_width).with_fill_x(true)
    }

    /// Returns a constrain with `max.height` size and `min.height` adjusted to be less-or-equal to `max.height`.
    pub fn with_max_height(mut self, max_height: Px) -> Self {
        self.max.height = max_height;
        self.min.height = self.min.height.min(self.max.height);
        self
    }

    /// Returns a constrain with `max.height` size, `min.height` adjusted to be less-or-equal to `max.height` and `fill.y` set.
    pub fn with_height_fill(self, max_height: Px) -> Self {
        self.with_max_height(max_height).with_fill_y(true)
    }

    /// Returns a constrain with `min.width` size and `max.width` adjusted to be more-or-equal to `min.width`.
    pub fn with_min_width(mut self, min_width: Px) -> Self {
        self.min.width = min_width;
        self.max.width = self.max.width.max(self.min.width);
        self
    }

    /// Returns a constrain with `max.height` size and `max.height` adjusted to be more-or-equal to `min.height`.
    pub fn with_min_height(mut self, min_height: Px) -> Self {
        self.min.height = min_height;
        self.max.height = self.max.height.max(self.min.height);
        self
    }

    /// Returns a constrain with fill config in both axis.
    pub fn with_fill(mut self, fill_x: bool, fill_y: bool) -> Self {
        self.fill = BoolVector2D { x: fill_x, y: fill_y };
        self
    }

    /// Returns a constrain with `fill.x` config.
    pub fn with_fill_x(mut self, fill_x: bool) -> Self {
        self.fill.x = fill_x;
        self
    }

    /// Returns a constrain with `fill.y` config.
    pub fn with_fill_y(mut self, fill_y: bool) -> Self {
        self.fill.y = fill_y;
        self
    }

    /* Note, Px ops are saturating */

    /// Returns a constrains with `max` subtracted by `removed` and `min` adjusted to be less-or-equal to `max`.
    pub fn with_less_size(mut self, removed: PxSize) -> Self {
        let unbounded = self.is_unbounded();
        if !unbounded.x {
            self.max.width -= removed.width;
        }
        if !unbounded.y {
            self.max.height -= removed.height;
        }
        self.min = self.min.min(self.max);
        self
    }

    /// Returns a constrains with `max.width` subtracted by `removed` and `min.width` adjusted to be less-or-equal to `max.width`.
    pub fn with_less_width(mut self, removed: Px) -> Self {
        if !self.is_unbounded().x {
            self.max.width -= removed;
            self.min.width = self.min.width.min(self.max.width);
        }
        self
    }

    /// Returns a constrains with `max.height` subtracted by `removed` and `min.height` adjusted to be less-or-equal to `max.height`.
    pub fn with_less_height(mut self, removed: Px) -> Self {
        if !self.is_unbounded().y {
            self.max.height -= removed;
            self.min.height = self.min.height.min(self.max.height);
        }
        self
    }

    /// Returns a constrains with `max` added by `added`.
    pub fn with_more_size(mut self, added: PxSize) -> Self {
        self.max += added;
        self
    }

    /// Returns a constrains with `max.width` added by `added`.
    pub fn with_more_width(mut self, added: Px) -> Self {
        self.max.width += added;
        self
    }

    /// Returns a constrains with `max.height` added by `added`.
    pub fn with_more_height(mut self, added: Px) -> Self {
        self.max.height += added;
        self
    }

    /// Returns a constrains with `max.width` set to [`Px::MAX`].
    pub fn with_unbounded_x(mut self) -> Self {
        self.max.width = Px::MAX;
        self
    }

    /// Returns a constrains with `max.height` set to [`Px::MAX`].
    pub fn with_unbounded_y(mut self) -> Self {
        self.max.height = Px::MAX;
        self
    }
}
