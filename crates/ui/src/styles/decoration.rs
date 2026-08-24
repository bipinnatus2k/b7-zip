use gpui::{px, Pixels, Styled, Tiling};

/// Defines window border radius for platforms that use client side decorations.
pub const CLIENT_SIDE_DECORATION_ROUNDING: Pixels = px(10.0);
/// Defines window shadow size for platforms that use client side decorations.
pub const CLIENT_SIDE_DECORATION_SHADOW: Pixels = px(10.0);

/// Styling helpers for elements that follow client-side window decorations.
pub trait ClientDecorationsExt: Styled {
    /// Rounds each corner whose two adjacent edges are both untiled.
    fn rounded_client_corners(mut self, tiling: Tiling) -> Self {
        if !tiling.top && !tiling.left {
            self = self.rounded_tl(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.top && !tiling.right {
            self = self.rounded_tr(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.bottom && !tiling.left {
            self = self.rounded_bl(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        if !tiling.bottom && !tiling.right {
            self = self.rounded_br(CLIENT_SIDE_DECORATION_ROUNDING);
        }
        self
    }
}

impl<T: Styled> ClientDecorationsExt for T {}