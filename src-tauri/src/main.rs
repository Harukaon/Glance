#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app_state;
mod bing_translate;
mod builtin_translate;
mod capture;
mod capture_window;
mod commands;
mod config;
mod error;
mod google_translate;
mod llm_translate;
mod models;
mod popup_shortcut;
mod secure;
mod self_test;
mod startup;
mod translate_cache;
mod translate_engine;

use std::path::PathBuf;

use api::YoudaoClient;
use app_state::SharedState;
use bing_translate::BingTranslateClient;
use builtin_translate::BuiltinTranslateClient;
use llm_translate::LlmTranslateClient;
use commands::{
    begin_capture, begin_copy_capture, cancel_capture, capture_debug_log, clear_history,
    clear_text_history, clear_translate_cache, close_overlay, hide_window, list_history,
    list_text_history, load_capture_payload, load_overlay_payload, load_settings,
    resize_main_window, save_settings, show_overlay, submit_capture_selection, translate_text,
};
use config::ConfigStore;
use models::TranslatorSettings;
use translate_engine::TextTranslator;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::EnvFilter;

/// Wire tracing to a rolling log file (in the app data dir) plus stderr.
/// Release builds have no console attached (`windows_subsystem = "windows"`),
/// so without a file the logs would be lost entirely on Windows.
fn setup_logging() {
    let filter =
        EnvFilter::from_default_env().add_directive("info".parse().unwrap());

    if let Some(dir) = dirs_of_log_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let appender = tracing_appender::rolling::daily(dir, "glance.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);
        // Keep the non-blocking guard alive for the process lifetime.
        std::mem::forget(guard);
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(non_blocking)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}

fn dirs_of_log_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("GLANCE_LOG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            // Same location as tauri's AppData dir on Windows:
            // %APPDATA%\com.harukaon.glance\logs
            let appdata = std::env::var_os("APPDATA")?;
            Some(
                std::path::PathBuf::from(appdata)
                    .join("com.harukaon.glance")
                    .join("logs"),
            )
        })
}

fn main() {
    setup_logging();

    if self_test::should_run_capture_self_test() {
        match self_test::run_capture_self_test() {
            Ok(result) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result)
                        .expect("capture self test result should serialize")
                );
                return;
            }
            Err(err) => {
                eprintln!("capture self test failed: {err}");
                std::process::exit(1);
            }
        }
    }

    if self_test::should_run_capture_smoke_test() {
        match self_test::run_capture_smoke_test() {
            Ok(result) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result)
                        .expect("capture smoke test result should serialize")
                );
                return;
            }
            Err(err) => {
                eprintln!("capture smoke test failed: {err}");
                std::process::exit(1);
            }
        }
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !startup::is_silent_launch(args) {
                show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            // Launch silently on boot: the autostart entry runs the app with this
            // flag so the main window stays hidden (tray only) on OS startup.
            Some(vec![startup::SILENT_START_ARG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let base_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".").join(".glance"));

            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                let config_store = ConfigStore::new(base_dir);
                config_store.ensure().await?;
                let settings = config_store
                    .load_settings()
                    .await
                    .unwrap_or_else(|_| TranslatorSettings::default());

                commands::apply_autostart(&app_handle, settings.autostart);
                commands::apply_hotkey(&app_handle, &settings.hotkey);
                if let Some(ref popup) = settings.popup_shortcut {
                    commands::apply_popup_shortcut(&app_handle, popup);
                }
                commands::apply_copy_hotkey(&app_handle, &settings.copy_hotkey);

                // ── HTTP clients ────────────────────────────────────────────────
                // General client for Youdao
                let mut general_headers = reqwest::header::HeaderMap::new();
                general_headers.insert(reqwest::header::ACCEPT, "*/*".parse().unwrap());
                general_headers.insert(reqwest::header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());

                let general_http = std::sync::Arc::new(
                    reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .default_headers(general_headers)
                        .connect_timeout(std::time::Duration::from_secs(5))
                        .timeout(std::time::Duration::from_secs(10))
                        .build()?,
                );

                // Bing client with Bing-specific headers
                let mut bing_headers = reqwest::header::HeaderMap::new();
                bing_headers.insert(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap());
                bing_headers.insert(reqwest::header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
                bing_headers.insert(reqwest::header::ORIGIN, "https://cn.bing.com".parse().unwrap());

                let bing_http = std::sync::Arc::new(
                    reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .default_headers(bing_headers)
                        .redirect(reqwest::redirect::Policy::none())
                        .connect_timeout(std::time::Duration::from_secs(5))
                        .timeout(std::time::Duration::from_secs(10))
                        .build()?,
                );

                let api_client = YoudaoClient::new(general_http.clone());
                let bing_client = BingTranslateClient::new(bing_http);
                let builtin_client = BuiltinTranslateClient::new();
                let llm_client = LlmTranslateClient::new(general_http);
                let text_translator = TextTranslator::new(bing_client, builtin_client, llm_client);
                app_handle.manage(SharedState::new(
                    config_store,
                    settings,
                    api_client,
                    text_translator,
                ));
                Ok::<(), error::AppError>(())
            })?;

            // ── System tray ──────────────────────────────────────────────
            let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))?;

            let show = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("Glance")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        show_main_window(app);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                        if button == tauri::tray::MouseButton::Left {
                            show_main_window(&tray.app_handle());
                        }
                    }
                })
                .build(app)?;

            // ── Intercept window close → hide to tray ────────────────────
            let main_window = app.get_webview_window("main").unwrap();

            // Silent startup: when launched via OS autostart the app is run with
            // `--minimized`, so keep the main window hidden (tray only). The window
            // is created hidden (see tauri.conf.json), so a normal launch must
            // explicitly show it here.
            if startup::is_silent_launch(std::env::args_os()) {
                hide_main_window_to_background(app.handle());
            } else {
                show_main_window(app.handle());
            }

            let mw = main_window.clone();
            main_window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_main_window_to_background(&mw.app_handle());
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            list_history,
            clear_history,
            clear_text_history,
            clear_translate_cache,
            list_text_history,
            begin_capture,
            begin_copy_capture,
            cancel_capture,
            load_capture_payload,
            submit_capture_selection,
            show_overlay,
            load_overlay_payload,
            close_overlay,
            translate_text,
            resize_main_window,
            hide_window,
            capture_debug_log
        ])
        .build(tauri::generate_context!())
        .expect("failed to build tauri app");

    app.run(|_app_handle, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main_window(_app_handle);
        }
    });
}

fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);

    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(target_os = "macos")]
fn hide_main_window_to_background(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    let _ = app.set_dock_visibility(false);
}

#[cfg(not(target_os = "macos"))]
fn hide_main_window_to_background(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}
