use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::YoudaoClient;
use crate::config::ConfigStore;
use crate::models::{OverlayPayload, TranslatorSettings};
use crate::translate_engine::TextTranslator;

#[derive(Clone)]
pub struct ActiveCaptureSession {
    pub rgba: Vec<u8>,
    pub img_w: u32,
    pub img_h: u32,
    pub scale_factor: f64,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    #[cfg(target_os = "macos")]
    pub display_id: u32,
    #[cfg(target_os = "macos")]
    pub preview_image_base64: Option<String>,
    #[cfg(target_os = "macos")]
    pub preview_image_mime: String,
    #[cfg(target_os = "macos")]
    pub restore_main_window: bool,
}

#[derive(Clone)]
pub struct SharedState {
    pub config_store: Arc<ConfigStore>,
    pub settings: Arc<RwLock<TranslatorSettings>>,
    pub api_client: Arc<YoudaoClient>,
    pub text_translator: Arc<TextTranslator>,
    pub capture_in_progress: Arc<RwLock<bool>>,
    pub capture_session: Arc<RwLock<Option<ActiveCaptureSession>>>,
    pub overlay_payload: Arc<RwLock<Option<OverlayPayload>>>,
}

impl SharedState {
    pub fn new(
        config_store: ConfigStore,
        settings: TranslatorSettings,
        api_client: YoudaoClient,
        text_translator: TextTranslator,
    ) -> Self {
        Self {
            config_store: Arc::new(config_store),
            settings: Arc::new(RwLock::new(settings)),
            api_client: Arc::new(api_client),
            text_translator: Arc::new(text_translator),
            capture_in_progress: Arc::new(RwLock::new(false)),
            capture_session: Arc::new(RwLock::new(None)),
            overlay_payload: Arc::new(RwLock::new(None)),
        }
    }
}