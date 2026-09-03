use std::path::PathBuf;
use std::sync::Arc;

use tokio::fs;
use tokio::sync::Mutex;

use crate::error::AppResult;
use crate::models::{TextHistoryItem, TranslationHistoryItem, TranslatorSettings};

#[derive(Debug, Clone)]
pub struct ConfigStore {
    base_dir: PathBuf,
    settings_file: PathBuf,
    history_file: PathBuf,
    text_history_file: PathBuf,
    cache_file: PathBuf,
    text_history_lock: Arc<Mutex<()>>,
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
            text_history_lock: Arc::new(Mutex::new(())),
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
        let _guard = self.text_history_lock.lock().await;
        self.load_text_history_unlocked().await
    }

    pub async fn append_text_history(&self, item: TextHistoryItem, limit: usize) -> AppResult<()> {
        let _guard = self.text_history_lock.lock().await;
        let mut history = self.load_text_history_unlocked().await?;
        history.push(item);
        let overflow = history.len().saturating_sub(limit);
        if overflow > 0 {
            history.drain(..overflow);
        }
        self.write_text_history_unlocked(&history).await
    }

    pub async fn clear_text_history(&self) -> AppResult<()> {
        let _guard = self.text_history_lock.lock().await;
        self.write_text_history_unlocked(&[]).await
    }

    async fn load_text_history_unlocked(&self) -> AppResult<Vec<TextHistoryItem>> {
        self.ensure().await?;
        let bytes = fs::read(&self.text_history_file).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn write_text_history_unlocked(&self, history: &[TextHistoryItem]) -> AppResult<()> {
        self.ensure().await?;
        let bytes = serde_json::to_vec_pretty(history)?;
        fs::write(&self.text_history_file, bytes).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;

    use super::*;
    use crate::models::TextSourceSide;

    fn history_item(source: impl Into<String>) -> TextHistoryItem {
        let source = source.into();
        TextHistoryItem {
            id: source.clone(),
            created_at: Utc::now(),
            from_lang: "en".to_string(),
            to_lang: "zh-CHS".to_string(),
            engine: "bing".to_string(),
            source,
            translated: "译文".to_string(),
            source_side: TextSourceSide::Left,
        }
    }

    fn test_store(name: &str) -> ConfigStore {
        let dir = std::env::temp_dir().join(format!("glance-{name}-{}", uuid::Uuid::new_v4()));
        ConfigStore::new(dir)
    }

    #[tokio::test]
    async fn text_history_limit_keeps_newest_entries() {
        let store = test_store("history-limit");
        store
            .append_text_history(history_item("one"), 2)
            .await
            .unwrap();
        store
            .append_text_history(history_item("two"), 2)
            .await
            .unwrap();
        store
            .append_text_history(history_item("three"), 2)
            .await
            .unwrap();

        let history = store.load_text_history().await.unwrap();
        let sources: Vec<_> = history.iter().map(|item| item.source.as_str()).collect();
        assert_eq!(sources, ["two", "three"]);
        let _ = tokio::fs::remove_dir_all(&store.base_dir).await;
    }

    #[tokio::test]
    async fn concurrent_text_history_appends_do_not_lose_entries() {
        let store = Arc::new(test_store("history-concurrency"));
        let mut tasks = Vec::new();
        for index in 0..20 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .append_text_history(history_item(index.to_string()), 100)
                    .await
                    .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let history = store.load_text_history().await.unwrap();
        let ids: HashSet<_> = history.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(history.len(), 20);
        assert_eq!(ids.len(), 20);
        let _ = tokio::fs::remove_dir_all(&store.base_dir).await;
    }
}
