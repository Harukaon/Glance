use std::path::PathBuf;

use tokio::fs;

use crate::error::AppResult;
use crate::models::{TranslationHistoryItem, TranslatorSettings};

#[derive(Debug, Clone)]
pub struct ConfigStore {
    base_dir: PathBuf,
    settings_file: PathBuf,
    history_file: PathBuf,
}

impl ConfigStore {
    pub fn new(base_dir: PathBuf) -> Self {
        let settings_file = base_dir.join("settings.json");
        let history_file = base_dir.join("history.json");
        Self {
            base_dir,
            settings_file,
            history_file,
        }
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
        Ok(())
    }

    pub async fn load_settings(&self) -> AppResult<TranslatorSettings> {
        self.ensure().await?;
        let bytes = fs::read(&self.settings_file).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn save_settings(&self, settings: &TranslatorSettings) -> AppResult<()> {
        self.ensure().await?;
        let bytes = serde_json::to_vec_pretty(settings)?;
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
}

#[cfg(test)]
mod tests {
    use super::ConfigStore;
    use crate::models::TranslatorSettings;

    // A settings.json written by an older build knows nothing about the enable
    // switches. Reading such a file has to succeed: `main.rs` swallows the error
    // and starts from `TranslatorSettings::default()`, so a failed parse would
    // quietly throw away the user's language pair and hotkeys.
    #[tokio::test]
    async fn settings_written_before_the_enable_switches_still_load() {
        let dir =
            std::env::temp_dir().join(format!("glance-legacy-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut legacy = serde_json::to_value(TranslatorSettings::default()).unwrap();
        let keys = legacy.as_object_mut().unwrap();
        for key in ["hotkeyEnabled", "copyHotkeyEnabled", "popupShortcutEnabled"] {
            keys.remove(key);
        }
        // Values the defaults do not carry, so a whole-file fallback cannot pass.
        keys.insert("hotkey".into(), serde_json::json!("CommandOrControl+Alt+K"));
        keys.insert("toLang".into(), serde_json::json!("ja"));
        keys.insert(
            "popupShortcut".into(),
            serde_json::json!("CommandOrControl+Alt+P"),
        );
        std::fs::write(
            dir.join("settings.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let settings = ConfigStore::new(dir.clone()).load_settings().await.unwrap();

        assert!(settings.hotkey_enabled);
        assert!(settings.copy_hotkey_enabled);
        assert!(settings.popup_shortcut_enabled);
        assert_eq!(settings.hotkey, "CommandOrControl+Alt+K");
        assert_eq!(settings.to_lang, "ja");
        assert_eq!(
            settings.popup_shortcut.as_deref(),
            Some("CommandOrControl+Alt+P")
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
