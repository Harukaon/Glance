use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, State, WebviewUrl,
    WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::app_state::SharedState;
use crate::capture;
use crate::capture_window::{self, CaptureCommand, CaptureEvent};
use crate::error::{AppError, AppResult};
use crate::models::{
    CaptureRect, CaptureTranslatePayload, CaptureViewPayload, HistoryQuery, OverlayPayload,
    SelectionPayload, TextTranslationResult, TranslationHistoryItem, TranslatorSettings,
};

const OVERLAY_WINDOW_LABEL: &str = "overlay";
#[cfg(target_os = "macos")]
const CAPTURE_WINDOW_LABEL: &str = "capture";

// ── Settings ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn load_settings(state: State<'_, SharedState>) -> AppResult<TranslatorSettings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: TranslatorSettings,
) -> AppResult<TranslatorSettings> {
    let old = state.settings.read().await.clone();
    state.config_store.save_settings(&settings).await?;
    *state.settings.write().await = settings.clone();

    if settings.autostart != old.autostart {
        apply_autostart(&app, settings.autostart);
    }
    if settings.hotkey != old.hotkey {
        unregister_hotkey(&app, &old.hotkey);
        apply_hotkey(&app, &settings.hotkey);
    }

    Ok(settings)
}

pub fn apply_autostart(app: &AppHandle, enable: bool) {
    let manager = app.autolaunch();
    if enable {
        if let Err(e) = manager.enable() {
            tracing::warn!("autostart enable failed: {e}");
        }
    } else if let Err(e) = manager.disable() {
        tracing::warn!("autostart disable failed: {e}");
    }
}

pub fn apply_hotkey(app: &AppHandle, hotkey: &str) {
    if hotkey.is_empty() {
        return;
    }
    let app_clone = app.clone();
    if let Err(e) = app.global_shortcut().on_shortcut(hotkey, move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let app2 = app_clone.clone();
            tauri::async_runtime::spawn(async move {
                let state: State<'_, SharedState> = app2.state();
                let _ = crate::commands::begin_capture(app2.clone(), state).await;
            });
        }
    }) {
        tracing::warn!("global shortcut register failed for '{hotkey}': {e}");
    }
}

fn unregister_hotkey(app: &AppHandle, hotkey: &str) {
    if hotkey.is_empty() {
        return;
    }
    if let Err(e) = app.global_shortcut().unregister(hotkey) {
        tracing::warn!("global shortcut unregister failed for '{hotkey}': {e}");
    }
}

// ── History ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_history(
    state: State<'_, SharedState>,
    query: Option<HistoryQuery>,
) -> AppResult<Vec<TranslationHistoryItem>> {
    let mut items = state.config_store.load_history().await?;
    items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if let Some(query) = query {
        if let Some(limit) = query.limit {
            items.truncate(limit);
        }
    }
    Ok(items)
}

#[tauri::command]
pub async fn clear_history(state: State<'_, SharedState>) -> AppResult<()> {
    state.config_store.save_history(&[]).await
}

// ── Text translation ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn translate_text(
    state: State<'_, SharedState>,
    text: String,
    from_lang: String,
    to_lang: String,
) -> AppResult<TextTranslationResult> {
    state.google_client.translate(&text, &from_lang, &to_lang).await
}

// ── Window ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn hide_window(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide()?;
    }
    Ok(())
}

// ── Capture flow ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn begin_capture(app: AppHandle, state: State<'_, SharedState>) -> AppResult<()> {
    if *state.capture_in_progress.read().await {
        return Err(AppError::Capture(
            "a capture session is already running".into(),
        ));
    }
    *state.capture_in_progress.write().await = true;

    let result = begin_capture_impl(&app, state.inner()).await;
    if result.is_err() {
        reset_capture_state(state.inner()).await;
        emit_workflow_state(&app, "", "", false).ok();
    }
    result
}

