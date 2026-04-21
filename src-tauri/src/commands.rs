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
use crate::popup_shortcut::{decide_popup_shortcut_action, PopupShortcutAction};

const OVERLAY_WINDOW_LABEL: &str = "overlay";
const CAPTURE_WINDOW_LABEL: &str = "capture";
const FOCUS_TEXT_INPUT_EVENT: &str = "main:focus-text-input";
const WIN_WIDTH: f64 = 520.0;

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
    if settings.popup_shortcut != old.popup_shortcut {
        if let Some(ref old_shortcut) = old.popup_shortcut {
            unregister_hotkey(&app, old_shortcut);
        }
        if let Some(ref new_shortcut) = settings.popup_shortcut {
            apply_popup_shortcut(&app, new_shortcut);
        }
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
    tracing::info!("registering global shortcut: '{hotkey}'");
    let app_clone = app.clone();
    if let Err(e) = app
        .global_shortcut()
        .on_shortcut(hotkey, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app2 = app_clone.clone();
                tauri::async_runtime::spawn(async move {
                    let state: State<'_, SharedState> = app2.state();
                    let _ = crate::commands::begin_capture(app2.clone(), state).await;
                });
            }
        })
    {
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

pub fn apply_popup_shortcut(app: &AppHandle, shortcut: &str) {
    if shortcut.is_empty() {
        return;
    }
    tracing::info!("registering popup shortcut: '{shortcut}'");
    let app_clone = app.clone();
    if let Err(e) = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app2 = app_clone.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = toggle_main_window_from_popup_shortcut(&app2);
                });
            }
        })
    {
        tracing::warn!("popup shortcut register failed for '{shortcut}': {e}");
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
    let (engine, llm_config) = {
        let settings = state.settings.read().await;
        (settings.text_translate_engine, settings.llm_config.clone())
    };
    state
        .text_translator
        .translate(&text, &from_lang, &to_lang, engine, &llm_config)
        .await
}

// ── Window ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn resize_main_window(app: AppHandle, height: f64) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        use tauri::LogicalSize;
        let _ = w.set_size(LogicalSize::new(WIN_WIDTH, height));
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_window(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        instant_hide(&w);
    }
    #[cfg(target_os = "macos")]
    app.set_dock_visibility(false)?;
    Ok(())
}

fn instant_hide(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowAnimationBehavior};
        if let Ok(raw) = window.ns_window() {
            unsafe {
                let ns_window: &NSWindow = &*raw.cast();
                // 2 is NSWindowAnimationBehaviorNone
                ns_window.setAnimationBehavior(NSWindowAnimationBehavior(2));
            }
        }
    }
    let _ = window.hide();
}

