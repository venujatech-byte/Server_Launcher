pub(crate) mod clipboard_image;
pub(crate) mod clipboard_sync;
pub mod credentials;
pub(crate) mod editor;
pub(crate) mod framebuffer;
pub(crate) mod input;
pub(crate) mod keymap;
pub(crate) mod probe;
pub(crate) mod profile;
pub(crate) mod rdp;
pub(crate) mod rdp_drives;
pub(crate) mod rdp_clipboard;
pub(crate) mod setup;
pub(crate) mod rdp_input;
mod recording;
pub(crate) mod screenshot;
pub(crate) mod session;
pub(crate) mod trust;
pub(crate) mod tunnel;
pub(crate) mod viewer;
pub(crate) mod vnc;
#[cfg(target_os = "linux")]
mod x11_keyboard;
#[cfg(target_os = "windows")]
mod windows_keyboard;

#[cfg(target_os = "linux")]
pub(crate) fn prepare_viewer_display_backend() {
    x11_keyboard::prefer_x11_when_available();
}