async fn begin_capture_impl(app: &AppHandle, state: &SharedState) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = w.close();
    }

    #[cfg(target_os = "macos")]
    {
        let dir = capture::debug_reset_dir()?;
        capture::debug_log(format!("[begin] debug dir={}", dir.display()));
    }

    let t0 = std::time::Instant::now();

    #[cfg(target_os = "macos")]
    let restore_main_window = if let Some(main_window) = app.get_webview_window("main") {
        let was_visible = main_window.is_visible().unwrap_or(false);
        capture::debug_log(format!("[begin] main window visible before capture={was_visible}"));
        if was_visible {
            let _ = main_window.hide();
            tokio::time::sleep(Duration::from_millis(120)).await;
            capture::debug_log("[begin] hid main window and waited 120ms");
        }
        was_visible
    } else {
        capture::debug_log("[begin] main window missing");
        false
    };

    let monitor = tokio::task::spawn_blocking(capture::find_primary_screen)
        .await
        .map_err(|e| AppError::Capture(format!("find monitor task failed: {e}")))??;

    tracing::info!(
        "[PERF] find_primary_screen: {:?} (scale={})",
        t0.elapsed(),
        monitor.scale_factor
    );
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[begin] primary monitor x={} y={} width={} height={} scale_factor={}",
        monitor.x, monitor.y, monitor.width, monitor.height, monitor.scale_factor
    ));

    let scale_factor = monitor.scale_factor;
    #[cfg(target_os = "macos")]
    let monitor_x = monitor.x;
    #[cfg(target_os = "macos")]
    let monitor_y = monitor.y;
    #[cfg(target_os = "macos")]
    let monitor_width = monitor.width;
    #[cfg(target_os = "macos")]
    let monitor_height = monitor.height;
    let screen = monitor.screen;

    let (rgba, w, h) = tokio::task::spawn_blocking(move || capture::capture_screen_to_memory(screen))
        .await
        .map_err(|e| AppError::Capture(format!("capture task failed: {e}")))??;

    tracing::info!(
        "[PERF] capture_to_memory: {:?} | {}x{} ({:.1} MB RGBA)",
        t0.elapsed(),
        w,
        h,
        rgba.len() as f64 / 1_048_576.0
    );
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[begin] capture_to_memory -> {}x{} rgba_bytes={}",
        w,
        h,
        rgba.len()
    ));

    #[cfg(target_os = "macos")]
    {
        let preview_png_base64 = build_capture_preview_base64(rgba.clone(), w, h).await?;
        *state.capture_session.write().await = Some(crate::app_state::ActiveCaptureSession {
            rgba,
            img_w: w,
            img_h: h,
            scale_factor,
            preview_png_base64,
            restore_main_window,
        });

        create_capture_window(app, monitor_x, monitor_y, monitor_width.max(w), monitor_height.max(h))?;
        capture::debug_log(format!(
            "[begin] capture window shown at x={} y={} width={} height={}",
            monitor_x,
            monitor_y,
            monitor_width.max(w),
            monitor_height.max(h)
        ));
        tracing::info!("[PERF] start_capture_webview: {:?}", t0.elapsed());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let (event_tx, event_rx) = mpsc::channel::<CaptureEvent>();
        capture_window::start_capture(rgba.clone(), w, h, scale_factor, event_tx);
        tracing::info!("[PERF] start_capture_native: {:?}", t0.elapsed());

        let state_clone = state.clone();
        let app_clone = app.clone();
        tokio::spawn(async move {
            handle_capture_events(event_rx, rgba, w, scale_factor, state_clone, app_clone).await;
        });
    }

    emit_workflow_state(app, "请框选需要翻译的区域", "", false)?;
    Ok(())
}

#[tauri::command]
pub async fn load_capture_payload(state: State<'_, SharedState>) -> AppResult<CaptureViewPayload> {
    let guard = state.capture_session.read().await;
    let session = guard
        .as_ref()
        .ok_or_else(|| AppError::Capture("capture payload missing".into()))?;

    Ok(CaptureViewPayload {
        image_base64: session.preview_png_base64.clone(),
        image_width: session.img_w,
        image_height: session.img_h,
    })
}

