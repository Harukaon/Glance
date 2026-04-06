/// macOS screen recording permission helpers.
///
/// On macOS 10.15+, capturing the screen requires the "Screen Recording"
/// permission. If the user hasn't granted it, the capture will return a
/// black/blank image. This module provides functions to check the permission
/// status and open System Preferences so the user can grant access.

#[cfg(target_os = "macos")]
mod imp {
    use core_graphics::access::ScreenCaptureAccess;
    use std::process::Command;

    pub fn has_screen_recording_permission() -> bool {
        ScreenCaptureAccess::default().preflight()
    }

    pub fn request_screen_recording_permission() -> bool {
        let granted = ScreenCaptureAccess::default().request();
        if !granted {
            // Also open System Preferences as a user-friendly hint.
            Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
                .status()
                .ok();
        }
        granted
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// On non-macOS platforms, screen recording permission is always "granted".
    pub fn has_screen_recording_permission() -> bool {
        true
    }

    /// No-op on non-macOS platforms.
    pub fn request_screen_recording_permission() -> bool {
        false
    }
}

pub use imp::{has_screen_recording_permission, request_screen_recording_permission};
