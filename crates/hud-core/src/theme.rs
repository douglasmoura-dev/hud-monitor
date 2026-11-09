//! Visual themes: the five built-in palettes and the rules for checking them.
//!
//! The palettes are plain data; hud-app copies the chosen one into the Slint `Theme` global.
//! There is no free-form color and no theme file (spec C7).

use serde::{Deserialize, Serialize, Serializer};

/// Minimum contrast of alert text over the theme background (WCAG large-text bar).
pub const MIN_ALERT_CONTRAST: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Opaque color from `0xRRGGBB`.
    pub const fn rgb(hex: u32) -> Self {
        Self::rgba(hex << 8 | 0xFF)
    }

    /// Color from `0xRRGGBBAA`.
    pub const fn rgba(hex: u32) -> Self {
        Self {
            r: (hex >> 24) as u8,
            g: (hex >> 16) as u8,
            b: (hex >> 8) as u8,
            a: hex as u8,
        }
    }

    /// WCAG relative luminance (alpha ignored).
    fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
}

/// WCAG contrast ratio between two colors, from 1 to 21.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(try_from = "String")]
pub enum ThemeId {
    #[default]
    GamerHud,
    Cyberpunk,
    Minimal,
    Terminal,
    Light,
}

impl ThemeId {
    pub const ALL: [ThemeId; 5] = [
        ThemeId::GamerHud,
        ThemeId::Cyberpunk,
        ThemeId::Minimal,
        ThemeId::Terminal,
        ThemeId::Light,
    ];

    /// Name used in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            Self::GamerHud => "gamer-hud",
            Self::Cyberpunk => "cyberpunk",
            Self::Minimal => "minimal",
            Self::Terminal => "terminal",
            Self::Light => "light",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::GamerHud => GAMER_HUD,
            Self::Cyberpunk => CYBERPUNK,
            Self::Minimal => MINIMAL,
            Self::Terminal => TERMINAL,
            Self::Light => LIGHT,
        }
    }
}

impl Serialize for ThemeId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

/// Config text that is not one of the five theme ids.
#[derive(Debug, thiserror::Error)]
#[error("invalid config field `appearance.theme`: unknown theme `{0}`")]
pub struct UnknownTheme(String);

impl TryFrom<String> for ThemeId {
    type Error = UnknownTheme;

    fn try_from(id: String) -> Result<Self, UnknownTheme> {
        Self::from_id(&id).ok_or(UnknownTheme(id))
    }
}

/// Every color and font a window draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub accent: Color,
    pub green: Color,
    pub purple: Color,
    pub amber: Color,
    pub blue: Color,
    pub text: Color,
    pub dim: Color,
    pub track: Color,
    pub border: Color,
    pub title_bar: Color,
    pub tip_bar: Color,
    pub warn: Color,
    pub crit: Color,
    pub label_font: &'static str,
    pub number_font: &'static str,
    /// Corner radius of the HUD and mini bar windows; 0 for the flat themes.
    pub radius: u8,
}

const RAJDHANI: &str = "Rajdhani";
const SHARE_TECH_MONO: &str = "Share Tech Mono";

const GAMER_HUD: Palette = Palette {
    background: Color::rgb(0x0A0E14),
    accent: Color::rgb(0x5DCAA5),
    green: Color::rgb(0x1D9E75),
    purple: Color::rgb(0x7F77DD),
    amber: Color::rgb(0xEF9F27),
    blue: Color::rgb(0x85B7EB),
    text: Color::rgb(0xC9D4DD),
    dim: Color::rgb(0x7D8B97),
    track: Color::rgba(0xFFFFFF14),
    border: Color::rgba(0x1D9E7566),
    title_bar: Color::rgba(0xFFFFFF0A),
    tip_bar: Color::rgb(0x0E1620),
    warn: Color::rgb(0xEF9F27),
    crit: Color::rgb(0xE24B4A),
    label_font: RAJDHANI,
    number_font: SHARE_TECH_MONO,
    radius: 6,
};

const CYBERPUNK: Palette = Palette {
    background: Color::rgb(0x0D0221),
    accent: Color::rgb(0x00F0FF),
    green: Color::rgb(0x05D9E8),
    purple: Color::rgb(0xD300C5),
    amber: Color::rgb(0xFFD300),
    blue: Color::rgb(0x7DF9FF),
    text: Color::rgb(0xF1E6FF),
    dim: Color::rgb(0x9A8CB8),
    track: Color::rgba(0xFF2A6D26),
    border: Color::rgba(0xFF2A6D99),
    title_bar: Color::rgba(0xFF2A6D1F),
    tip_bar: Color::rgb(0x1A0B2E),
    warn: Color::rgb(0xFFD300),
    crit: Color::rgb(0xFF2A6D),
    label_font: RAJDHANI,
    number_font: SHARE_TECH_MONO,
    radius: 6,
};

