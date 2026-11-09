//! Visual themes: the five built-in palettes, their style (fonts, label case, corners, bars,
//! decorations) and the pure helpers the windows draw with.
//!
//! Palette and style are plain data; hud-app copies the chosen ones into the Slint `Theme` global.
//! There is no free-form color and no theme file (spec C7).

use crate::ui_text;

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

    /// Position in `ALL`; the Slint side uses it to tell themes apart.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn style(self) -> Style {
        self.palette().style
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
    /// Text and icon color on the title strip.
    pub title_text: Color,
    pub title_icon: Color,
    /// Fill of the metric cards; fully transparent when the theme has none.
    pub card: Color,
    pub style: Style,
    pub id: ThemeId,
}

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
    title_text: Color::rgb(0x5DCAA5),
    title_icon: Color::rgb(0x7D8B97),
    card: Color::rgba(0x00000000),
    style: GAMER_STYLE,
    id: ThemeId::GamerHud,
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
    title_bar: Color::rgb(0xFCEE0A),
    tip_bar: Color::rgb(0x1A0B2E),
    warn: Color::rgb(0xFFD300),
    crit: Color::rgb(0xFF2A6D),
    title_text: Color::rgb(0x0D0221),
    title_icon: Color::rgb(0x0D0221),
    card: Color::rgba(0x00000000),
    style: CYBERPUNK_STYLE,
    id: ThemeId::Cyberpunk,
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
    border: Color::rgba(0xFFFFFF00),
    title_bar: Color::rgba(0xFFFFFF00),
    tip_bar: Color::rgb(0x1E2227),
    warn: Color::rgb(0xE0A93B),
    crit: Color::rgb(0xE5675F),
    title_text: Color::rgb(0xE6EAEE),
    title_icon: Color::rgb(0x8A939B),
    card: Color::rgba(0x00000000),
    style: MINIMAL_STYLE,
    id: ThemeId::Minimal,
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
    title_text: Color::rgb(0x33FF66),
    title_icon: Color::rgb(0x4FA468),
    card: Color::rgba(0x00000000),
    style: TERMINAL_STYLE,
    id: ThemeId::Terminal,
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
    title_text: Color::rgb(0x0B7A5C),
    title_icon: Color::rgb(0x5C6773),
    card: Color::rgb(0xFFFFFF),
    style: LIGHT_STYLE,
    id: ThemeId::Light,
};

/// How a bar is drawn; picked per theme, never by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    Thin,
    Segmented,
    Ascii,
    None,
    Rounded,
}

impl BarStyle {
    /// Name the Slint side matches on.
    pub fn id(self) -> &'static str {
        match self {
            Self::Thin => "thin",
            Self::Segmented => "segmented",
            Self::Ascii => "ascii",
            Self::None => "none",
            Self::Rounded => "rounded",
        }
    }
}

/// Shape of a window corner: rounded by a radius (0 is square) or cut on the diagonal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    Round(u8),
    Cut(u8),
}

impl Corner {
    pub fn radius(self) -> u8 {
        match self {
            Self::Round(r) => r,
            Self::Cut(_) => 0,
        }
    }

    pub fn cut(self) -> u8 {
        match self {
            Self::Round(_) => 0,
            Self::Cut(c) => c,
        }
    }
}

/// Case rule of the section labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelCase {
    /// As written in `ui_text`.
    AsIs,
    Lower,
    /// Upper case joined by underscores, like `CPU_LOAD`.
    Snake,
}

impl LabelCase {
    /// Label shown for a `ui_text` key (the upper-case section names).
    pub fn apply(self, key: &str) -> String {
        match self {
            Self::AsIs => key.to_string(),
            Self::Lower => key.to_lowercase(),
            Self::Snake => ui_text::snake_label(key),
        }
    }
}

/// Look of the title strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleStrip {
    /// Faint tint over the window background.
    Subtle,
    /// Solid color from the palette with dark text.
    Solid,
    /// Shell prompt text with a blinking cursor.
    Prompt,
}

/// Overall arrangement of the HUD sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Standard,
    /// Large numbers, no rules or bars, details on one dim line.
    Airy,
    /// Main metrics in cards with a status dot.
    Cards,
}

/// Everything about a theme that is not a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub title_font: &'static str,
    pub label_font: &'static str,
    pub number_font: &'static str,
    pub label_case: LabelCase,
    pub corner: Corner,
    pub bar: BarStyle,
    pub strip: TitleStrip,
    pub layout: Layout,
    pub borders: bool,
    /// Left accent rule color, if the theme has one.
    pub side_rule: Option<Color>,
    /// Color of the scanlines drawn over the window, if any.
    pub scanlines: Option<Color>,
    /// Font size of numbers and labels, in percent of the base size.
    pub number_scale: u16,
    pub label_scale: u16,
    /// Letter spacing of the labels, in tenths of a pixel.
    pub tracking: u8,
    /// Title strip text (a `ui_text` constant).
    pub title: &'static str,
}

