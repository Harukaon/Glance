#[cfg(not(target_os = "macos"))]
use screenshots::Screen as CaptureScreen;
#[cfg(target_os = "macos")]
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(target_os = "macos")]
pub struct MockMonitor {}

#[cfg(target_os = "macos")]
type CaptureScreen = MockMonitor;

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

#[cfg(target_os = "macos")]
pub struct InteractiveCaptureImage {
    pub png_bytes: Vec<u8>,
    pub rgba_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[cfg(target_os = "macos")]
pub struct CapturedScreenImage {
    pub preview_bytes: Vec<u8>,
    pub preview_mime: &'static str,
    pub rgba_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Find the primary monitor (fast, ~5ms). Returns the Screen handle and display info.
#[cfg(not(target_os = "macos"))]
pub fn find_primary_screen() -> AppResult<PrimaryMonitorInfo> {
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
#[cfg(not(target_os = "macos"))]
pub fn capture_screen_to_memory(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    let t0 = std::time::Instant::now();
    let capture = screen
        .capture()
        .map_err(|e| AppError::Capture(e.to_string()))?;
    #[cfg(target_os = "windows")]
    tracing::info!(
        "[PERF][capture] screen.capture() (BitBlt): {:?}",
        t0.elapsed()
    );
    #[cfg(target_os = "macos")]
    tracing::info!(
        "[PERF][capture] screen.capture() (CoreGraphics): {:?}",
        t0.elapsed()
    );
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
pub fn find_primary_screen() -> AppResult<PrimaryMonitorInfo> {
    find_primary_screen_macos()
}

#[cfg(target_os = "macos")]
pub fn capture_screen_to_memory(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    capture_screen_to_memory_macos(screen)
}

#[cfg(target_os = "macos")]
pub fn capture_interactive_region() -> AppResult<Option<InteractiveCaptureImage>> {
    capture_interactive_region_macos()
}

#[cfg(target_os = "macos")]
pub fn capture_screen_with_preview(screen: CaptureScreen) -> AppResult<CapturedScreenImage> {
    capture_screen_with_preview_macos(screen)
}

#[cfg(target_os = "macos")]
fn find_primary_screen_macos() -> AppResult<PrimaryMonitorInfo> {
    use core_graphics::display::CGDisplay;

    let t0 = std::time::Instant::now();
    let main_display = CGDisplay::main();
    let bounds = main_display.bounds();
    
    let logical_w = bounds.size.width;
    let logical_h = bounds.size.height;
    let pixels_w = main_display.pixels_wide();
    
    let scale_factor = if logical_w > 0.0 {
        (pixels_w as f64) / logical_w
    } else {
        1.0
    };

    let x = bounds.origin.x as i32;
    let y = bounds.origin.y as i32;
    let width = logical_w as u32;
    let height = logical_h as u32;

    tracing::info!("[PERF][capture] CGDisplay::main: {:?}", t0.elapsed());

    debug_log(format!(
        "[monitor] primary x={} y={} width={} height={} scale_factor={}",
        x, y, width, height, scale_factor
    ));

    Ok(PrimaryMonitorInfo {
        screen: MockMonitor {},
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
    fs::create_dir_all(&dir).map_err(|e| {
        AppError::Capture(format!(
            "failed to create debug dir {DEBUG_CAPTURE_DIR}: {e}"
        ))
    })?;
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
fn capture_screen_to_memory_macos(_screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    let captured = capture_screen_with_preview_macos(_screen)?;
    Ok((captured.rgba_bytes, captured.width, captured.height))
}

#[cfg(target_os = "macos")]
fn capture_screen_with_preview_macos(_screen: CaptureScreen) -> AppResult<CapturedScreenImage> {
    let started = std::time::Instant::now();
    let capture_path = temp_capture_path("jpg");

    let status = Command::new("screencapture")
        .args(["-x", "-m", "-t", "jpg", capture_path.to_string_lossy().as_ref()])
        .status()
        .map_err(|e| AppError::Capture(format!("failed to run screencapture: {e}")))?;

    if !status.success() {
        return Err(AppError::Capture(format!(
            "screencapture exited with status {status}"
        )));
    }

    let jpeg_bytes = fs::read(&capture_path)?;
    let _ = fs::remove_file(&capture_path);
    tracing::info!("[PERF][capture] screencapture jpg: {:?}", started.elapsed());

    let decode_started = std::time::Instant::now();
    let image = image::load_from_memory(&jpeg_bytes)
        .map_err(AppError::Image)?
        .into_rgba8();
    tracing::info!(
        "[PERF][capture] decode screenshot jpg: {:?}",
        decode_started.elapsed()
    );

    let w = image.width();
    let h = image.height();
    if should_write_debug_capture_images() {
        debug_write_bytes("01_screencapture.jpg", &jpeg_bytes);
        if let Ok(png_bytes) = encode_rgba_png(image.clone()) {
            debug_write_bytes("01_screencapture.png", &png_bytes);
        }
    }
    let rgba_bytes = image.into_raw();
    debug_log(format!(
        "[capture] screencapture -> {}x{} rgba_bytes={}",
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

    Ok(CapturedScreenImage {
        preview_bytes: jpeg_bytes,
        preview_mime: "image/jpeg",
        rgba_bytes,
        width: w,
        height: h,
    })
}

#[cfg(target_os = "macos")]
fn capture_interactive_region_macos() -> AppResult<Option<InteractiveCaptureImage>> {
    let started = std::time::Instant::now();
    let capture_path = temp_capture_path("png");

    let output = Command::new("screencapture")
        .args([
            "-i",
            "-x",
            "-t",
            "png",
            capture_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|e| AppError::Capture(format!("failed to run interactive screencapture: {e}")))?;

    let capture_exists = capture_path.exists();
    if !output.status.success() && !capture_exists {
        debug_log(format!(
            "[capture] interactive screencapture cancelled status={:?}",
            output.status.code()
        ));
        return Ok(None);
    }

    if !capture_exists {
        debug_log("[capture] interactive screencapture finished without output file");
        return Ok(None);
    }

    let png_bytes = fs::read(&capture_path)?;
    let _ = fs::remove_file(&capture_path);

    if png_bytes.is_empty() {
        debug_log("[capture] interactive screencapture produced empty file");
        return Ok(None);
    }

    tracing::info!(
        "[PERF][capture] screencapture interactive png: {:?}",
        started.elapsed()
    );

    let decode_started = std::time::Instant::now();
    let image = image::load_from_memory_with_format(&png_bytes, image::ImageFormat::Png)
        .map_err(AppError::Image)?
        .into_rgba8();
    tracing::info!(
        "[PERF][capture] decode interactive screenshot png: {:?}",
        decode_started.elapsed()
    );

    let width = image.width();
    let height = image.height();
    let rgba_bytes = image.into_raw();

    debug_log(format!(
        "[capture] interactive screencapture -> {}x{} rgba_bytes={} status={:?}",
        width,
        height,
        rgba_bytes.len(),
        output.status.code()
    ));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug_log(format!(
            "[capture] interactive screencapture returned non-zero but wrote output: {}",
            stderr.trim()
        ));
    }

    Ok(Some(InteractiveCaptureImage {
        png_bytes,
        rgba_bytes,
        width,
        height,
    }))
}

#[cfg(target_os = "macos")]
fn should_write_debug_capture_images() -> bool {
    std::env::var_os("GLANCE_CAPTURE_DEBUG_IMAGES").is_some()
}

#[cfg(target_os = "macos")]
fn temp_capture_path(ext: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    PathBuf::from(format!("/private/tmp/glance-capture-{ts}.{ext}"))
}

#[cfg(target_os = "macos")]
fn encode_rgba_png(image: image::RgbaImage) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| AppError::Capture(format!("debug png encode failed: {e}")))?;
    Ok(bytes)
}