#[tauri::command]
pub async fn submit_capture_selection(
    app: AppHandle,
    state: State<'_, SharedState>,
    selection: CaptureRect,
) -> AppResult<CaptureTranslatePayload> {
    if selection.width <= 4 || selection.height <= 4 {
        return Err(AppError::Capture("selection too small".into()));
    }

    emit_workflow_state(&app, "正在翻译…", "", true).ok();

    let result = async {
        let (crop, scale_factor) = {
            let guard = state.capture_session.read().await;
            let session = guard
                .as_ref()
                .ok_or_else(|| AppError::Capture("capture session missing".into()))?;

            #[cfg(target_os = "macos")]
            capture::debug_log(format!(
                "[select] x={} y={} width={} height={} source_image={}x{} scale_factor={}",
                selection.x,
                selection.y,
                selection.width,
                selection.height,
                session.img_w,
                session.img_h,
                session.scale_factor
            ));

            (
                capture_window::crop_rgba(
                    &session.rgba,
                    session.img_w,
                    selection.x,
                    selection.y,
                    selection.width,
                    selection.height,
                ),
                session.scale_factor,
            )
        };

        let png_bytes = encode_cropped_png(crop, selection.width, selection.height).await?;
        #[cfg(target_os = "macos")]
        capture::debug_write_bytes("03_crop.png", &png_bytes);
        let image_base64 =
            translate_capture_png(state.inner(), png_bytes, &selection, scale_factor).await?;

        Ok::<CaptureTranslatePayload, AppError>(CaptureTranslatePayload {
            image_base64,
            selection,
        })
    }
    .await;

    match result {
        Ok(payload) => {
            emit_workflow_state(&app, "翻译完成", "ok", false).ok();
            Ok(payload)
        }
        Err(err) => {
            #[cfg(target_os = "macos")]
            capture::debug_log(format!("[select] error={err}"));
            emit_workflow_state(&app, "翻译失败", "error", false).ok();
            Err(err)
        }
    }
}

async fn handle_capture_events(
    event_rx: mpsc::Receiver<CaptureEvent>,
    rgba: Vec<u8>,
    img_w: u32,
    scale_factor: f64,
    state: SharedState,
    app: AppHandle,
) {
    let rx = std::sync::Arc::new(std::sync::Mutex::new(event_rx));

    loop {
        let rx_clone = rx.clone();
        let event = tokio::task::spawn_blocking(move || rx_clone.lock().unwrap().recv()).await;

        let event = match event {
            Ok(Ok(e)) => e,
            _ => break,
        };

        match event {
            CaptureEvent::Selection { x, y, w, h } => {
                emit_workflow_state(&app, "正在翻译…", "", true).ok();
                let _ = capture_window::capture_proxy().send_event(CaptureCommand::ShowLoading);

                let crop = capture_window::crop_rgba(&rgba, img_w, x, y, w, h);
                let rect = CaptureRect {
                    x,
                    y,
                    width: w,
                    height: h,
                };

                let png_bytes = match encode_cropped_png(crop, w, h).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::error!("PNG encode failed: {e}");
                        emit_workflow_state(&app, "截图编码失败", "error", false).ok();
                        continue;
                    }
                };

                match translate_capture_png(&state, png_bytes, &rect, scale_factor).await {
                    Ok(image_base64) => {
                        if !image_base64.is_empty() {
                            match BASE64_STANDARD.decode(&image_base64) {
                                Ok(jpeg_bytes) => {
                                    let rgba_result = tokio::task::spawn_blocking(move || {
                                        decode_jpeg_to_rgba(&jpeg_bytes)
                                    })
                                    .await;

                                    match rgba_result {
                                        Ok(Ok((result_rgba, rw, rh))) => {
                                            let _ = capture_window::capture_proxy()
                                                .send_event(CaptureCommand::ShowResult {
                                                    rgba_bytes: result_rgba,
                                                    x,
                                                    y,
                                                    w: rw,
                                                    h: rh,
                                                });
                                        }
                                        Ok(Err(e)) => tracing::warn!("JPEG decode: {e}"),
                                        Err(e) => tracing::warn!("spawn_blocking JPEG: {e}"),
                                    }
                                }
                                Err(e) => tracing::warn!("base64 decode: {e}"),
                            }
                        }

                        emit_workflow_state(&app, "翻译完成", "ok", false).ok();
                    }
                    Err(e) => {
                        tracing::error!("Translation error: {e}");
                        emit_workflow_state(&app, "翻译失败", "error", false).ok();
                    }
                }
            }
            CaptureEvent::Cancelled => break,
        }
    }

    reset_capture_state(&state).await;
    emit_workflow_state(&app, "", "", false).ok();
}