const GAMER_STYLE: Style = Style {
    title_font: "Rajdhani",
    label_font: "Rajdhani",
    number_font: "Share Tech Mono",
    label_case: LabelCase::AsIs,
    corner: Corner::Round(6),
    bar: BarStyle::Thin,
    strip: TitleStrip::Subtle,
    layout: Layout::Standard,
    borders: true,
    side_rule: None,
    scanlines: None,
    number_scale: 100,
    label_scale: 100,
    tracking: 15,
    title: ui_text::TITLE_GAMER,
};

const CYBERPUNK_STYLE: Style = Style {
    title_font: "Orbitron",
    label_font: "Chakra Petch",
    number_font: "Orbitron",
    label_case: LabelCase::Snake,
    corner: Corner::Cut(8),
    bar: BarStyle::Segmented,
    strip: TitleStrip::Solid,
    layout: Layout::Standard,
    borders: true,
    side_rule: Some(Color::rgb(0xFF2E88)),
    scanlines: None,
    number_scale: 90,
    label_scale: 100,
    tracking: 12,
    title: ui_text::TITLE_CYBERPUNK,
};

const MINIMAL_STYLE: Style = Style {
    title_font: "Inter",
    label_font: "Inter",
    number_font: "Inter",
    label_case: LabelCase::Lower,
    corner: Corner::Round(18),
    bar: BarStyle::None,
    strip: TitleStrip::Subtle,
    layout: Layout::Airy,
    borders: false,
    side_rule: None,
    scanlines: None,
    number_scale: 100,
    label_scale: 100,
    tracking: 0,
    title: ui_text::TITLE_MINIMAL,
};

const TERMINAL_STYLE: Style = Style {
    title_font: "VT323",
    label_font: "VT323",
    number_font: "VT323",
    label_case: LabelCase::AsIs,
    corner: Corner::Round(0),
    bar: BarStyle::Ascii,
    strip: TitleStrip::Prompt,
    layout: Layout::Standard,
    borders: true,
    side_rule: None,
    scanlines: Some(Color::rgba(0x33FF6618)),
    number_scale: 125,
    label_scale: 125,
    tracking: 5,
    title: ui_text::TITLE_TERMINAL,
};

const LIGHT_STYLE: Style = Style {
    title_font: "Segoe UI",
    label_font: "Segoe UI",
    number_font: "Segoe UI",
    label_case: LabelCase::AsIs,
    corner: Corner::Round(10),
    bar: BarStyle::Rounded,
    strip: TitleStrip::Subtle,
    layout: Layout::Cards,
    borders: true,
    side_rule: None,
    scanlines: None,
    number_scale: 100,
    label_scale: 100,
    tracking: 5,
    title: ui_text::TITLE_LIGHT,
};

/// Number of cells (`#` or segments) lit for `percent`, out of `cells`.
pub fn lit_cells(percent: f32, cells: usize) -> usize {
    if percent.is_nan() {
        return 0;
    }
    let share = percent.clamp(0.0, 100.0) / 100.0;
    ((share * cells as f32).round() as usize).min(cells)
}

/// ASCII bar like `[###.......]` with `cells` characters between the brackets.
pub fn ascii_bar(percent: f32, cells: usize) -> String {
    let lit = lit_cells(percent, cells);
    format!("[{}{}]", "#".repeat(lit), ".".repeat(cells - lit))
}

