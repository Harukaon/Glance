use std::sync::Arc;
use std::time::Duration;

use crate::bing_translate::BingTranslateClient;
use crate::builtin_translate::BuiltinTranslateClient;
use crate::error::AppResult;
use crate::llm_translate::LlmTranslateClient;
use crate::models::{LlmConfig, TextTranslateEngine, TextTranslationResult};

#[derive(Clone)]
pub struct TextTranslator {
    bing: Arc<BingTranslateClient>,
    builtin: Arc<BuiltinTranslateClient>,
    llm: Arc<LlmTranslateClient>,
}

pub struct TranslationOutcome {
    pub result: TextTranslationResult,
    pub engine: TextTranslateEngine,
}

/// How many times a transient failure (timeout, 5xx, 429, network) retries the
/// primary engine before falling back.
const MAX_RETRIES: u32 = 2;
const RETRY_BASE_DELAY_MS: u64 = 300;

impl TextTranslator {
    pub fn new(
        bing: BingTranslateClient,
        builtin: BuiltinTranslateClient,
        llm: LlmTranslateClient,
    ) -> Self {
        Self {
            bing: Arc::new(bing),
            builtin: Arc::new(builtin),
            llm: Arc::new(llm),
        }
    }

    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
        engine: TextTranslateEngine,
        llm_config: &LlmConfig,
        proxy: Option<&str>,
    ) -> AppResult<TranslationOutcome> {
        // Retry transient failures on the primary engine with a small backoff.
        let mut attempt = 0;
        let primary_err = loop {
            match self
                .translate_once(text, from, to, engine, llm_config, proxy)
                .await
            {
                Ok(result) => return Ok(TranslationOutcome { result, engine }),
                Err(err) if err.is_transient() && attempt < MAX_RETRIES => {
                    attempt += 1;
                    tracing::warn!(
                        "engine {engine:?} transient failure (attempt {attempt}): {err}"
                    );
                    tokio::time::sleep(Duration::from_millis(
                        RETRY_BASE_DELAY_MS * (attempt as u64),
                    ))
                    .await;
                }
                Err(err) => break err,
            }
        };

        // Retries exhausted: degrade to a free fallback engine so the user
        // still gets a result. Never fall back *to* the LLM (it costs money).
        if let Some(fallback) = fallback_engine(engine) {
            tracing::warn!("engine {engine:?} failed, falling back to {fallback:?}: {primary_err}");
            let mut result = self
                .translate_once(text, from, to, fallback, llm_config, proxy)
                .await?;
            result
                .alternatives
                .push("⚠ 主引擎不可用，已自动使用备用引擎".to_string());
            return Ok(TranslationOutcome {
                result,
                engine: fallback,
            });
        }
        Err(primary_err)
    }

    async fn translate_once(
        &self,
        text: &str,
        from: &str,
        to: &str,
        engine: TextTranslateEngine,
        llm_config: &LlmConfig,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        match engine {
            TextTranslateEngine::Bing => self.bing.translate(text, from, to).await,
            TextTranslateEngine::Google => self.builtin.google(text, from, to, proxy).await,
            TextTranslateEngine::Microsoft => self.builtin.microsoft(text, from, to, proxy).await,
            TextTranslateEngine::Transmart => self.builtin.transmart(text, from, to, proxy).await,
            TextTranslateEngine::Yandex => self.builtin.yandex(text, from, to, proxy).await,
            TextTranslateEngine::Iciba => self.builtin.iciba(text, from, to, proxy).await,
            TextTranslateEngine::Llm => {
                self.llm
                    .translate(
                        text,
                        from,
                        to,
                        &llm_config.base_url,
                        &llm_config.api_key,
                        &llm_config.model,
                        &llm_config.prompt,
                        &llm_config.auto_prompt,
                        (llm_config.max_tokens > 0).then_some(llm_config.max_tokens),
                    )
                    .await
            }
        }
    }
}

/// Backup engine used when the configured engine fails after retries.
/// Both candidates are key-free engines; LLM is intentionally excluded.
fn fallback_engine(engine: TextTranslateEngine) -> Option<TextTranslateEngine> {
    match engine {
        TextTranslateEngine::Bing => Some(TextTranslateEngine::Google),
        TextTranslateEngine::Llm | TextTranslateEngine::Google => Some(TextTranslateEngine::Bing),
        TextTranslateEngine::Microsoft
        | TextTranslateEngine::Transmart
        | TextTranslateEngine::Yandex
        | TextTranslateEngine::Iciba => Some(TextTranslateEngine::Bing),
    }
}