#[tauri::command]
pub async fn cancel_capture(app: AppHandle, state: State<'_, SharedState>) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    restore_main_window_if_needed(&app, state.inner()).await;
    #[cfg(target_os = "macos")]
    capture::debug_log("[cancel] capture cancelled");

    reset_capture_state(state.inner()).await;

    #[cfg(target_os = "macos")]
    if let Some(w) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        let _ = w.close();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = capture_window::capture_proxy().send_event(CaptureCommand::Close);
    }

    emit_workflow_state(&app, "", "", false)?;
    Ok(())
}

// ── Overlay (kept for standalone use) ───────────────────────────────────────

#[tauri::command]
pub async fn show_overlay(
    app: AppHandle,
    state: State<'_, SharedState>,
    payload: OverlayPayload,
) -> AppResult<()> {
    *state.overlay_payload.write().await = Some(payload.clone());
    create_overlay_window(
        &app,
        payload.selection.monitor_x,
        payload.selection.monitor_y,
        payload.selection.monitor_width,
        payload.selection.monitor_height,
    )?;
    Ok(())
}

#[tauri::command]
pub async fn load_overlay_payload(state: State<'_, SharedState>) -> AppResult<OverlayPayload> {
    state
        .overlay_payload
        .read()
        .await
        .clone()
        .ok_or_else(|| tauri::Error::AssetNotFound("overlay payload missing".into()).into())
}

#[tauri::command]
pub async fn close_overlay(app: AppHandle, state: State<'_, SharedState>) -> AppResult<()> {
    *state.overlay_payload.write().await = None;
    if let Some(w) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        w.close()?;
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn reset_capture_state(state: &SharedState) {
    #[cfg(target_os = "macos")]
    capture::debug_log("[state] reset capture state");
    *state.capture_in_progress.write().await = false;
    *state.capture_session.write().await = None;
}

#[cfg(target_os = "macos")]
async fn restore_main_window_if_needed(app: &AppHandle, state: &SharedState) {
    let should_restore = state
        .capture_session
        .read()
        .await
        .as_ref()
        .map(|session| session.restore_main_window)
        .unwrap_or(false);

    if should_restore {
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
            let _ = main_window.set_focus();
            capture::debug_log("[state] restored main window");
        }
    }
}

#[cfg(target_os = "macos")]
async fn build_capture_preview_base64(rgba: Vec<u8>, w: u32, h: u32) -> AppResult<String> {
    let png_bytes = tokio::task::spawn_blocking(move || capture_window::encode_png(&rgba, w, h))
        .await
        .map_err(|e| AppError::Capture(format!("preview encode task failed: {e}")))??;
    capture::debug_write_bytes("02_preview.png", &png_bytes);
    capture::debug_log(format!(
        "[preview] encoded preview png width={} height={} bytes={}",
        w,
        h,
        png_bytes.len()
    ));
    Ok(BASE64_STANDARD.encode(png_bytes))
}

async fn encode_cropped_png(crop: Vec<u8>, w: u32, h: u32) -> AppResult<Vec<u8>> {
    tokio::task::spawn_blocking(move || capture_window::encode_png(&crop, w, h))
        .await
        .map_err(|e| AppError::Capture(format!("PNG encode task failed: {e}")))?
}

