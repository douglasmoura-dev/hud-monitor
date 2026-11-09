//! Copies a core palette into the Slint `Theme` global of each window.

use hud_core::theme::{Color, Palette};

pub fn slint_color(c: Color) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
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
        theme.set_label_font(p.label_font.into());
        theme.set_number_font(p.number_font.into());
        theme.set_radius(f32::from(p.radius));
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
