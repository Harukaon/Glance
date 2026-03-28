use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::sync::mpsc;
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
use crate::models::{HistoryQuery, OverlayPayload, SelectionPayload, TextTranslationResult, TranslationHistoryItem, TranslatorSettings};

const OVERLAY_WINDOW_LABEL: &str = "overlay";

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
        // Unregister old hotkey before registering the new one.
        unregister_hotkey(&app, &old.hotkey);
        apply_hotkey(&app, &settings.hotkey);
    }

    Ok(settings)
}

/// Enable or disable launch-at-login using tauri-plugin-autostart.
pub fn apply_autostart(app: &AppHandle, enable: bool) {
    let manager = app.autolaunch();
    if enable {
        if let Err(e) = manager.enable() {
            tracing::warn!("autostart enable failed: {e}");
        }
    } else {
        if let Err(e) = manager.disable() {
            tracing::warn!("autostart disable failed: {e}");
        }
    }
}

/// Register a global hotkey that triggers begin_capture.
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

/// Capture the primary screen and display a native fullscreen window for selection.
/// The native window eliminates the ~652ms WebView startup time.
#[tauri::command]
pub async fn begin_capture(app: AppHandle, state: State<'_, SharedState>) -> AppResult<()> {
    if *state.capture_in_progress.read().await {
        return Err(AppError::Capture(
            "a capture session is already running".into(),
        ));
    }
    *state.capture_in_progress.write().await = true;

    // Close any lingering overlay window.
    if let Some(w) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = w.close();
    }

    let t0 = std::time::Instant::now();

    // Step 1: Find primary monitor.
    let monitor = tokio::task::spawn_blocking(capture::find_primary_screen)
        .await
        .map_err(|e| AppError::Capture(format!("find monitor task failed: {e}")))?
        ?;

    tracing::info!("[PERF] find_primary_screen: {:?}", t0.elapsed());

    // Step 2: Capture screen to raw RGBA bytes.
    let (rgba, w, h) = tokio::task::spawn_blocking(move || {
        capture::capture_screen_to_memory(monitor.screen)
    })
    .await
    .map_err(|e| AppError::Capture(format!("capture task failed: {e}")))?
    ?;

    tracing::info!(
        "[PERF] capture_to_memory: {:?} | {}x{} ({:.1} MB RGBA)",
        t0.elapsed(),
        w, h,
        rgba.len() as f64 / 1_048_576.0
    );

    // Step 3: Start native capture window (sends event to singleton event loop).
    let (event_tx, event_rx) = mpsc::channel::<CaptureEvent>();
    capture_window::start_capture(rgba.clone(), w, h, event_tx);

    tracing::info!("[PERF] start_capture: {:?}", t0.elapsed());

    // Step 4: Spawn async task to handle selection events and translation.
    let state_clone = state.inner().clone();
    let app_clone = app.clone();
    tokio::spawn(async move {
        handle_capture_events(event_rx, rgba, w, h, state_clone, app_clone).await;
    });

    emit_workflow_state(&app, "请框选需要翻译的区域", "", false)?;
    Ok(())
}

async fn handle_capture_events(
    event_rx: mpsc::Receiver<CaptureEvent>,
    rgba: Vec<u8>,
    img_w: u32,
    _img_h: u32,
    state: SharedState,
    app: AppHandle,
) {
    // Wrap the Receiver in a Mutex so we can share it with spawn_blocking closures.
    let rx = std::sync::Arc::new(std::sync::Mutex::new(event_rx));

    loop {
        // Block on channel (use spawn_blocking since mpsc::Receiver is not async).
        let rx_clone = rx.clone();
        let event = tokio::task::spawn_blocking(move || {
            rx_clone.lock().unwrap().recv()
        })
        .await;

        let event = match event {
            Ok(Ok(e)) => e,
            _ => break,
        };

        match event {
            CaptureEvent::Selection { x, y, w, h } => {
                emit_workflow_state(&app, "正在翻译…", "", true).ok();
                let _ = capture_window::capture_proxy().send_event(CaptureCommand::ShowLoading);

                let crop = capture_window::crop_rgba(&rgba, img_w, x, y, w, h);

                let png_result = tokio::task::spawn_blocking(move || {
                    capture_window::encode_png(&crop, w, h)
                })
                .await;

                let png_bytes = match png_result {
                    Ok(Ok(b)) => b,
                    Ok(Err(e)) => {
                        tracing::error!("PNG encode failed: {e}");
                        emit_workflow_state(&app, "截图编码失败", "error", false).ok();
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("spawn_blocking PNG failed: {e}");
                        continue;
                    }
                };

                let settings = state.settings.read().await.clone();
                let from_lang = settings.from_lang.clone();
                let to_lang = settings.to_lang.clone();

                let selection = SelectionPayload {
                    x: 0.0,
                    y: 0.0,
                    width: w as f64,
                    height: h as f64,
                    monitor_id: format!("capture:{x}:{y}:{w}:{h}"),
                    monitor_x: x as i32,
                    monitor_y: y as i32,
                    monitor_width: w,
                    monitor_height: h,
                    monitor_scale_factor: 1.0,
                };

                let translate_result = state
                    .api_client
                    .translate_image_bytes(
                        png_bytes,
                        "capture.png".into(),
                        "image/png".into(),
                        from_lang,
                        to_lang,
                        selection,
                        None,
                        &settings,
                    )
                    .await;

                match translate_result {
                    Ok(response) => {
                        tracing::info!(
                            "API response: request_id={}, rendered_image len={}, regions={}",
                            response.request_id,
                            response.rendered_image_base64.len(),
                            response.regions.len()
                        );

                        // Save to history.
                        if let Ok(mut history) = state.config_store.load_history().await {
                            history.push(response.history_item.clone());
                            let _ = state.config_store.save_history(&history).await;
                        }

                        // Decode result image and send to window.
                        if !response.rendered_image_base64.is_empty() {
                            match BASE64_STANDARD.decode(&response.rendered_image_base64) {
                                Ok(jpeg_bytes) => {
                                    // Decode JPEG → RGBA via `image` crate.
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

            CaptureEvent::Cancelled => {
                break;
            }
        }
    }

    // Clean up state after window closes.
    *state.capture_in_progress.write().await = false;
    emit_workflow_state(&app, "", "", false).ok();
}

/// Close the capture window and end the session.
#[tauri::command]
pub async fn cancel_capture(app: AppHandle, state: State<'_, SharedState>) -> AppResult<()> {
    *state.capture_in_progress.write().await = false;
    let _ = capture_window::capture_proxy().send_event(CaptureCommand::Close);
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
