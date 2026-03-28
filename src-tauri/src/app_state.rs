use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::YoudaoClient;
use crate::config::ConfigStore;
use crate::google_translate::GoogleTranslateClient;
use crate::models::{OverlayPayload, TranslatorSettings};

#[derive(Clone)]
pub struct SharedState {
    pub config_store: Arc<ConfigStore>,
    pub settings: Arc<RwLock<TranslatorSettings>>,
    pub api_client: Arc<YoudaoClient>,
    pub google_client: Arc<GoogleTranslateClient>,
    pub capture_in_progress: Arc<RwLock<bool>>,
    pub overlay_payload: Arc<RwLock<Option<OverlayPayload>>>,
}

impl SharedState {
    pub fn new(
        config_store: ConfigStore,
        settings: TranslatorSettings,
        api_client: YoudaoClient,
        google_client: GoogleTranslateClient,
    ) -> Self {
        Self {
            config_store: Arc::new(config_store),
            settings: Arc::new(RwLock::new(settings)),
            api_client: Arc::new(api_client),
            google_client: Arc::new(google_client),
            capture_in_progress: Arc::new(RwLock::new(false)),
            overlay_payload: Arc::new(RwLock::new(None)),
        }
    }
}
