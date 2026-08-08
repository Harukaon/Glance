use std::path::PathBuf;

use tokio::fs;

use crate::error::AppResult;
use crate::models::{TextHistoryItem, TranslationHistoryItem, TranslatorSettings};

#[derive(Debug, Clone)]
pub struct ConfigStore {
    base_dir: PathBuf,
    settings_file: PathBuf,
    history_file: PathBuf,
    text_history_file: PathBuf,
    cache_file: PathBuf,
}

impl ConfigStore {
    pub fn new(base_dir: PathBuf) -> Self {
        let settings_file = base_dir.join("settings.json");
        let history_file = base_dir.join("history.json");
        let text_history_file = base_dir.join("text_history.json");
        let cache_file = base_dir.join("translate_cache.json");
        Self {
            base_dir,
            settings_file,
            history_file,
            text_history_file,
            cache_file,
        }
    }

    pub fn translate_cache_path(&self) -> PathBuf {
        self.cache_file.clone()
    }

    pub async fn ensure(&self) -> AppResult<()> {
        fs::create_dir_all(&self.base_dir).await?;
        if fs::metadata(&self.settings_file).await.is_err() {
            let bytes = serde_json::to_vec_pretty(&TranslatorSettings::default())?;
            fs::write(&self.settings_file, bytes).await?;
        }
        if fs::metadata(&self.history_file).await.is_err() {
            fs::write(&self.history_file, b"[]").await?;
        }
        if fs::metadata(&self.text_history_file).await.is_err() {
            fs::write(&self.text_history_file, b"[]").await?;
        }
        Ok(())
    }

    pub async fn load_settings(&self) -> AppResult<TranslatorSettings> {
        self.ensure().await?;
        let bytes = fs::read(&self.settings_file).await?;
        let mut settings: TranslatorSettings = serde_json::from_slice(&bytes)?;
        // API key is stored encrypted on Windows; decrypt on load.
        let stored_key = settings.llm_config.api_key.clone();
        let legacy_plaintext = !stored_key.is_empty() && !stored_key.starts_with("enc:v1:");
        settings.llm_config.api_key = crate::secure::decrypt(&stored_key);
        // One-time migration: persist the legacy plaintext key encrypted.
        if legacy_plaintext {
            let _ = self.save_settings(&settings).await;
        }
        Ok(settings)
    }

    pub async fn save_settings(&self, settings: &TranslatorSettings) -> AppResult<()> {
        self.ensure().await?;
        let mut stored = settings.clone();
        // Encrypt the API key at rest. `encrypt` falls back to plaintext when
        // DPAPI is unavailable, and re-encrypting an already-encrypted value is
        // a no-op (it is recognized by its prefix).
        stored.llm_config.api_key = crate::secure::encrypt(&stored.llm_config.api_key);
        let bytes = serde_json::to_vec_pretty(&stored)?;
        fs::write(&self.settings_file, bytes).await?;
        Ok(())
    }

    pub async fn load_history(&self) -> AppResult<Vec<TranslationHistoryItem>> {
        self.ensure().await?;
        let bytes = fs::read(&self.history_file).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_history(&self, history: &[TranslationHistoryItem]) -> AppResult<()> {
        self.ensure().await?;
        let bytes = serde_json::to_vec_pretty(history)?;
        fs::write(&self.history_file, bytes).await?;
        Ok(())
    }

    pub async fn load_text_history(&self) -> AppResult<Vec<TextHistoryItem>> {
        self.ensure().await?;
        let bytes = fs::read(&self.text_history_file).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_text_history(&self, history: &[TextHistoryItem]) -> AppResult<()> {
        self.ensure().await?;
        let bytes = serde_json::to_vec_pretty(history)?;
        fs::write(&self.text_history_file, bytes).await?;
        Ok(())
    }
}