/// Rows of one scanline tile: `true` marks the dark row, one per `step` rows.
pub fn scanline_rows(step: usize) -> Vec<bool> {
    let step = step.max(1);
    (0..step).map(|row| row == step - 1).collect()
}

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
    fn terminal_uses_only_the_mono_font_and_flat_themes_have_no_radius() {
        let t = ThemeId::Terminal.style();
        assert_eq!(t.title_font, "VT323");
        assert_eq!(t.label_font, t.number_font);
        assert_eq!(t.label_font, t.title_font);
        assert_eq!(t.corner, Corner::Round(0));
        assert_ne!(ThemeId::GamerHud.style().label_font, t.label_font);
    }

    #[test]
    fn gamer_hud_keeps_its_original_look() {
        let s = ThemeId::GamerHud.style();
        assert_eq!(s.label_font, "Rajdhani");
        assert_eq!(s.number_font, "Share Tech Mono");
        assert_eq!(s.label_case, LabelCase::AsIs);
        assert_eq!(s.corner, Corner::Round(6));
        assert_eq!(s.bar, BarStyle::Thin);
        assert_eq!((s.number_scale, s.label_scale), (100, 100));
    }

    #[test]
    fn no_two_themes_share_number_font_bar_style_and_corner() {
        for (i, a) in ThemeId::ALL.iter().enumerate() {
            for b in &ThemeId::ALL[i + 1..] {
                let (sa, sb) = (a.style(), b.style());
                assert!(
                    (sa.number_font, sa.bar, sa.corner) != (sb.number_font, sb.bar, sb.corner),
                    "{} and {} look alike",
                    a.id(),
                    b.id()
                );
            }
        }
    }

    #[test]
    fn every_theme_has_its_own_identity_beyond_the_triple() {
        let cyber = ThemeId::Cyberpunk.style();
        assert_eq!(cyber.title_font, "Orbitron");
        assert_eq!(cyber.number_font, "Orbitron");
        assert_eq!(cyber.label_font, "Chakra Petch");
        assert_eq!(cyber.corner, Corner::Cut(8));
        assert_eq!(cyber.bar, BarStyle::Segmented);
        assert_eq!(cyber.strip, TitleStrip::Solid);
        assert_eq!(cyber.side_rule, Some(Color::rgb(0xFF2E88)));
        let p = ThemeId::Cyberpunk.palette();
        assert_eq!(p.title_bar, Color::rgb(0xFCEE0A));
        assert!(contrast(p.title_text, p.title_bar) >= 7.0);

        let min = ThemeId::Minimal.style();
        assert_eq!(min.number_font, "Inter");
        assert_eq!(min.bar, BarStyle::None);
        assert_eq!(min.label_case, LabelCase::Lower);
        assert!(!min.borders);
        assert!(matches!(min.corner, Corner::Round(r) if r >= 16));
        assert!(min.layout == Layout::Airy);

        let term = ThemeId::Terminal.style();
        assert_eq!(term.bar, BarStyle::Ascii);
        assert_eq!(term.strip, TitleStrip::Prompt);
        assert!(term.scanlines.is_some());
        assert_eq!(term.title, "user@hud:~$");

        let light = ThemeId::Light.style();
        assert_eq!(light.number_font, "Segoe UI");
        assert_eq!(light.layout, Layout::Cards);
        assert_eq!(light.bar, BarStyle::Rounded);
    }

    #[test]
    fn only_light_has_cards_only_cyberpunk_a_side_rule_only_terminal_scanlines() {
        for t in ThemeId::ALL {
            assert_eq!(t.palette().card.a != 0, t == ThemeId::Light, "{}", t.id());
            assert_eq!(t.style().side_rule.is_some(), t == ThemeId::Cyberpunk);
            assert_eq!(t.style().scanlines.is_some(), t == ThemeId::Terminal);
        }
    }

    #[test]
    fn ascii_bar_has_a_fixed_width_and_proportional_hashes() {
        assert_eq!(ascii_bar(0.0, 10), "[..........]");
        assert_eq!(ascii_bar(34.0, 10), "[###.......]");
        assert_eq!(ascii_bar(100.0, 10), "[##########]");
        assert_eq!(ascii_bar(34.0, 12).matches('#').count(), 4);
        for p in [0.0, 12.5, 34.0, 99.0, 100.0] {
            assert_eq!(ascii_bar(p, 12).len(), 14);
        }
    }

    #[test]
    fn ascii_bar_clamps_out_of_range_and_unknown_percentages() {
        assert_eq!(ascii_bar(-5.0, 4), "[....]");
        assert_eq!(ascii_bar(250.0, 4), "[####]");
        assert_eq!(ascii_bar(f32::NAN, 4), "[....]");
        assert_eq!(ascii_bar(50.0, 0), "[]");
        assert_eq!(lit_cells(34.0, 20), 7);
    }

    #[test]
    fn label_case_lowercases_for_minimal_and_snake_cases_for_cyberpunk() {
        let min = ThemeId::Minimal.style().label_case;
        let cyber = ThemeId::Cyberpunk.style().label_case;
        assert_eq!(min.apply("CPU"), "cpu");
        assert_eq!(min.apply("LIMITES IA"), "limites ia");
        assert_eq!(min.apply("MÊS"), "mês");
        assert_eq!(cyber.apply("CPU"), "CPU_LOAD");
        assert_eq!(cyber.apply("LIMITES IA"), "LIMITES_IA");
        assert_eq!(cyber.apply("CLAUDE"), "CLAUDE");
    }

    #[test]
    fn other_themes_keep_labels_as_in_ui_text() {
        for t in [ThemeId::GamerHud, ThemeId::Terminal, ThemeId::Light] {
            assert_eq!(t.style().label_case.apply("DISCO"), "DISCO");
            assert_eq!(t.style().label_case.apply("LIMITES IA"), "LIMITES IA");
        }
    }

    #[test]
    fn scanline_tile_has_one_dark_row_per_step() {
        assert_eq!(scanline_rows(3), [false, false, true]);
        assert_eq!(scanline_rows(1), [true]);
        assert_eq!(scanline_rows(0), [true]);
    }

    #[test]
    fn bar_style_ids_are_unique() {
        let ids = [
            BarStyle::Thin,
            BarStyle::Segmented,
            BarStyle::Ascii,
            BarStyle::None,
            BarStyle::Rounded,
        ]
        .map(BarStyle::id);
        for (i, a) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(a));
        }
    }
}
