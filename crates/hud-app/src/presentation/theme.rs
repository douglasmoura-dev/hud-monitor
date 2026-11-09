//! Copies a core palette into the Slint `Theme` global of each window.

use hud_core::theme::{
    ascii_bar, lit_cells, scanline_rows, Color, Layout, Palette, ThemeId, TitleStrip,
};

/// Distance between scanlines, in logical pixels.
const SCANLINE_STEP: usize = 3;

/// One-pixel-wide image of a scanline period, tiled over the window by the Slint side.
fn scanline_tile(color: Color) -> slint::Image {
    let rows = scanline_rows(SCANLINE_STEP);
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(1, rows.len() as u32);
    for (pixel, dark) in pixels.make_mut_slice().iter_mut().zip(rows) {
        *pixel = if dark {
            slint::Rgba8Pixel::new(color.r, color.g, color.b, color.a)
        } else {
            slint::Rgba8Pixel::new(0, 0, 0, 0)
        };
    }
    slint::Image::from_rgba8(pixels)
}

pub fn slint_color(c: Color) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

/// Sets the non-color properties of a window's `Theme` and its pure helpers.
pub(super) fn paint_style(theme: &super::Theme<'_>, p: &Palette) {
    let s = &p.style;
    theme.set_title_font(s.title_font.into());
    theme.set_label_font(s.label_font.into());
    theme.set_number_font(s.number_font.into());
    theme.set_radius(f32::from(s.corner.radius()));
    theme.set_corner_cut(f32::from(s.corner.cut()));
    theme.set_bar_style(s.bar.id().into());
    theme.set_title(s.title.into());
    theme.set_borders(s.borders);
    theme.set_airy(s.layout == Layout::Airy);
    theme.set_cards(s.layout == Layout::Cards);
    theme.set_prompt(s.strip == TitleStrip::Prompt);
    theme.set_solid_strip(s.strip == TitleStrip::Solid);
    theme.set_has_side_rule(s.side_rule.is_some());
    if let Some(color) = s.side_rule {
        theme.set_side_rule(slint_color(color));
    }
    theme.set_has_scanlines(s.scanlines.is_some());
    if let Some(color) = s.scanlines {
        theme.set_scan_tile(scanline_tile(color));
    }
    theme.set_num_scale(f32::from(s.number_scale) / 100.0);
    theme.set_label_scale(f32::from(s.label_scale) / 100.0);
    theme.set_tracking(f32::from(s.tracking) / 10.0);
    theme.set_theme_id(p.id.index() as i32);
    theme.on_label_text(|id, key| {
        let theme = ThemeId::ALL
            .get(id.max(0) as usize)
            .copied()
            .unwrap_or_default();
        theme.style().label_case.apply(&key).into()
    });
    theme.on_ascii(|percent, cells| ascii_bar(percent, cells.max(0) as usize).into());
    theme.on_lit(|percent, cells| lit_cells(percent, cells.max(0) as usize) as i32);
}

/// Sets every `Theme` property of one window; the globals are per window, so each window needs it.
macro_rules! paint {
    ($window:expr, $palette:expr) => {{
        use slint::Global as _;
        let theme = $crate::presentation::Theme::get($window);
        let p: &hud_core::theme::Palette = $palette;
        let c = $crate::presentation::slint_color;
        theme.set_background(c(p.background));
        theme.set_teal(c(p.accent));
        theme.set_green(c(p.green));
        theme.set_purple(c(p.purple));
        theme.set_amber(c(p.amber));
        theme.set_blue(c(p.blue));
        theme.set_text(c(p.text));
        theme.set_dim(c(p.dim));
        theme.set_track(c(p.track));
        theme.set_border(c(p.border));
        theme.set_title_bar(c(p.title_bar));
        theme.set_tip_bar(c(p.tip_bar));
        theme.set_warn(c(p.warn));
        theme.set_crit(c(p.crit));
        theme.set_title_text(c(p.title_text));
        theme.set_title_icon(c(p.title_icon));
        theme.set_card(c(p.card));
        $crate::presentation::theme::paint_style(&theme, p);
    }};
}

pub fn paint_hud(window: &super::Hud, palette: &Palette) {
    paint!(window, palette);
}

pub fn paint_minibar(window: &super::MiniBar, palette: &Palette) {
    paint!(window, palette);
}

pub fn paint_scanner(window: &super::Scanner, palette: &Palette) {
    paint!(window, palette);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_keep_their_channels_and_alpha() {
        let c = slint_color(Color::rgba(0x1D9E7566));
        assert_eq!(
            (c.red(), c.green(), c.blue(), c.alpha()),
            (0x1D, 0x9E, 0x75, 0x66)
        );
    }
}
