use std::sync::Arc;

use reqwest::Client;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::TextTranslationResult;

pub struct GoogleTranslateClient {
    http: Arc<Client>,
}

impl GoogleTranslateClient {
    pub fn new(http: Arc<Client>) -> Self {
        Self { http }
    }

    /// Translate `text` from `from` language to `to` language using Google Translate free API.
    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> AppResult<TextTranslationResult> {
        let sl = map_lang_code(from);
        let tl = map_lang_code(to);

        let resp = self
            .http
            .get("https://translate.googleapis.com/translate_a/single")
            .query(&[
                ("client", "gtx"),
                ("sl", &sl),
                ("tl", &tl),
                ("dt", "t"),
                ("q", text),
            ])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Google Translate request failed: {e}")))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Api(format!("Google Translate parse failed: {e}")))?;

        // response[0] is an array of [translated_segment, original_segment, ...]
        let mut translated = String::new();
        if let Some(segments) = body.get(0).and_then(|v| v.as_array()) {
            for seg in segments {
                if let Some(text) = seg.get(0).and_then(|v| v.as_str()) {
                    translated.push_str(text);
                }
            }
        }

        // response[2] is the detected source language code
        let detected = body
            .get(2)
            .and_then(|v| v.as_str())
            .unwrap_or(from)
            .to_string();

        if translated.is_empty() {
            return Err(AppError::Api("Google Translate returned empty result".into()));
        }

        Ok(TextTranslationResult {
            translated_text: translated,
            from_lang_detected: detected,
        })
    }
}

/// Map Youdao-style language codes to Google Translate codes.
fn map_lang_code(code: &str) -> String {
    match code {
        "zh-CHS" => "zh-CN".to_string(),
        "zh-CHT" => "zh-TW".to_string(),
        _ => code.to_string(),
    }
}
