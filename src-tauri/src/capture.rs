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

use crate::error::{AppError, AppResult};

#[cfg(target_os = "macos")]
const DEBUG_CAPTURE_DIR: &str = "/tmp/glance-debug/latest";

/// Monitor info returned by find_cursor_monitor / find_primary_screen.
#[derive(Clone, Copy)]
pub struct MonitorInfo {
    pub scale_factor: f64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[cfg(target_os = "macos")]
    pub display_id: u32,
}

#[cfg(not(target_os = "macos"))]
pub struct CursorMonitorResult {
    pub screen: CaptureScreen,
    pub monitor: MonitorInfo,
}

/// A single RGBA snapshot of the complete virtual desktop.
#[cfg(not(target_os = "macos"))]
pub struct VirtualDesktopCapture {
    pub rgba: Vec<u8>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub monitor_count: usize,
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

// ── Cursor-aware monitor detection ──────────────────────────────────────────

/// Find the monitor that the cursor is currently on.
#[cfg(not(target_os = "macos"))]
pub fn find_cursor_monitor() -> AppResult<CursorMonitorResult> {
    let (cursor_x, cursor_y) = get_cursor_position()
        .map_err(|e| AppError::Capture(format!("failed to get cursor position: {e}")))?;

    // Screen::from_point finds the screen containing the given point
    let screen = CaptureScreen::from_point(cursor_x, cursor_y)
        .map_err(|e| AppError::Capture(format!("no screen at cursor ({cursor_x},{cursor_y}): {e}")))?;

    let info = &screen.display_info;
    Ok(CursorMonitorResult {
        screen,
        monitor: MonitorInfo {
            scale_factor: info.scale_factor as f64,
            x: info.x,
            y: info.y,
            width: info.width,
            height: info.height,
        },
    })
}

/// Find the monitor that the cursor is currently on.
#[cfg(target_os = "macos")]
pub fn find_cursor_monitor() -> AppResult<MonitorInfo> {
    use core_graphics::display::CGDisplay;
    use core_graphics::geometry::CGPoint;

    let t0 = std::time::Instant::now();

    // Get cursor position via NSEvent (AppKit coords: lower-left origin, Y up).
    // CGDisplay::bounds() uses CG coords: upper-left origin, Y down.
    // We need to convert: cg_y = main_bounds.origin.y + main_bounds.size.height - ns_y
    let main_display = CGDisplay::main();
    let main_bounds = main_display.bounds();

    let ns_point = unsafe { objc2_app_kit::NSEvent::mouseLocation() };
    let cg_x = ns_point.x;
    let cg_y = main_bounds.origin.y + main_bounds.size.height - ns_point.y;
    let cg_point = CGPoint::new(cg_x, cg_y);

    debug_log(format!(
        "[monitor] cursor ns=({}, {}) cg=({}, {})",
        ns_point.x, ns_point.y, cg_x, cg_y
    ));

    // Find which display contains this CG point
    match CGDisplay::displays_with_point(cg_point, 16) {
        Ok((display_ids, count)) if count > 0 => {
            for &id in &display_ids[..count as usize] {
                let display = CGDisplay::new(id);
                if !display.is_active() {
                    continue;
                }
                // Skip mirrored displays — prefer the primary in a mirror set
                if display.is_in_mirror_set() && display.mirrors_display() != 0 {
                    continue;
                }

                let bounds = display.bounds();
                let logical_w = bounds.size.width;
                let pixels_w = display.pixels_wide();
                let scale_factor = if logical_w > 0.0 {
                    (pixels_w as f64) / logical_w
                } else {
                    1.0
                };

                debug_log(format!(
                    "[monitor] found cursor display id={} x={} y={} w={} h={} scale={}",
                    id,
                    bounds.origin.x,
                    bounds.origin.y,
                    bounds.size.width,
                    bounds.size.height,
                    scale_factor
                ));

                tracing::info!("[PERF][capture] find_cursor_monitor: {:?}", t0.elapsed());

                return Ok(MonitorInfo {
                    scale_factor,
                    x: bounds.origin.x as i32,
                    y: bounds.origin.y as i32,
                    width: logical_w as u32,
                    height: bounds.size.height as u32,
                    display_id: id,
                });
            }
        }
        _ => {}
    }

    // Fallback to main display
    debug_log("[monitor] no display found for cursor, falling back to main");
    find_primary_screen()
}

/// Find the primary monitor (fallback / self-test).
#[cfg(not(target_os = "macos"))]
pub fn find_primary_screen() -> AppResult<CursorMonitorResult> {
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

    Ok(CursorMonitorResult {
        screen: primary,
        monitor: MonitorInfo {
            scale_factor,
            x,
            y,
            width,
            height,
        },
    })
}

/// Find the primary monitor (fallback / self-test).
#[cfg(target_os = "macos")]
pub fn find_primary_screen() -> AppResult<MonitorInfo> {
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

    Ok(MonitorInfo {
        scale_factor,
        x,
        y,
        width,
        height,
        display_id: main_display.id,
    })
}

// ── Cursor position (non-macOS) ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn get_cursor_position() -> Result<(i32, i32), String> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    extern "system" {
        fn GetCursorPos(lpPoint: *mut Point) -> i32;
    }
    let mut pt = Point { x: 0, y: 0 };
    let result = unsafe { GetCursorPos(&mut pt) };
    if result == 0 {
        return Err("GetCursorPos failed".into());
    }
    Ok((pt.x, pt.y))
}

