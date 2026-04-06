#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app_state;
mod capture;
mod capture_window;
mod commands;
mod config;
mod error;
mod google_translate;
mod models;

use std::path::PathBuf;

use api::YoudaoClient;
use app_state::SharedState;
use commands::{
    begin_capture, cancel_capture, clear_history, close_overlay, hide_window, list_history,
    load_capture_payload, load_overlay_payload, load_settings, save_settings, show_overlay,
    submit_capture_selection, translate_text,
};
use config::ConfigStore;
use models::TranslatorSettings;
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

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
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

                let http = std::sync::Arc::new(
                    reqwest::Client::builder()
                        .user_agent("glance/0.1")
                        .build()?,
                );
                let api_client = YoudaoClient::new(http.clone());
                let google_client = google_translate::GoogleTranslateClient::new(http);
                app_handle.manage(SharedState::new(config_store, settings, api_client, google_client));
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
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                        if button == tauri::tray::MouseButton::Left {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
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
                    let _ = mw.hide();
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
            hide_window
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
