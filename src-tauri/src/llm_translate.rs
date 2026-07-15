use std::sync::Arc;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::TextTranslationResult;

pub struct LlmTranslateClient {
    http: Arc<Client>,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

impl LlmTranslateClient {
    pub fn new(http: Arc<Client>) -> Self {
        Self { http }
    }

    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
        auto_prompt: &str,
    ) -> AppResult<TextTranslationResult> {
        let from_label = lang_label(from);
        let to_label = lang_label(to);

        // Pick the prompt template based on whether the source language is
        // auto-detect. Each has its own user-configurable template, falling back
        // to the built-in default when empty. `{from}` and `{to}` placeholders
        // are substituted with the resolved language labels.
        let template = if from == "auto" {
            if auto_prompt.trim().is_empty() {
                crate::models::default_llm_auto_prompt()
            } else {
                auto_prompt.to_string()
            }
        } else if prompt.trim().is_empty() {
            crate::models::default_llm_prompt()
        } else {
            prompt.to_string()
        };
        let system_prompt = template
            .replace("{from}", from_label)
            .replace("{to}", to_label);

        let url = base_url.trim().to_string();

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                },
            ],
            temperature: 0.3,
        };

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Api(format!("LLM translate request failed: {e}")))?;

        let status = resp.status();
        let body_text = resp
            .text()
            .await
            .map_err(|e| AppError::Api(format!("LLM translate read body failed: {e}")))?;

        if !status.is_success() {
            let detail = &body_text[..body_text.len().min(500)];
            return Err(AppError::Api(format!(
                "LLM API error (HTTP {}): {}",
                status.as_u16(),
                detail
            )));
        }

        let chat_resp: ChatResponse = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Api(format!("LLM translate parse failed: {e}")))?;

        let translated = chat_resp
            .choices
            .first()
            .and_then(|c| Some(c.message.content.clone()))
            .unwrap_or_default()
            .trim()
            .to_string();

        if translated.is_empty() {
            return Err(AppError::Api("LLM returned empty translation".into()));
        }

        Ok(TextTranslationResult {
            translated_text: translated,
            from_lang_detected: from.to_string(),
            alternatives: Vec::new(),
        })
    }
}

fn lang_label(code: &str) -> &str {
    match code {
        "auto" => "auto-detect",
        "zh-CHS" => "Simplified Chinese",
        "zh-CHT" => "Traditional Chinese",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "ru" => "Russian",
        "es" => "Spanish",
        _ => code,
    }
}