#[cfg(target_os = "linux")]
fn get_cursor_position() -> Result<(i32, i32), String> {
    // On Linux X11, use XQueryPointer to get cursor position.
    // Fall back to (0, 0) which will resolve to the primary display.
    // Wayland does not expose global cursor coordinates; from_point
    // will still find a valid display.
    #[cfg(feature = "x11")]
    {
        // Attempt X11 if available
        use std::ffi::c_void;
        use std::ptr;

        extern "C" {
            fn XOpenDisplay(name: *const c_void) -> *mut c_void;
            fn XCloseDisplay(display: *mut c_void);
            fn XDefaultRootWindow(display: *mut c_void) -> u64;
            fn XQueryPointer(
                display: *mut c_void,
                window: u64,
                root_return: *mut u64,
                child_return: *mut u64,
                root_x_return: *mut i32,
                root_y_return: *mut i32,
                win_x_return: *mut i32,
                win_y_return: *mut i32,
                mask_return: *mut u32,
            ) -> i32;
        }

        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Err("XOpenDisplay failed".into());
        }
        let root = unsafe { XDefaultRootWindow(display) };
        let mut root_x = 0i32;
        let mut root_y = 0i32;
        unsafe {
            XQueryPointer(
                display,
                root,
                &mut 0u64,
                &mut 0u64,
                &mut root_x,
                &mut root_y,
                &mut 0i32,
                &mut 0i32,
                &mut 0u32,
            );
            XCloseDisplay(display);
        }
        return Ok((root_x, root_y));
    }

    #[allow(unreachable_code)]
    Err("Linux cursor position not available without x11".into())
}

// ── Screen capture ──────────────────────────────────────────────────────────

