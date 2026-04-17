use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

use crate::capture;
use crate::capture_window;
use crate::error::{AppError, AppResult};

const SELF_TEST_DIR: &str = "/tmp/glance-self-test/latest";
const SMOKE_TEST_DIR: &str = "/tmp/glance-smoke-test/latest";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSelfTestResult {
    pub ok: bool,
    pub output_dir: String,
    pub capture_png_path: String,
    pub width: u32,
    pub height: u32,
    pub rgba_bytes: usize,
    pub expected_rgba_bytes: usize,
    pub scale_factor: f64,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub find_monitor_ms: u128,
    pub capture_ms: u128,
    pub encode_ms: u128,
    pub total_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSmokeTestResult {
    pub ok: bool,
    pub cancelled: bool,
    pub mode: String,
    pub output_dir: String,
    pub capture_png_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub rgba_bytes: usize,
    pub capture_ms: u128,
    pub total_ms: u128,
}

pub fn should_run_capture_self_test() -> bool {
    std::env::args().any(|arg| arg == "--capture-self-test")
}

pub fn should_run_capture_smoke_test() -> bool {
    std::env::args().any(|arg| arg == "--capture-smoke-test")
}

pub fn run_capture_self_test() -> AppResult<CaptureSelfTestResult> {
    let total_started = Instant::now();
    let output_dir = reset_output_dir(PathBuf::from(SELF_TEST_DIR))?;

    let find_started = Instant::now();

    #[cfg(target_os = "macos")]
    let (monitor, screen_for_capture) = {
        let monitor = capture::find_primary_screen()?;
        (monitor, monitor.display_id)
    };
    #[cfg(not(target_os = "macos"))]
    let (monitor, screen_for_capture) = {
        let result = capture::find_primary_screen()?;
        (result.monitor, result.screen)
    };

    let find_monitor_ms = find_started.elapsed().as_millis();

    let scale_factor = monitor.scale_factor;
    let monitor_x = monitor.x;
    let monitor_y = monitor.y;
    let monitor_width = monitor.width;
    let monitor_height = monitor.height;

    let capture_started = Instant::now();
    #[cfg(target_os = "macos")]
    let (rgba, width, height) = capture::capture_screen_to_memory(screen_for_capture)?;
    #[cfg(not(target_os = "macos"))]
    let (rgba, width, height) = capture::capture_screen_to_memory(screen_for_capture)?;
    let capture_ms = capture_started.elapsed().as_millis();

    let expected_rgba_bytes = (width as usize) * (height as usize) * 4;
    if width == 0 || height == 0 {
        return Err(AppError::Capture(
            "capture self test returned an empty image".into(),
        ));
    }
    if rgba.len() != expected_rgba_bytes {
        return Err(AppError::Capture(format!(
            "capture self test returned {} RGBA bytes, expected {}",
            rgba.len(),
            expected_rgba_bytes
        )));
    }

    let encode_started = Instant::now();
    let png_bytes = capture_window::encode_png(&rgba, width, height)?;
    let encode_ms = encode_started.elapsed().as_millis();

    let capture_png_path = output_dir.join("capture.png");
    fs::write(&capture_png_path, &png_bytes).map_err(AppError::Io)?;

    let result = CaptureSelfTestResult {
        ok: true,
        output_dir: output_dir.display().to_string(),
        capture_png_path: capture_png_path.display().to_string(),
        width,
        height,
        rgba_bytes: rgba.len(),
        expected_rgba_bytes,
        scale_factor,
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
        find_monitor_ms,
        capture_ms,
        encode_ms,
        total_ms: total_started.elapsed().as_millis(),
    };

    let result_json = serde_json::to_vec_pretty(&result)?;
    fs::write(output_dir.join("result.json"), result_json).map_err(AppError::Io)?;

    Ok(result)
}

pub fn run_capture_smoke_test() -> AppResult<CaptureSmokeTestResult> {
    #[cfg(not(target_os = "macos"))]
    {
        return Err(AppError::Capture(
            "--capture-smoke-test is only supported on macOS".into(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let total_started = Instant::now();
        let output_dir = reset_output_dir(PathBuf::from(SMOKE_TEST_DIR))?;

        eprintln!("Glance macOS native capture smoke test is starting.");
        eprintln!("Select a region in the system screenshot UI. Press Esc to cancel.");

        let capture_started = Instant::now();
        let interactive = capture::capture_interactive_region()?;

        let capture_ms = capture_started.elapsed().as_millis();

        let result = match interactive {
            Some(image) => {
                let capture_png_path = output_dir.join("selection.png");
                fs::write(&capture_png_path, &image.png_bytes).map_err(AppError::Io)?;

                CaptureSmokeTestResult {
                    ok: true,
                    cancelled: false,
                    mode: "macos-native-screencapture".into(),
                    output_dir: output_dir.display().to_string(),
                    capture_png_path: Some(capture_png_path.display().to_string()),
                    width: image.width,
                    height: image.height,
                    rgba_bytes: image.rgba_bytes.len(),
                    capture_ms,
                    total_ms: total_started.elapsed().as_millis(),
                }
            }
            None => CaptureSmokeTestResult {
                ok: false,
                cancelled: true,
                mode: "macos-native-screencapture".into(),
                output_dir: output_dir.display().to_string(),
                capture_png_path: None,
                width: 0,
                height: 0,
                rgba_bytes: 0,
                capture_ms,
                total_ms: total_started.elapsed().as_millis(),
            },
        };

        let result_json = serde_json::to_vec_pretty(&result)?;
        fs::write(output_dir.join("result.json"), result_json).map_err(AppError::Io)?;

        Ok(result)
    }
}

fn reset_output_dir(output_dir: PathBuf) -> AppResult<PathBuf> {
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir).map_err(AppError::Io)?;
    }
    fs::create_dir_all(&output_dir).map_err(AppError::Io)?;
    Ok(output_dir)
}