fn toggle_main_window_from_popup_shortcut(app: &AppHandle) -> AppResult<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let is_visible = window.is_visible().unwrap_or(false);
    match decide_popup_shortcut_action(is_visible) {
        PopupShortcutAction::HideWindow => {
            instant_hide(&window);
            #[cfg(target_os = "macos")]
            app.set_dock_visibility(false)?;
        }
        PopupShortcutAction::ShowWindowAndFocusInput => {
            #[cfg(target_os = "macos")]
            app.set_dock_visibility(true)?;
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            // 显示窗口由后端负责，真正聚焦哪个输入控件交给前端决定，避免后端耦合 DOM 细节。
            app.emit_to("main", FOCUS_TEXT_INPUT_EVENT, serde_json::json!({}))?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn capture_debug_log(message: String) -> AppResult<()> {
    let _ = &message;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!("[timeline][ui] {message}"));
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
    capture_timeline(&t0, "begin_capture_impl entered");

    #[cfg(target_os = "macos")]
    let restore_main_window = if let Some(main_window) = app.get_webview_window("main") {
        let was_visible = main_window.is_visible().unwrap_or(false);
        capture::debug_log(format!(
            "[begin] main window visible before capture={was_visible}"
        ));
        if was_visible {
            instant_hide(&main_window);
            capture::debug_log("[begin] hid main window instantly");
            capture_timeline(&t0, "main window hidden");
        }
        was_visible
    } else {
        capture::debug_log("[begin] main window missing");
        false
    };

    #[cfg(target_os = "macos")]
    {
        let monitor = tokio::task::spawn_blocking(capture::find_cursor_monitor)
            .await
            .map_err(|e| AppError::Capture(format!("find monitor task failed: {e}")))??;

        tracing::info!(
            "[PERF] find_cursor_monitor: {:?} (scale={})",
            t0.elapsed(),
            monitor.scale_factor
        );
        capture::debug_log(format!(
            "[begin] cursor monitor x={} y={} width={} height={} scale_factor={} display_id={}",
            monitor.x, monitor.y, monitor.width, monitor.height, monitor.scale_factor, monitor.display_id
        ));
        capture_timeline(&t0, "cursor monitor resolved");

        let scale_factor = monitor.scale_factor;
        let display_id = monitor.display_id;

        let captured = tokio::task::spawn_blocking(move || capture::capture_screen_with_preview(display_id))
            .await
            .map_err(|e| AppError::Capture(format!("capture task failed: {e}")))??;
        let rgba = captured.rgba_bytes;
        let w = captured.width;
        let h = captured.height;

        tracing::info!(
            "[PERF] capture_to_memory: {:?} | {}x{} ({:.1} MB RGBA)",
            t0.elapsed(),
            w,
            h,
            rgba.len() as f64 / 1_048_576.0
        );
        capture::debug_log(format!(
            "[begin] capture_to_memory -> {}x{} rgba_bytes={} scale_factor={}",
            w,
            h,
            rgba.len(),
            scale_factor
        ));
        capture_timeline(&t0, format!("screen capture ready {}x{}", w, h));

        let preview_started = std::time::Instant::now();
        let (preview_image_base64, preview_image_mime) =
            build_capture_preview_base64(captured.preview_bytes, captured.preview_mime);
        capture::debug_log(format!(
            "[preview] ready after {:?}",
            preview_started.elapsed()
        ));
        capture_timeline(
            &t0,
            format!(
                "preview payload ready mime={} base64_chars={}",
                preview_image_mime,
                preview_image_base64.len()
            ),
        );

        // Tauri's physical window APIs expect device pixels. The monitor geometry
        // reported above is in macOS points, so convert before sizing the capture
        // window or it appears as a smaller corner window on Retina displays.
        let window_x = ((monitor.x as f64) * scale_factor).round() as i32;
        let window_y = ((monitor.y as f64) * scale_factor).round() as i32;
        let window_width = w;
        let window_height = h;

        capture::debug_log(format!(
            "[begin] opening capture window physical x={} y={} width={} height={} logical={}x{}",
            window_x, window_y, window_width, window_height, monitor.width, monitor.height
        ));

        *state.capture_session.write().await = Some(crate::app_state::ActiveCaptureSession {
            rgba,
            img_w: w,
            img_h: h,
            scale_factor,
            monitor_x: monitor.x,
            monitor_y: monitor.y,
            monitor_width: monitor.width,
            monitor_height: monitor.height,
            display_id,
            preview_image_base64: Some(preview_image_base64),
            preview_image_mime,
            restore_main_window,
        });
        capture_timeline(&t0, "capture session stored");

        create_capture_window(app, window_x, window_y, window_width, window_height)?;
        capture_timeline(&t0, "capture window created");
        emit_workflow_state(app, "请框选需要翻译的区域", "", false)?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let result = tokio::task::spawn_blocking(capture::find_cursor_monitor)
            .await
            .map_err(|e| AppError::Capture(format!("find monitor task failed: {e}")))??;

        let monitor = result.monitor;
        tracing::info!(
            "[PERF] find_cursor_monitor: {:?} (scale={})",
            t0.elapsed(),
            monitor.scale_factor
        );

        let scale_factor = monitor.scale_factor;
        let screen = result.screen;

        let (rgba, w, h) =
            tokio::task::spawn_blocking(move || capture::capture_screen_to_memory(screen))
                .await
                .map_err(|e| AppError::Capture(format!("capture task failed: {e}")))??;

        tracing::info!(
            "[PERF] capture_to_memory: {:?} | {}x{} ({:.1} MB RGBA)",
            t0.elapsed(),
            w,
            h,
            rgba.len() as f64 / 1_048_576.0
        );

        *state.capture_session.write().await = Some(crate::app_state::ActiveCaptureSession {
            rgba: rgba.clone(),
            img_w: w,
            img_h: h,
            scale_factor,
            monitor_x: monitor.x,
            monitor_y: monitor.y,
            monitor_width: monitor.width,
            monitor_height: monitor.height,
            preview_image_base64: None,
            preview_image_mime: String::new(),
        });

        let (event_tx, event_rx) = mpsc::channel::<CaptureEvent>();
        capture_window::start_capture(rgba.clone(), w, h, scale_factor, monitor.x, monitor.y, event_tx);
        tracing::info!("[PERF] start_capture_native: {:?}", t0.elapsed());

        let state_clone = state.clone();
        let app_clone = app.clone();
        tokio::spawn(async move {
            handle_capture_events(event_rx, rgba, w, scale_factor, state_clone, app_clone).await;
        });
    }

    #[cfg(not(target_os = "macos"))]
    emit_workflow_state(app, "请框选需要翻译的区域", "", false)?;
    Ok(())
}

#[tauri::command]
pub async fn load_capture_payload(state: State<'_, SharedState>) -> AppResult<CaptureViewPayload> {
    let guard = state.capture_session.read().await;
    let session = guard
        .as_ref()
        .ok_or_else(|| AppError::Capture("capture payload missing".into()))?;

    let image_base64 = session
        .preview_image_base64
        .clone()
        .ok_or_else(|| AppError::Capture("capture preview not ready".into()))?;

    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend] load_capture_payload ready mime={} size={}x{} base64_chars={}",
        session.preview_image_mime,
        session.img_w,
        session.img_h,
        image_base64.len()
    ));

    Ok(CaptureViewPayload {
        image_base64,
        image_mime: session.preview_image_mime.clone(),
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
                                            let _ = capture_window::capture_proxy().send_event(
                                                CaptureCommand::ShowResult {
                                                    rgba_bytes: result_rgba,
                                                    x,
                                                    y,
                                                    w: rw,
                                                    h: rh,
                                                },
                                            );
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
                        let _ = capture_window::capture_proxy().send_event(CaptureCommand::Close);
                        break;
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

    let _ = close_capture_window(&app);
    #[cfg(not(target_os = "macos"))]
    let _ = capture_window::capture_proxy().send_event(CaptureCommand::Close);

    emit_workflow_state(&app, "", "", false)?;
    Ok(())
}

fn close_capture_window(app: &AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        w.hide()?;
    }
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
        #[cfg(target_os = "macos")]
        let _ = app.set_dock_visibility(true);

        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
            let _ = main_window.set_focus();
            capture::debug_log("[state] restored main window");
        }
    }
}

