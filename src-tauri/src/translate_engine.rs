use std::sync::Arc;

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
                    )
                    .await
            }
        }
    }
}