/// Capture the screen to raw RGBA bytes in memory (no file I/O).
///
/// On Windows, `display-info` geometry is already in the process DPI space and
/// `screenshots::Screen::capture()` multiplies by `scale_factor` again, which
/// breaks mixed-DPI layouts (e.g. 100% + 125%) and yields a black overlay.
/// Use `capture_area_ignore_area_check` with the reported size instead.
#[cfg(target_os = "windows")]
pub fn capture_screen_to_memory(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    let t0 = std::time::Instant::now();
    let info = screen.display_info;
    let capture = screen
        .capture_area_ignore_area_check(0, 0, info.width, info.height)
        .map_err(|e| AppError::Capture(e.to_string()))?;
    tracing::info!(
        "[PERF][capture] screen.capture_area_ignore_area_check (BitBlt): {:?}",
        t0.elapsed()
    );

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

/// Capture the screen to raw RGBA bytes in memory (no file I/O).
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub fn capture_screen_to_memory(screen: CaptureScreen) -> AppResult<(Vec<u8>, u32, u32)> {
    let t0 = std::time::Instant::now();
    let capture = screen
        .capture()
        .map_err(|e| AppError::Capture(e.to_string()))?;
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
pub fn capture_screen_to_memory(display_id: u32) -> AppResult<(Vec<u8>, u32, u32)> {
    let captured = capture_screen_with_preview(display_id)?;
    Ok((captured.rgba_bytes, captured.width, captured.height))
}

#[cfg(target_os = "macos")]
pub fn capture_screen_with_preview(display_id: u32) -> AppResult<CapturedScreenImage> {
    capture_screen_with_preview_macos(display_id)
}

#[cfg(target_os = "macos")]
pub fn capture_interactive_region() -> AppResult<Option<InteractiveCaptureImage>> {
    capture_interactive_region_macos()
}

/// Capture every display and combine the snapshots into one virtual desktop.
/// Window coordinates and selection coordinates use physical pixels.
#[cfg(not(target_os = "macos"))]
pub fn capture_virtual_desktop_to_memory() -> AppResult<VirtualDesktopCapture> {
    let screens = CaptureScreen::all().map_err(|e| AppError::Capture(e.to_string()))?;
    if screens.is_empty() {
        return Err(AppError::Capture("no monitors found".into()));
    }

    let mut snapshots = Vec::with_capacity(screens.len());
    for screen in screens {
        let info = &screen.display_info;
        // Windows: display-info x/y/width/height are already physical pixels.
        // Linux/other: treat them as logical and scale to match Screen::capture().
        #[cfg(target_os = "windows")]
        let (x, y, expected_width, expected_height) =
            (info.x, info.y, info.width, info.height);
        #[cfg(not(target_os = "windows"))]
        let (x, y, expected_width, expected_height) = physical_display_geometry(
            info.x,
            info.y,
            info.width,
            info.height,
            info.scale_factor,
        )?;
        let (rgba, width, height) = capture_screen_to_memory(screen)?;
        if width != expected_width || height != expected_height {
            return Err(AppError::Capture(format!(
                "display capture size {width}x{height} does not match physical geometry {expected_width}x{expected_height}"
            )));
        }
        snapshots.push((x, y, width, height, rgba));
    }

    let monitor_bounds = snapshots
        .iter()
        .map(|(x, y, width, height, _)| (*x, *y, *width, *height))
        .collect::<Vec<_>>();
    let (desktop_x, desktop_y, desktop_width, desktop_height) =
        virtual_desktop_bounds(&monitor_bounds)?;
    let pixel_count = (desktop_width as usize)
        .checked_mul(desktop_height as usize)
        .ok_or_else(|| AppError::Capture("virtual desktop is too large".into()))?;
    let byte_count = pixel_count
        .checked_mul(4)
        .ok_or_else(|| AppError::Capture("virtual desktop is too large".into()))?;
    let mut rgba = vec![0; byte_count];
    let monitor_count = snapshots.len();

    for (x, y, width, height, pixels) in snapshots {
        let offset_x = (x - desktop_x) as usize;
        let offset_y = (y - desktop_y) as usize;
        let row_bytes = (width as usize)
            .checked_mul(4)
            .ok_or_else(|| AppError::Capture("display row is too large".into()))?;
        for row in 0..height as usize {
            let src_start = row * row_bytes;
            let dst_start = ((offset_y + row) * desktop_width as usize + offset_x) * 4;
            rgba[dst_start..dst_start + row_bytes]
                .copy_from_slice(&pixels[src_start..src_start + row_bytes]);
        }
    }

    Ok(VirtualDesktopCapture {
        rgba,
        x: desktop_x,
        y: desktop_y,
        width: desktop_width,
        height: desktop_height,
        monitor_count,
    })
}

#[cfg(not(target_os = "windows"))]
fn physical_display_geometry(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> AppResult<(i32, i32, u32, u32)> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(AppError::Capture("display scale factor is invalid".into()));
    }

    // Match `screenshots::capture_screen`, which applies the factor with an
    // f32 multiplication before converting to an integer pixel count.
    let scale_coordinate = |value: i32| -> AppResult<i32> {
        let scaled = value as f32 * scale_factor;
        if !scaled.is_finite() || scaled < i32::MIN as f32 || scaled > i32::MAX as f32 {
            return Err(AppError::Capture("physical display coordinate is invalid".into()));
        }
        Ok(scaled as i32)
    };
    let scale_dimension = |value: u32| -> AppResult<u32> {
        let scaled = value as f32 * scale_factor;
        if !scaled.is_finite() || scaled < 0.0 || scaled > u32::MAX as f32 {
            return Err(AppError::Capture("physical display dimension is invalid".into()));
        }
        Ok(scaled as u32)
    };

    Ok((
        scale_coordinate(x)?,
        scale_coordinate(y)?,
        scale_dimension(width)?,
        scale_dimension(height)?,
    ))
}

fn virtual_desktop_bounds(monitors: &[(i32, i32, u32, u32)]) -> AppResult<(i32, i32, u32, u32)> {
    let min_x = monitors.iter().map(|(x, _, _, _)| *x as i64).min()
        .ok_or_else(|| AppError::Capture("no monitors found".into()))?;
    let min_y = monitors.iter().map(|(_, y, _, _)| *y as i64).min()
        .ok_or_else(|| AppError::Capture("no monitors found".into()))?;
    let max_x = monitors.iter().map(|(x, _, width, _)| *x as i64 + *width as i64).max()
        .ok_or_else(|| AppError::Capture("no monitors found".into()))?;
    let max_y = monitors.iter().map(|(_, y, _, height)| *y as i64 + *height as i64).max()
        .ok_or_else(|| AppError::Capture("no monitors found".into()))?;

    let width = u32::try_from(max_x - min_x)
        .map_err(|_| AppError::Capture("virtual desktop width is invalid".into()))?;
    let height = u32::try_from(max_y - min_y)
        .map_err(|_| AppError::Capture("virtual desktop height is invalid".into()))?;
    Ok((
        i32::try_from(min_x).map_err(|_| AppError::Capture("virtual desktop x is invalid".into()))?,
        i32::try_from(min_y).map_err(|_| AppError::Capture("virtual desktop y is invalid".into()))?,
        width,
        height,
    ))
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use super::physical_display_geometry;
    use super::virtual_desktop_bounds;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn display_geometry_is_converted_to_physical_pixels() {
        assert_eq!(
            physical_display_geometry(-1280, 0, 1280, 1024, 1.5).unwrap(),
            (-1920, 0, 1920, 1536)
        );
    }

    #[test]
    fn bounds_cover_negative_and_vertical_displays() {
        assert_eq!(
            virtual_desktop_bounds(&[(0, 0, 1920, 1080), (-1280, 0, 1280, 1024), (0, -900, 1600, 900)]).unwrap(),
            (-1280, -900, 3200, 1980)
        );
    }
}

#[cfg(target_os = "macos")]
fn capture_screen_with_preview_macos(display_id: u32) -> AppResult<CapturedScreenImage> {
    let started = std::time::Instant::now();
    let capture_path = temp_capture_path("jpg");

    let display_id_str = display_id.to_string();
    let status = Command::new("screencapture")
        .args([
            "-x",
            "-D",
            &display_id_str,
            "-t",
            "jpg",
            capture_path.to_string_lossy().as_ref(),
        ])
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

// ── Debug helpers (macOS) ─────────────────────────────────────────────────────

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