#[cfg(target_os = "macos")]
fn build_capture_preview_base64(preview_bytes: Vec<u8>, preview_mime: &str) -> (String, String) {
    let extension = preview_mime
        .split('/')
        .nth(1)
        .filter(|value| !value.is_empty())
        .unwrap_or("bin");
    capture::debug_write_bytes(&format!("02_preview.{extension}"), &preview_bytes);
    capture::debug_log(format!(
        "[preview] reused capture preview mime={} bytes={}",
        preview_mime,
        preview_bytes.len()
    ));
    (BASE64_STANDARD.encode(preview_bytes), preview_mime.to_string())
}

#[cfg(target_os = "macos")]
fn capture_timeline(started: &std::time::Instant, message: impl AsRef<str>) {
    capture::debug_log(format!(
        "[timeline][backend] +{}ms {}",
        started.elapsed().as_millis(),
        message.as_ref()
    ));
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
    let (settings, monitor_x, monitor_y, monitor_width, monitor_height) = {
        let guard = state.capture_session.read().await;
        let session = guard
            .as_ref()
            .ok_or_else(|| AppError::Capture("capture session missing".into()))?;
        (
            state.settings.read().await.clone(),
            session.monitor_x,
            session.monitor_y,
            session.monitor_width,
            session.monitor_height,
        )
    };
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
                monitor_x,
                monitor_y,
                monitor_width,
                monitor_height,
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
fn create_capture_window(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    let t0 = std::time::Instant::now();

    if let Some(w) = app.get_webview_window(CAPTURE_WINDOW_LABEL) {
        w.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
        w.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
        let _ = w.set_visible_on_all_workspaces(true);
        let _ = w.eval("window.location.reload()");
        w.show()?;
        configure_capture_window_macos(&w)?;
        w.set_focus()?;
        capture::debug_log(format!(
            "[timeline][backend][window] +{}ms reused existing capture window",
            t0.elapsed().as_millis()
        ));
        return Ok(());
    }

    let url = WebviewUrl::App("capture.html".into());
    let window = WebviewWindowBuilder::new(app, CAPTURE_WINDOW_LABEL, url)
        .title("Capture")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(true)
        .visible(false)
        .position(0.0, 0.0)
        .inner_size(100.0, 100.0)
        .build()?;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms builder finished",
        t0.elapsed().as_millis()
    ));
    window.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms position set x={} y={}",
        t0.elapsed().as_millis(),
        x,
        y
    ));
    window.set_size(Size::Physical(PhysicalSize::new(width, height)))?;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms size set {}x{}",
        t0.elapsed().as_millis(),
        width,
        height
    ));
    #[cfg(target_os = "macos")]
    let _ = window.set_visible_on_all_workspaces(true);
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms visible_on_all_workspaces set",
        t0.elapsed().as_millis()
    ));
    window.show()?;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms window shown",
        t0.elapsed().as_millis()
    ));
    #[cfg(target_os = "macos")]
    configure_capture_window_macos(&window)?;
    window.set_focus()?;
    #[cfg(target_os = "macos")]
    capture::debug_log(format!(
        "[timeline][backend][window] +{}ms window focused",
        t0.elapsed().as_millis()
    ));
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_capture_window_macos(window: &tauri::WebviewWindow) -> AppResult<()> {
    use objc2_app_kit::{NSMainMenuWindowLevel, NSWindow, NSWindowCollectionBehavior};

    let (tx, rx) = mpsc::sync_channel(1);
    let dispatcher = window.clone();
    let window = window.clone();
    let t0 = std::time::Instant::now();

    dispatcher.run_on_main_thread(move || {
        let result = (|| -> Result<(), String> {
            let raw = window.ns_window().map_err(|e| e.to_string())?;
            unsafe {
                let ns_window: &NSWindow = &*raw.cast();
                let mut behavior = ns_window.collectionBehavior();
                behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::FullScreenAuxiliary;
                ns_window.setCollectionBehavior(behavior);
                ns_window.setLevel(NSMainMenuWindowLevel + 1);
                ns_window.setCanHide(false);
                ns_window.setHidesOnDeactivate(false);
                ns_window.orderFrontRegardless();
            }
            Ok(())
        })();

        let _ = tx.send(result);
    })?;

    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => {
            capture::debug_log(format!(
                "[begin] promoted capture window above menu bar in {}ms",
                t0.elapsed().as_millis()
            ));
            Ok(())
        }
        Ok(Err(err)) => Err(AppError::Capture(format!(
            "failed to configure macOS capture window: {err}"
        ))),
        Err(err) => Err(AppError::Capture(format!(
            "timed out configuring macOS capture window: {err}"
        ))),
    }
}

fn create_overlay_window(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> AppResult<()> {
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

