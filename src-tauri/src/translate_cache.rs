//! Exact-match, persisted LRU cache for text translations.
//!
//! The key is the full source text + language pair + engine (+ model), so a
//! hit only happens when the exact same query was translated before — the
//! cached result is identical to what a fresh request would return. This is
//! purely a latency/cost optimization, never a quality one: partial or fuzzy
//! matches are deliberately not supported.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::models::{LlmConfig, TextTranslateEngine, TextTranslationResult};

/// Skip caching very large inputs so the cache file stays small.
const MAX_CACHED_TEXT_LEN: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    /// sha256 hex of `text|from|to|engine|configuration variant`.
    key: String,
    result: TextTranslationResult,
    /// Unix timestamp (seconds) of the last access, used for eviction.
    last_used: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct CacheFile {
    entries: Vec<CacheEntry>,
}

pub struct TranslateCache {
    path: PathBuf,
    inner: Mutex<CacheState>,
}

struct CacheState {
    entries: Vec<CacheEntry>,
    max_entries: usize,
    dirty: bool,
}

impl TranslateCache {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            inner: Mutex::new(CacheState {
                entries: Vec::new(),
                max_entries: 200,
                dirty: false,
            }),
        }
    }

    pub async fn load(&self) {
        let mut state = self.inner.lock().await;
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => match serde_json::from_slice::<CacheFile>(&bytes) {
                Ok(file) => state.entries = file.entries,
                Err(err) => tracing::warn!("translate cache load failed: {err}"),
            },
            Err(_) => {} // no cache file yet
        }
    }

    pub async fn get(
        &self,
        text: &str,
        from: &str,
        to: &str,
        engine: &str,
        variant: &str,
    ) -> Option<TextTranslationResult> {
        if text.len() > MAX_CACHED_TEXT_LEN {
            return None;
        }
        let cache_key = make_key(text, from, to, engine, variant);
        let mut state = self.inner.lock().await;
        let now = now_secs();
        let mut hit = false;
        if let Some(entry) = state.entries.iter_mut().find(|e| e.key == cache_key) {
            // Defensive: never serve a stale "identity" entry (the model once
            // echoed the input back). Treat it as a miss and drop it so the
            // request goes out again.
            if entry.result.translated_text.trim() == text.trim() {
                state.entries.retain(|e| e.key != cache_key);
                state.dirty = true;
            } else {
                entry.last_used = now;
                hit = true;
            }
        }
        if hit {
            state.dirty = true;
            let entry = state.entries.iter().find(|e| e.key == cache_key);
            return entry.map(|e| e.result.clone());
        }
        None
    }

    pub async fn insert(
        &self,
        text: &str,
        from: &str,
        to: &str,
        engine: &str,
        variant: &str,
        result: TextTranslationResult,
    ) {
        if text.len() > MAX_CACHED_TEXT_LEN {
            return;
        }
        // Never cache an identity result (model echoed the input): it is
        // almost always a direction/mismatch artifact, and caching it would
        // poison every later request for the same text.
        if result.translated_text.trim() == text.trim() {
            return;
        }
        let cache_key = make_key(text, from, to, engine, variant);
        let mut state = self.inner.lock().await;
        if let Some(entry) = state.entries.iter_mut().find(|e| e.key == cache_key) {
            entry.result = result;
            entry.last_used = now_secs();
            state.dirty = true;
            return;
        }
        state.entries.push(CacheEntry {
            key: cache_key,
            result,
            last_used: now_secs(),
        });
        // Evict the least recently used entry when over the cap.
        while state.entries.len() > state.max_entries {
            let mut oldest = 0usize;
            for (i, e) in state.entries.iter().enumerate() {
                if e.last_used < state.entries[oldest].last_used {
                    oldest = i;
                }
            }
            state.entries.swap_remove(oldest);
        }
        state.dirty = true;
    }

    pub async fn set_max_entries(&self, max: usize) {
        let mut state = self.inner.lock().await;
        let max = max.max(1);
        state.max_entries = max;
        while state.entries.len() > max {
            let mut oldest = 0usize;
            for (i, e) in state.entries.iter().enumerate() {
                if e.last_used < state.entries[oldest].last_used {
                    oldest = i;
                }
            }
            state.entries.swap_remove(oldest);
        }
        state.dirty = true;
    }

    pub async fn clear(&self) {
        let mut state = self.inner.lock().await;
        state.entries.clear();
        state.dirty = true;
    }

    pub async fn entry_count(&self) -> usize {
        self.inner.lock().await.entries.len()
    }

    /// Persist the cache to disk if anything changed. Cheap to call often:
    /// it's a no-op when clean.
    pub async fn save(&self) {
        let mut state = self.inner.lock().await;
        if !state.dirty {
            return;
        }
        let bytes = match serde_json::to_vec(&CacheFile {
            entries: state.entries.clone(),
        }) {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!("translate cache serialize failed: {err}");
                return;
            }
        };
        if let Err(err) = tokio::fs::write(&self.path, bytes).await {
            tracing::warn!("translate cache save failed: {err}");
            return;
        }
        state.dirty = false;
    }
}

fn make_key(text: &str, from: &str, to: &str, engine: &str, variant: &str) -> String {
    let mut hasher = Sha256::new();
    // Version the key so older entries that omitted effective LLM settings can
    // never be served after an upgrade.
    hasher.update(b"glance-translation-cache-v2");
    hasher.update([0u8]);
    hasher.update(text.as_bytes());
    hasher.update([0u8]);
    hasher.update(from.as_bytes());
    hasher.update([0u8]);
    hasher.update(to.as_bytes());
    hasher.update([0u8]);
    hasher.update(engine.as_bytes());
    hasher.update([0u8]);
    hasher.update(variant.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Settings that can change an LLM translation's output. The API key is
/// deliberately excluded: credentials do not affect translation semantics.
pub fn configuration_variant(engine: TextTranslateEngine, llm: &LlmConfig) -> String {
    if engine != TextTranslateEngine::Llm {
        return String::new();
    }

    let mut hasher = Sha256::new();
    hasher.update(b"llm-configuration-v1");
    for value in [
        llm.base_url.trim(),
        llm.model.trim(),
        llm.prompt.as_str(),
        llm.auto_prompt.as_str(),
    ] {
        hasher.update([0u8]);
        hasher.update(value.as_bytes());
    }
    hasher.update([0u8]);
    hasher.update(llm.max_tokens.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_configuration_changes_invalidate_cache_variant() {
        let base = LlmConfig::default();
        let base_variant = configuration_variant(TextTranslateEngine::Llm, &base);

        let mut cases = Vec::new();
        let mut changed = base.clone();
        changed.base_url.push_str("/other");
        cases.push(changed);
        let mut changed = base.clone();
        changed.model.push_str("-other");
        cases.push(changed);
        let mut changed = base.clone();
        changed.prompt.push_str(" Be concise.");
        cases.push(changed);
        let mut changed = base.clone();
        changed.auto_prompt.push_str(" Be concise.");
        cases.push(changed);
        let mut changed = base.clone();
        changed.max_tokens += 1;
        cases.push(changed);

        for changed in cases {
            assert_ne!(
                base_variant,
                configuration_variant(TextTranslateEngine::Llm, &changed)
            );
        }
    }

    #[test]
    fn non_llm_engines_ignore_llm_configuration() {
        assert_eq!(
            configuration_variant(TextTranslateEngine::Bing, &LlmConfig::default()),
            ""
        );
    }
}
