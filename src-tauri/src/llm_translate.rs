use std::sync::Arc;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::TextTranslationResult;

pub struct LlmTranslateClient {
    http: Arc<Client>,
}

const LLM_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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
    #[serde(default)]
    finish_reason: Option<String>,
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
        max_tokens: Option<u32>,
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

        // A hard rule appended AFTER the user's custom prompt: the model must
        // never chat, refuse, or echo instructions. Whatever the input
        // language, it has to produce target-language text (or the input
        // unchanged when it already is in the target language). This keeps
        // mixed-language or wrong-side input from producing chatty replies.
        let hard_rule = format!(
            "Hard rule: the user text may be in any language, not necessarily {from_label}. \
             Always output the translation in {to_label}. If the text is entirely in {to_label}, \
             output it unchanged. If the text contains parts in other languages, translate those \
             parts into {to_label} so the whole output is in {to_label}. Never explain, comment, \
             ask questions, or refuse — output only the {to_label} text."
        );

        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt,
                },
                ChatMessage {
                    role: "system".to_string(),
                    content: hard_rule,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                },
            ],
            temperature: 0.3,
            max_tokens,
        };

        let (status, body_text) = tokio::time::timeout(LLM_REQUEST_TIMEOUT, async {
            let resp = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
                .map_err(AppError::Network)?;
            let status = resp.status();
            let body_text = resp.text().await.map_err(AppError::Network)?;
            Ok::<_, AppError>((status, body_text))
        })
        .await
        .map_err(|_| AppError::Timeout("LLM request timed out".into()))??;

        if !status.is_success() {
            let detail = truncate_chars(&body_text, 500);
            return Err(AppError::HttpStatus(
                status.as_u16(),
                format!("LLM API error (HTTP {}): {}", status.as_u16(), detail),
            ));
        }

        let chat_resp: ChatResponse = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Api(format!("LLM translate parse failed: {e}")))?;

        let choice = chat_resp.choices.first();
        let translated = choice
            .map(|c| c.message.content.clone())
            .unwrap_or_default()
            .trim()
            .to_string();

        if translated.is_empty() {
            return Err(AppError::Api("LLM returned empty translation".into()));
        }

        // Surface truncation so the user knows the result is incomplete.
        let mut alternatives = Vec::new();
        if choice.and_then(|c| c.finish_reason.as_deref()) == Some("length") {
            alternatives.push("⚠ 输出达到 token 上限，可能被截断".to_string());
        }
        // Identity output: with the hard rule above this is only expected when
        // the input was already in the target language — otherwise it usually
        // means the source/target language pair is wrong for the text.
        if translated == text.trim() {
            alternatives.push("⚠ 结果与原文相同：可能原文已是目标语言，或方向设置有误".to_string());
        }

        // When the source is auto-detect, resolve the actual language with a
        // lightweight local heuristic. This makes the detection tag / TTS
        // language reliable without an extra API call.
        let detected = if from == "auto" {
            detect_lang(text)
        } else {
            from.to_string()
        };

        Ok(TextTranslationResult {
            translated_text: translated,
            from_lang_detected: detected,
            alternatives,
        })
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Cheap local language guess for auto-detect mode, good enough to drive the
/// detection tag and TTS voice selection. Falls back to "auto" when unclear.
fn detect_lang(text: &str) -> String {
    let mut han = 0usize;
    let mut kana = 0usize;
    let mut hangul = 0usize;
    let mut latin = 0usize;
    let mut total = 0usize;

    for c in text.chars() {
        total += 1;
        match c {
            'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' => kana += 1,
            '\u{ac00}'..='\u{d7a3}' => hangul += 1,
            '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{f900}'..='\u{faff}' => han += 1,
            'a'..='z' | 'A'..='Z' => latin += 1,
            _ => {}
        }
    }
    if total == 0 {
        return "auto".to_string();
    }

    let han_ratio = han as f64 / total as f64;
    let kana_ratio = kana as f64 / total as f64;
    let hangul_ratio = hangul as f64 / total as f64;

    if kana_ratio > 0.05 {
        "ja".to_string()
    } else if hangul_ratio > 0.1 {
        "ko".to_string()
    } else if han_ratio > 0.1 {
        "zh-CHS".to_string()
    } else if latin as f64 / total as f64 > 0.3 {
        "en".to_string()
    } else {
        "auto".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_detail_truncation_preserves_utf8_boundaries() {
        let input = format!("{}🙂tail", "中".repeat(500));
        let detail = truncate_chars(&input, 500);
        assert_eq!(detail.chars().count(), 500);
        assert_eq!(detail, "中".repeat(500));
    }
}
