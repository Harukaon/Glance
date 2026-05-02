use std::sync::Arc;

use reqwest::Client;
use reqwest::header::{HeaderMap, ORIGIN, REFERER};
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::models::TextTranslationResult;

const CN_HOST: &str = "https://cn.bing.com";
const WWW_HOST: &str = "https://www.bing.com";

pub struct BingTranslateClient {
    http: Arc<Client>,
    token: Arc<RwLock<Option<BingToken>>>,
}

#[derive(Clone)]
struct BingToken {
    value: String,
    key: String,
    ig: String,
    iid: String,
    host: String, // "cn" or "www"
}

impl BingTranslateClient {
    pub fn new(http: Arc<Client>) -> Self {
        Self {
            http,
            token: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> AppResult<TextTranslationResult> {
        let token = self.get_or_refresh_token().await?;

        let from_bing = map_lang_code_bing(from);
        let to_bing = map_lang_code_bing(to);
        let host = &token.host;

        let mut headers = HeaderMap::new();
        headers.insert(REFERER, format!("{host}/translator").parse().unwrap());
        headers.insert(ORIGIN, host.parse().unwrap());

        let url = format!("{host}/ttranslatev3");

        let resp = self
            .http
            .post(&url)
            .query(&[
                ("isVertical", "1"),
                ("IG", &token.ig),
                ("IID", &token.iid),
            ])
            .headers(headers.clone())
            .form(&[
                ("fromLang", from_bing.as_str()),
                ("to", to_bing.as_str()),
                ("text", text),
                ("token", &token.value),
                ("key", &token.key),
            ])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Bing translate request failed: {e}")))?;

        let status = resp.status();

        // cn.bing.com 301 -> www.bing.com means cn is unavailable, fallback
        if status.as_u16() == 301 || status.as_u16() == 302 {
            *self.token.write().await = None;
            return self.translate_fallback(text, from, to).await;
        }

        if status.as_u16() == 429 {
            *self.token.write().await = None;
            return Err(AppError::Api("Bing translate rate limited, token reset".into()));
        }

        let resp_text = resp.text().await.map_err(|e| {
            AppError::Api(format!("Bing translate read body failed: {e}"))
        })?;

        let body: serde_json::Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::Api(format!("Bing translate parse failed: {e}")))?;

        if body.get("ShowCaptcha").is_some()
            || body
                .get("statusCode")
                .and_then(|v| v.as_u64())
                .map_or(false, |c| c >= 400)
        {
            *self.token.write().await = None;
            return Err(AppError::Api("Bing translate auth failed, token reset".into()));
        }

        let translated = body
            .pointer("/0/translations/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let detected = body
            .pointer("/0/detectedLanguage/language")
            .and_then(|v| v.as_str())
            .unwrap_or(from)
            .to_string();

        if translated.is_empty() {
            *self.token.write().await = None;
            return Err(AppError::Api(
                "Bing translate returned empty result, token reset".into(),
            ));
        }

        Ok(TextTranslationResult {
            translated_text: translated,
            from_lang_detected: normalize_bing_lang(&detected),
            alternatives: Vec::new(),
        })
    }

    async fn translate_fallback(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> AppResult<TextTranslationResult> {
        let token = self.get_or_refresh_token_with_host(WWW_HOST).await?;

        let from_bing = map_lang_code_bing(from);
        let to_bing = map_lang_code_bing(to);

        let mut headers = HeaderMap::new();
        headers.insert(REFERER, format!("{WWW_HOST}/translator").parse().unwrap());
        headers.insert(ORIGIN, WWW_HOST.parse().unwrap());

        let resp = self
            .http
            .post(format!("{WWW_HOST}/ttranslatev3"))
            .query(&[
                ("isVertical", "1"),
                ("IG", &token.ig),
                ("IID", &token.iid),
            ])
            .headers(headers)
            .form(&[
                ("fromLang", from_bing.as_str()),
                ("to", to_bing.as_str()),
                ("text", text),
                ("token", &token.value),
                ("key", &token.key),
            ])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Bing translate (www) request failed: {e}")))?;

        let status = resp.status();
        if status.as_u16() == 429 {
            *self.token.write().await = None;
            return Err(AppError::Api("Bing translate rate limited".into()));
        }

        let resp_text = resp.text().await.map_err(|e| {
            AppError::Api(format!("Bing translate (www) read body failed: {e}"))
        })?;

        let body: serde_json::Value = serde_json::from_str(&resp_text)
            .map_err(|e| AppError::Api(format!("Bing translate (www) parse failed: {e}")))?;

        let translated = body
            .pointer("/0/translations/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let detected = body
            .pointer("/0/detectedLanguage/language")
            .and_then(|v| v.as_str())
            .unwrap_or(from)
            .to_string();

        if translated.is_empty() {
            *self.token.write().await = None;
            return Err(AppError::Api("Bing translate (www) returned empty result, token reset".into()));
        }

        Ok(TextTranslationResult {
            translated_text: translated,
            from_lang_detected: normalize_bing_lang(&detected),
            alternatives: Vec::new(),
        })
    }

    async fn get_or_refresh_token(&self) -> AppResult<BingToken> {
        {
            let guard = self.token.read().await;
            if let Some(ref t) = *guard {
                return Ok(t.clone());
            }
        }

        // Try cn first
        match self.fetch_token(CN_HOST).await {
            Ok(token) => {
                *self.token.write().await = Some(token.clone());
                Ok(token)
            }
            Err(_) => {
                // Fallback to www
                let token = self.fetch_token(WWW_HOST).await?;
                *self.token.write().await = Some(token.clone());
                Ok(token)
            }
        }
    }

    async fn get_or_refresh_token_with_host(&self, host: &str) -> AppResult<BingToken> {
        let token = self.fetch_token(host).await?;
        *self.token.write().await = Some(token.clone());
        Ok(token)
    }

    async fn fetch_token(&self, host: &str) -> AppResult<BingToken> {
        let resp = self
            .http
            .get(format!("{host}/translator"))
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Bing token fetch ({host}) failed: {e}")))?;

        // If redirected, the host is unavailable
        if resp.status().as_u16() == 301 || resp.status().as_u16() == 302 {
            return Err(AppError::Api(format!("Bing {host} redirected, unavailable")));
        }

        let html = resp
            .text()
            .await
            .map_err(|e| AppError::Api(format!("Bing token read ({host}) failed: {e}")))?;

        let (key, value) = extract_abuse_prevention_token(&html)?;
        let ig = extract_ig(&html)?;
        let iid = extract_iid(&html).unwrap_or_else(|| "translator.5023".to_string());

        Ok(BingToken {
            value,
            key,
            ig,
            iid,
            host: host.to_string(),
        })
    }
}

fn extract_abuse_prevention_token(html: &str) -> AppResult<(String, String)> {
    let prefix = "params_AbusePreventionHelper = [";
    let start = html
        .find(prefix)
        .ok_or_else(|| AppError::Api("Bing: params_AbusePreventionHelper not found".into()))?;
    let rest = &html[start + prefix.len()..];
    let end = rest
        .find(']')
        .ok_or_else(|| AppError::Api("Bing: params_AbusePreventionHelper end not found".into()))?;
    let arr_str = &rest[..end];

    let parts: Vec<&str> = arr_str.splitn(3, ',').collect();
    if parts.len() < 2 {
        return Err(AppError::Api(
            "Bing: abuse prevention token array too short".into(),
        ));
    }

    let key = parts[0].trim().to_string();
    let value = parts[1].trim().trim_matches('"').to_string();

    if value.is_empty() || key.is_empty() {
        return Err(AppError::Api(
            "Bing: abuse prevention token or key is empty".into(),
        ));
    }

    Ok((key, value))
}

fn extract_ig(html: &str) -> AppResult<String> {
    let prefix = "IG:\"";
    let start = html
        .find(prefix)
        .ok_or_else(|| AppError::Api("Bing IG not found".into()))?;
    let rest = &html[start + prefix.len()..];
    let end = rest.find('"').unwrap_or(rest.len());
    Ok(rest[..end].to_string())
}

fn extract_iid(html: &str) -> Option<String> {
    let start = html.find("data-iid=\"")?;
    let rest = &html[start + 10..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn map_lang_code_bing(code: &str) -> String {
    match code {
        "zh-CHS" => "zh-Hans".to_string(),
        "zh-CHT" => "zh-Hant".to_string(),
        "auto" => "auto-detect".to_string(),
        _ => code.to_string(),
    }
}

fn normalize_bing_lang(code: &str) -> String {
    match code {
        "zh-Hans" => "zh-CHS".to_string(),
        "zh-Hant" => "zh-CHT".to_string(),
        "auto-detect" => "auto".to_string(),
        other => other.to_lowercase(),
    }
}