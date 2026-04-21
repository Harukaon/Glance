#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app_state;
mod bing_translate;
mod capture;
mod capture_window;
mod commands;
mod config;
mod error;
mod google_translate;
mod llm_translate;
mod models;
mod popup_shortcut;
mod self_test;
mod translate_engine;

use std::path::PathBuf;

use api::YoudaoClient;
use app_state::SharedState;
use bing_translate::BingTranslateClient;
use llm_translate::LlmTranslateClient;
use commands::{
    begin_capture, cancel_capture, capture_debug_log, clear_history, close_overlay, hide_window,
    list_history, load_capture_payload, load_overlay_payload, load_settings, resize_main_window,
    save_settings, show_overlay, submit_capture_selection, translate_text,
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
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

                // ── HTTP clients ────────────────────────────────────────────────
                // General client for Youdao
                let mut general_headers = reqwest::header::HeaderMap::new();
                general_headers.insert(reqwest::header::ACCEPT, "*/*".parse().unwrap());
                general_headers.insert(reqwest::header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());

                let general_http = std::sync::Arc::new(
                    reqwest::Client::builder()
                        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .default_headers(general_headers)
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
                        .build()?,
                );

                let api_client = YoudaoClient::new(general_http.clone());
                let bing_client = BingTranslateClient::new(bing_http);
                let llm_client = LlmTranslateClient::new(general_http);
                let text_translator = TextTranslator::new(bing_client, llm_client);
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
            begin_capture,
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
