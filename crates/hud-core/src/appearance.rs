//! Pure rules for window appearance: opacity, backdrop and click-through styles.

use crate::theme::Color;

pub const MIN_OPACITY: f32 = 0.2;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;

/// `AABBGGRR` tint for the acrylic backdrop, same color and opacity as the solid background.
pub fn acrylic_tint(background: Color, opacity: f32) -> u32 {
    let alpha = (opacity * 255.0).round() as u32;
    alpha << 24
        | u32::from(background.b) << 16
        | u32::from(background.g) << 8
        | u32::from(background.r)
}

/// The window ignores the mouse only while click-through is on and Ctrl is not held.
pub fn passes_clicks(enabled: bool, ctrl_down: bool) -> bool {
    enabled && !ctrl_down
}

/// Extended window style with click-through added or removed; other bits are untouched.
pub fn click_through_style(current: u32, pass: bool) -> u32 {
    if pass {
        current | WS_EX_LAYERED | WS_EX_TRANSPARENT
    } else {
        current & !WS_EX_TRANSPARENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAMER_BACKGROUND: Color = Color::rgb(0x0A0E14);

    #[test]
    fn acrylic_tint_keeps_color_and_opacity() {
        assert_eq!(acrylic_tint(GAMER_BACKGROUND, 1.0), 0xFF14_0E0A);
        assert_eq!(acrylic_tint(GAMER_BACKGROUND, 0.5) >> 24, 128);
    }

    #[test]
    fn acrylic_tint_follows_the_theme_background() {
        assert_eq!(acrylic_tint(Color::rgb(0xF4F6F8), 1.0), 0xFFF8_F6F4);
    }

    #[test]
    fn ctrl_lifts_click_through_until_released() {
        assert!(passes_clicks(true, false));
        assert!(!passes_clicks(true, true));
        assert!(!passes_clicks(false, false));
    }

    #[test]
    fn click_through_sets_layered_and_transparent_and_ctrl_removes_it() {
        let on = click_through_style(0x1, true);
        assert_eq!(
            on & (WS_EX_LAYERED | WS_EX_TRANSPARENT),
            WS_EX_LAYERED | WS_EX_TRANSPARENT
        );
        let off = click_through_style(on, false);
        assert_eq!(off & WS_EX_TRANSPARENT, 0);
        assert_eq!(off & 0x1, 0x1);
    }
}
