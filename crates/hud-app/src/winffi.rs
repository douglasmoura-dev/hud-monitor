//! The only module allowed to use `unsafe` (Windows FFI).

use hud_core::position::Size;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// Primary monitor size in physical pixels.
pub fn screen_size() -> Size {
    // SAFETY: GetSystemMetrics takes a plain integer index, has no pointer args
    // and no preconditions; it cannot violate memory safety.
    unsafe {
        Size {
            width: GetSystemMetrics(SM_CXSCREEN),
            height: GetSystemMetrics(SM_CYSCREEN),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn screen_has_positive_size() {
        let s = super::screen_size();
        assert!(s.width > 0 && s.height > 0);
    }
}
