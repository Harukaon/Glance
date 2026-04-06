#[cfg(target_os = "macos")]
use xcap::Monitor as CaptureScreen;
#[cfg(not(target_os = "macos"))]
use screenshots::Screen as CaptureScreen;
#[cfg(target_os = "macos")]
use std::{fs, io::Cursor, path::PathBuf};

use crate::error::{AppError, AppResult};

#[cfg(target_os = "macos")]
const DEBUG_CAPTURE_DIR: &str = "/tmp/glance-debug/latest";

/// Monitor info returned by find_primary_screen.
pub struct PrimaryMonitorInfo {
    pub screen: CaptureScreen,
    pub scale_factor: f64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Find the primary monitor (fast, ~5ms). Returns the Screen handle and display info.
pub fn find_primary_screen() -> AppResult<PrimaryMonitorInfo> {
    #[cfg(target_os = "macos")]
    {
        return find_primary_screen_macos();
    }

    let t0 = std::time::Instant::now();
    let screens = CaptureScreen::all().map_err(|e| AppError::Capture(e.to_string()))?;
    tracing::info!("[PERF][capture] Screen::all(): {:?}", t0.elapsed());

    let primary = screens
        .into_iter()
        .find(|s| s.display_info.is_primary)
        .ok_or_else(|| AppError::Capture("no primary monitor found".into()))?;

    let scale_factor = primary.display_info.scale_factor as f64;
    let x = primary.display_info.x;
    let y = primary.display_info.y;
    let width = primary.display_info.width;
    let height = primary.display_info.height;

    Ok(PrimaryMonitorInfo {
        screen: primary,
        scale_factor,
        x,
        y,
        width,
        height,
    })
}

/// Capture the screen to raw RGBA bytes in memory (no file I/O).
pub fn capture_screen_to_memory(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    #[cfg(target_os = "macos")]
    {
        return capture_screen_to_memory_macos(screen);
    }

    let t0 = std::time::Instant::now();
    let capture = screen
        .capture()
        .map_err(|e| AppError::Capture(e.to_string()))?;
    #[cfg(target_os = "windows")]
    tracing::info!("[PERF][capture] screen.capture() (BitBlt): {:?}", t0.elapsed());
    #[cfg(target_os = "macos")]
    tracing::info!("[PERF][capture] screen.capture() (CoreGraphics): {:?}", t0.elapsed());
    #[cfg(target_os = "linux")]
    tracing::info!("[PERF][capture] screen.capture(): {:?}", t0.elapsed());

    let w = capture.width();
    let h = capture.height();
    let rgba_bytes = capture.into_raw();
    tracing::info!(
        "[PERF][capture] raw RGBA bytes: {} ({:.1} MB), {}x{}",
        rgba_bytes.len(),
        rgba_bytes.len() as f64 / 1_048_576.0,
        w,
        h
    );

    Ok((rgba_bytes, w, h))
}

#[cfg(target_os = "macos")]
fn find_primary_screen_macos() -> AppResult<PrimaryMonitorInfo> {
    let t0 = std::time::Instant::now();
    let monitors = CaptureScreen::all().map_err(|e| AppError::Capture(e.to_string()))?;
    tracing::info!("[PERF][capture] Monitor::all(): {:?}", t0.elapsed());

    let primary = monitors
        .into_iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .ok_or_else(|| AppError::Capture("no primary monitor found".into()))?;

    let scale_factor = primary.scale_factor().unwrap_or(1.0) as f64;
    let x = primary.x().unwrap_or(0);
    let y = primary.y().unwrap_or(0);
    let width = primary.width().unwrap_or(0);
    let height = primary.height().unwrap_or(0);

    debug_log(format!(
        "[monitor] primary x={} y={} width={} height={} scale_factor={}",
        x, y, width, height, scale_factor
    ));

    Ok(PrimaryMonitorInfo {
        screen: primary,
        scale_factor,
        x,
        y,
        width,
        height,
    })
}

#[cfg(target_os = "macos")]
pub fn debug_reset_dir() -> AppResult<PathBuf> {
    let dir = PathBuf::from(DEBUG_CAPTURE_DIR);
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Capture(format!("failed to create debug dir {DEBUG_CAPTURE_DIR}: {e}")))?;
    Ok(dir)
}

#[cfg(target_os = "macos")]
pub fn debug_dir() -> PathBuf {
    PathBuf::from(DEBUG_CAPTURE_DIR)
}

#[cfg(target_os = "macos")]
pub fn debug_log(message: impl AsRef<str>) {
    let dir = PathBuf::from(DEBUG_CAPTURE_DIR);
    let _ = fs::create_dir_all(&dir);
    let log_path = dir.join("capture.log");
    let line = format!("{}\n", message.as_ref());
    use std::io::Write;
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(target_os = "macos")]
pub fn debug_write_bytes(file_name: &str, bytes: &[u8]) {
    let dir = PathBuf::from(DEBUG_CAPTURE_DIR);
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join(file_name), bytes);
}

#[cfg(target_os = "macos")]
fn capture_screen_to_memory_macos(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    let t0 = std::time::Instant::now();
    let image = screen
        .capture_image()
        .map_err(|e| AppError::Capture(format!("xcap capture_image failed: {e}")))?;
    tracing::info!("[PERF][capture] xcap capture_image(): {:?}", t0.elapsed());
    let w = image.width();
    let h = image.height();
    if let Ok(png_bytes) = encode_rgba_png(image.clone()) {
        debug_write_bytes("01_xcap_capture.png", &png_bytes);
    }
    let rgba_bytes = image.into_raw();
    debug_log(format!(
        "[capture] xcap capture -> {}x{} rgba_bytes={}",
        w,
        h,
        rgba_bytes.len()
    ));
    tracing::info!(
        "[PERF][capture] raw RGBA bytes: {} ({:.1} MB), {}x{}",
        rgba_bytes.len(),
        rgba_bytes.len() as f64 / 1_048_576.0,
        w,
        h
    );

    Ok((rgba_bytes, w, h))
}

#[cfg(target_os = "macos")]
fn encode_rgba_png(image: image::RgbaImage) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| AppError::Capture(format!("debug png encode failed: {e}")))?;
    Ok(bytes)
}
