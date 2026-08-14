//! Design tokens: colors, spacing, and font sizes used by all components.
//!
//! Components never hardcode values; they reference these tokens so the
//! whole kit can be re-themed in one place.

use gpui::Rgba;

/// Build an [`Rgba`] from a 0xRRGGBB hex value (const-compatible).
pub const fn rgba_hex(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

// ============================================================================
// Colors
// ============================================================================

pub const BACKGROUND: Rgba = rgba_hex(0x1E1E22);
pub const SURFACE: Rgba = rgba_hex(0x26262C);
pub const SURFACE_HOVER: Rgba = rgba_hex(0x2E2E36);
pub const SURFACE_ACTIVE: Rgba = rgba_hex(0x363640);
pub const BORDER: Rgba = rgba_hex(0x3A3A44);
pub const TEXT_PRIMARY: Rgba = rgba_hex(0xE8E8EC);
pub const TEXT_SECONDARY: Rgba = rgba_hex(0x9A9AA5);
pub const TEXT_DISABLED: Rgba = rgba_hex(0x5C5C66);
pub const ACCENT: Rgba = rgba_hex(0x4C8DFF);
pub const ACCENT_HOVER: Rgba = rgba_hex(0x66A0FF);
pub const ACCENT_TEXT: Rgba = rgba_hex(0xFFFFFF);
pub const DANGER: Rgba = rgba_hex(0xE5484D);
pub const WARNING: Rgba = rgba_hex(0xF5A623);
pub const SUCCESS: Rgba = rgba_hex(0x30A46C);
pub const SELECTED: Rgba = rgba_hex(0x2A3F5F);

// ============================================================================
// Spacing (in px)
// ============================================================================

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;
pub const SPACE_6: f32 = 32.0;

// ============================================================================
// Font sizes (in rem)
// ============================================================================

pub const FONT_SIZE_XS: f32 = 0.75;
pub const FONT_SIZE_SM: f32 = 0.85;
pub const FONT_SIZE_MD: f32 = 1.0;
pub const FONT_SIZE_LG: f32 = 1.15;
pub const FONT_SIZE_XL: f32 = 1.35;

// ============================================================================
// Radii
// ============================================================================

pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 10.0;

// ============================================================================
// Sizes
// ============================================================================

/// Control (button/input) height in px.
pub const CONTROL_HEIGHT: f32 = 28.0;
/// Icon button size in px.
pub const ICON_BUTTON_SIZE: f32 = 24.0;