async fn translate_capture_png(
    state: &SharedState,
    png_bytes: Vec<u8>,
    rect: &CaptureRect,
    scale_factor: f64,
) -> AppResult<String> {
    let settings = state.settings.read().await.clone();
    let response = state
        .api_client
        .translate_image_bytes(
            png_bytes,
            "capture.png".into(),
            "image/png".into(),
            settings.from_lang.clone(),
            settings.to_lang.clone(),
            SelectionPayload {
                x: 0.0,
                y: 0.0,
                width: rect.width as f64,
                height: rect.height as f64,
                monitor_id: format!(
                    "capture:{}:{}:{}:{}",
                    rect.x, rect.y, rect.width, rect.height
                ),
                monitor_x: rect.x as i32,
                monitor_y: rect.y as i32,
                monitor_width: rect.width,
                monitor_height: rect.height,
                monitor_scale_factor: scale_factor,
            },
            None,
            &settings,
        )
        .await?;

    tracing::info!(
        "API response: request_id={}, rendered_image len={}, regions={}",
        response.request_id,
        response.rendered_image_base64.len(),
        response.regions.len()
    );
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[translate] request_id={} rendered_b64_len={} regions={}",
        response.request_id,
        response.rendered_image_base64.len(),
        response.regions.len()
    ));

    if let Ok(mut history) = state.config_store.load_history().await {
        history.push(response.history_item.clone());
        let _ = state.config_store.save_history(&history).await;
    }

    #[cfg(target_os = "macos")]
    if !response.rendered_image_base64.is_empty() {
        if let Ok(bytes) = BASE64_STANDARD.decode(&response.rendered_image_base64) {
            capture::debug_write_bytes("04_translated.jpg", &bytes);
        }
    }

    Ok(response.rendered_image_base64)
}

fn emit_workflow_state(app: &AppHandle, message: &str, kind: &str, busy: bool) -> AppResult<()> {
    app.emit_to(
        "main",
        "workflow:state",
        serde_json::json!({
            "message": message,
            "type": kind,
            "busy": busy,
        }),
    )?;
    Ok(())
}

fn decode_jpeg_to_rgba(jpeg_bytes: &[u8]) -> AppResult<(Vec<u8>, u32, u32)> {
    use image::ImageReader;
    let img = ImageReader::new(std::io::Cursor::new(jpeg_bytes))
        .with_guessed_format()
        .map_err(|e| AppError::Capture(format!("image reader: {e}")))?
        .decode()
        .map_err(|e| AppError::Capture(format!("image decode: {e}")))?
        .into_rgba8();
    let w = img.width();
    let h = img.height();
    Ok((img.into_raw(), w, h))
}

#[cfg(target_os = "macos")]
fn create_capture_window(app: &AppHandle, x: i32, y: i32, width: u32, height: u32) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        let _ = w.close();
    }

    let url = WebviewUrl::App("capture.html".into());
    let window = WebviewWindowBuilder::new(app, CAPTURE_WINDOW_LABEL, url)
        .title("Capture")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .visible(false)
        .position(0.0, 0.0)
        .inner_size(width as f64, height as f64)
        .build()?;
    window.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
    window.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
    #[cfg(target_os = "macos")]
    let _ = window.set_visible_on_all_workspaces(true);
    window.show()?;
    window.set_focus()?;
    Ok(())
}

fn create_overlay_window(app: &AppHandle, x: i32, y: i32, width: u32, height: u32) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        w.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
        w.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }

    let url = WebviewUrl::App("overlay.html".into());
    let window = WebviewWindowBuilder::new(app, OVERLAY_WINDOW_LABEL, url)
        .title("Translation Overlay")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .visible(false)
        .position(0.0, 0.0)
        .inner_size(100.0, 100.0)
        .build()?;
    window.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
    window.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}