const MINIMAL: Palette = Palette {
    background: Color::rgb(0x15181C),
    accent: Color::rgb(0xE6EAEE),
    green: Color::rgb(0x8FA3B3),
    purple: Color::rgb(0xA59CC0),
    amber: Color::rgb(0xD9A441),
    blue: Color::rgb(0x8FB0D0),
    text: Color::rgb(0xD5DADF),
    dim: Color::rgb(0x8A939B),
    track: Color::rgba(0xFFFFFF14),
    border: Color::rgba(0xFFFFFF1F),
    title_bar: Color::rgba(0xFFFFFF08),
    tip_bar: Color::rgb(0x1E2227),
    warn: Color::rgb(0xE0A93B),
    crit: Color::rgb(0xE5675F),
    label_font: RAJDHANI,
    number_font: SHARE_TECH_MONO,
    radius: 0,
};

const TERMINAL: Palette = Palette {
    background: Color::rgb(0x000000),
    accent: Color::rgb(0x33FF66),
    green: Color::rgb(0x22CC55),
    purple: Color::rgb(0x99FFCC),
    amber: Color::rgb(0xFFB000),
    blue: Color::rgb(0x66FFCC),
    text: Color::rgb(0xB6FFC8),
    dim: Color::rgb(0x4FA468),
    track: Color::rgba(0x33FF661F),
    border: Color::rgba(0x33FF6680),
    title_bar: Color::rgba(0x33FF660F),
    tip_bar: Color::rgb(0x061008),
    warn: Color::rgb(0xFFB000),
    crit: Color::rgb(0xFF4444),
    label_font: SHARE_TECH_MONO,
    number_font: SHARE_TECH_MONO,
    radius: 0,
};

const LIGHT: Palette = Palette {
    background: Color::rgb(0xF4F6F8),
    accent: Color::rgb(0x0B7A5C),
    green: Color::rgb(0x0F8F6B),
    purple: Color::rgb(0x5B52C7),
    amber: Color::rgb(0xB26A00),
    blue: Color::rgb(0x2B6CB0),
    text: Color::rgb(0x1C2630),
    dim: Color::rgb(0x5C6773),
    track: Color::rgba(0x00000014),
    border: Color::rgba(0x0B7A5C66),
    title_bar: Color::rgba(0x0000000A),
    tip_bar: Color::rgb(0xFFFFFF),
    warn: Color::rgb(0xB45309),
    crit: Color::rgb(0xB91C1C),
    label_font: RAJDHANI,
    number_font: SHARE_TECH_MONO,
    radius: 6,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_gamer_hud_with_the_current_colors() {
        let p = ThemeId::default().palette();
        assert_eq!(ThemeId::default(), ThemeId::GamerHud);
        assert_eq!(p.accent, Color::rgb(0x5DCAA5));
        assert_eq!(p.background, Color::rgb(0x0A0E14));
        assert_eq!(p.crit, Color::rgb(0xE24B4A));
    }

    #[test]
    fn the_five_ids_round_trip_and_unknown_ids_are_rejected() {
        let ids = ["gamer-hud", "cyberpunk", "minimal", "terminal", "light"];
        for (theme, id) in ThemeId::ALL.into_iter().zip(ids) {
            assert_eq!(theme.id(), id);
            assert_eq!(ThemeId::from_id(id), Some(theme));
        }
        assert_eq!(ThemeId::from_id("xyz"), None);
    }

    #[test]
    fn alert_text_has_at_least_3_to_1_contrast_on_every_background() {
        for theme in ThemeId::ALL {
            let p = theme.palette();
            for (name, color) in [("warn", p.warn), ("crit", p.crit)] {
                let ratio = contrast(color, p.background);
                assert!(
                    ratio >= MIN_ALERT_CONTRAST,
                    "{} {name}: {ratio:.2}",
                    theme.id()
                );
            }
        }
    }

    #[test]
    fn contrast_of_black_on_white_is_21() {
        let ratio = contrast(Color::rgb(0), Color::rgb(0xFFFFFF));
        assert!((ratio - 21.0).abs() < 0.01);
    }

    #[test]
    fn color_hex_parsing_splits_channels() {
        assert_eq!(
            Color::rgba(0x1D9E7566),
            Color {
                r: 0x1D,
                g: 0x9E,
                b: 0x75,
                a: 0x66
            }
        );
        assert_eq!(Color::rgb(0x5DCAA5).a, 0xFF);
    }

    #[test]
    fn flat_themes_have_square_corners_and_terminal_uses_only_the_mono_font() {
        assert_eq!(ThemeId::Minimal.palette().radius, 0);
        assert_eq!(ThemeId::Terminal.palette().radius, 0);
        let t = ThemeId::Terminal.palette();
        assert_eq!(t.label_font, t.number_font);
        assert_ne!(ThemeId::GamerHud.palette().label_font, t.label_font);
    }
}
