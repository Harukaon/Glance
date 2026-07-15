//! Built-in, key-free text translation engines ported from STranslate's
//! `*BuiltIn` plugins. None of these require the user to provide an API key;
//! they authenticate via baked-in client identifiers / signatures / tokens,
//! the same technique the Youdao image-translation endpoint already uses.
//!
//! Providers:
//! - Google   : `translate.googleapis.com` free `gtx` endpoint
//! - Microsoft: Edge auth token + `api-edge.cognitive.microsofttranslator.com`
//! - Transmart: Tencent `transmart.qq.com/api/imt` (hard-coded client_key)
//! - Yandex   : `translate.yandex.net` mobile endpoint (baked-in ucid)
//! - iCiba    : `dictionary.iciba.com/dictionary/fy/batch` (client/key + MD5 sign)

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use reqwest::Client;
use sha2::Sha256;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::models::TextTranslationResult;

type HmacSha256 = Hmac<Sha256>;

/// Matches C#'s `Uri.EscapeDataString`: escape everything except the unreserved
/// set A-Za-z0-9 and `-` `_` `.` `~`.
const ESCAPE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Microsoft "MSTranslatorAndroidApp" HMAC signing key (from GTranslate / STranslate).
const MS_PRIVATE_KEY: [u8; 64] = [
    0xa2, 0x29, 0x3a, 0x3d, 0xd0, 0xdd, 0x32, 0x73, 0x97, 0x7a, 0x64, 0xdb, 0xc2, 0xf3, 0x27, 0xf5,
    0xd7, 0xbf, 0x87, 0xd9, 0x45, 0x9d, 0xf0, 0x5a, 0x09, 0x66, 0xc6, 0x30, 0xc6, 0x6a, 0xaa, 0x84,
    0x9a, 0x41, 0xaa, 0x94, 0x3a, 0xa8, 0xd5, 0x1a, 0x6e, 0x4d, 0xaa, 0xc9, 0xa3, 0x70, 0x12, 0x35,
    0xc7, 0xeb, 0x12, 0xf6, 0xe8, 0x23, 0x07, 0x9e, 0x47, 0x10, 0x95, 0x91, 0x88, 0x55, 0xd8, 0x17,
];
const MS_ENDPOINT: &str = "api.cognitive.microsofttranslator.com";

const YANDEX_USER_AGENT: &str = "ru.yandex.translate/3.20.2024";
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36";

// iCiba batch-translate signing constants (from STranslate ICibaTranslateBuiltIn).
const ICIBA_PATH: &str = "/dictionary/fy/batch";
const ICIBA_CLIENT: &str = "6";
const ICIBA_KEY: &str = "1000006";
const ICIBA_SALT: &str = "7ece94d9f9c202b0d2ec557dg4r9bc";

pub struct BuiltinTranslateClient {
    /// Cached HTTP client keyed by its proxy URL ("" = no proxy). Rebuilt
    /// on demand whenever the effective proxy configuration changes.
    cache: RwLock<Option<(String, Client)>>,
}

impl BuiltinTranslateClient {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(None),
        }
    }

    /// Return an HTTP client configured for the given proxy URL (None = direct),
    /// reusing the cached one when the proxy setting is unchanged. The client
    /// has NO default User-Agent so each provider can set its own per request.
    async fn client(&self, proxy: Option<&str>) -> Client {
        let key = proxy.unwrap_or("").to_string();
        {
            let guard = self.cache.read().await;
            if let Some((cached_key, client)) = guard.as_ref() {
                if cached_key == &key {
                    return client.clone();
                }
            }
        }

        let mut builder = Client::builder();
        match proxy {
            Some(url) if !url.is_empty() => {
                if let Ok(p) = reqwest::Proxy::all(url) {
                    builder = builder.proxy(p);
                }
            }
            // Explicitly disable proxies (ignore env vars) when direct is chosen.
            _ => builder = builder.no_proxy(),
        }
        let client = builder.build().unwrap_or_else(|_| Client::new());
        *self.cache.write().await = Some((key, client.clone()));
        client
    }

    // ── Google (free gtx endpoint) ─────────────────────────────────────────
    pub async fn google(
        &self,
        text: &str,
        from: &str,
        to: &str,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        let sl = google_lang(from);
        let tl = google_lang(to);

        let resp = self
            .client(proxy)
            .await
            .get("https://translate.googleapis.com/translate_a/single")
            .header(USER_AGENT, BROWSER_UA)
            .query(&[
                ("client", "gtx"),
                ("sl", sl.as_str()),
                ("tl", tl.as_str()),
                ("dt", "t"),
                ("q", text),
            ])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Google translate request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Api(format!("Google translate http error: {e}")))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Api(format!("Google translate parse failed: {e}")))?;

        let mut translated = String::new();
        if let Some(segments) = body.get(0).and_then(|v| v.as_array()) {
            for seg in segments {
                if let Some(part) = seg.get(0).and_then(|v| v.as_str()) {
                    translated.push_str(part);
                }
            }
        }
        let detected = body
            .get(2)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| from.to_string());

        finalize(translated, detected, "Google")
    }

    // ── Microsoft (MSTranslatorAndroidApp signature, no token needed) ──────
    pub async fn microsoft(
        &self,
        text: &str,
        from: &str,
        to: &str,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        let from_ms = microsoft_lang(from);
        let to_ms = microsoft_lang(to);

        // Build the request path (without protocol); it is used both as the POST
        // URL and as the signed content, so the two must match exactly.
        let mut request_path = format!("{MS_ENDPOINT}/translate?api-version=3.0&to={to_ms}");
        if !from_ms.is_empty() && from_ms != "auto" {
            request_path.push_str(&format!("&from={from_ms}"));
        }

        let signature = ms_signature(&request_path);

        let resp = self
            .client(proxy)
            .await
            .post(format!("https://{request_path}"))
            .header("X-MT-Signature", signature)
            .header(USER_AGENT, BROWSER_UA)
            .json(&[serde_json::json!({ "Text": text })])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Microsoft translate request failed: {e}")))?;

        let body: serde_json::Value = resp
            .error_for_status()
            .map_err(|e| AppError::Api(format!("Microsoft translate http error: {e}")))?
            .json()
            .await
            .map_err(|e| AppError::Api(format!("Microsoft translate parse failed: {e}")))?;

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

        finalize(translated, detected, "Microsoft")
    }

    // ── Tencent Transmart ──────────────────────────────────────────────────
    pub async fn transmart(
        &self,
        text: &str,
        from: &str,
        to: &str,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        let source = transmart_lang(from);
        let target = transmart_lang(to);

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36",
        ));
        headers.insert(REFERER, HeaderValue::from_static("https://yi.qq.com/zh-CN/index"));

        let payload = serde_json::json!({
            "header": {
                "fn": "auto_translation_block",
                "client_key": "browser-chrome-110.0.0-Mac OS-df4bd4c5-a65d-44b2-a40f-42f34f3535f2-1677486696487"
            },
            "type": "plain",
            "model_category": "normal",
            "source": { "lang": source, "text_block": text },
            "target": { "lang": target }
        });

        let body: serde_json::Value = self
            .client(proxy)
            .await
            .post("https://transmart.qq.com/api/imt")
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Transmart request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Api(format!("Transmart http error: {e}")))?
            .json()
            .await
            .map_err(|e| AppError::Api(format!("Transmart parse failed: {e}")))?;

        // `auto_translation` may be a string or an array of blocks.
        let translated = match body.get("auto_translation") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };

        finalize(translated, from.to_string(), "Transmart")
    }

    // ── Yandex ─────────────────────────────────────────────────────────────
    pub async fn yandex(
        &self,
        text: &str,
        from: &str,
        to: &str,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        let src = yandex_lang(from);
        let tgt = yandex_lang(to);
        // Yandex rejects "auto-<tgt>"; for auto-detect send only the target code.
        let lang = if src == "auto" {
            tgt.clone()
        } else {
            format!("{src}-{tgt}")
        };
        let ucid = uuid::Uuid::new_v4().simple().to_string();

        let url = format!(
            "https://translate.yandex.net/api/v1/tr.json/translate?ucid={ucid}&srv=android&format=text"
        );

        let body: serde_json::Value = self
            .client(proxy)
            .await
            .post(&url)
            .header(USER_AGENT, YANDEX_USER_AGENT)
            .form(&[("text", text), ("lang", lang.as_str())])
            .send()
            .await
            .map_err(|e| AppError::Api(format!("Yandex request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Api(format!("Yandex http error: {e}")))?
            .json()
            .await
            .map_err(|e| AppError::Api(format!("Yandex parse failed: {e}")))?;

        let translated = body
            .get("text")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        finalize(translated, from.to_string(), "Yandex")
    }

    // ── iCiba (Kingsoft) batch translate ───────────────────────────────────
    pub async fn iciba(
        &self,
        text: &str,
        from: &str,
        to: &str,
        proxy: Option<&str>,
    ) -> AppResult<TextTranslationResult> {
        let from_ic = iciba_lang(from);
        let to_ic = iciba_lang(to);
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();

        // signature = md5(path + concat(values sorted by key) + salt)
        // sorted keys (Ordinal): client < key < timestamp
        let sign_src = format!(
            "{ICIBA_PATH}{ICIBA_CLIENT}{ICIBA_KEY}{timestamp}{ICIBA_SALT}"
        );
        let signature = format!("{:x}", md5::compute(sign_src.as_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static("https://www.iciba.com"));
        headers.insert(REFERER, HeaderValue::from_static("https://www.iciba.com/"));
        headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));

        let payload = serde_json::json!({
            "from": from_ic,
            "to": to_ic,
            "textList": [text],
        });

        let body: serde_json::Value = self
            .client(proxy)
            .await
            .post("https://dictionary.iciba.com/dictionary/fy/batch")
            .query(&[
                ("client", ICIBA_CLIENT),
                ("key", ICIBA_KEY),
                ("timestamp", timestamp.as_str()),
                ("signature", signature.as_str()),
            ])
            .headers(headers)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Api(format!("iCiba request failed: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Api(format!("iCiba http error: {e}")))?
            .json()
            .await
            .map_err(|e| AppError::Api(format!("iCiba parse failed: {e}")))?;

        let code_ok = match body.get("code") {
            Some(serde_json::Value::Number(n)) => n.as_i64() == Some(1),
            Some(serde_json::Value::String(s)) => s == "1",
            _ => false,
        };
        if !code_ok {
            return Err(AppError::Api(format!("iCiba error: {body}")));
        }

        let mut lines = Vec::new();
        if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
            for item in arr {
                match item {
                    serde_json::Value::String(s) if !s.is_empty() => lines.push(s.clone()),
                    serde_json::Value::Object(_) => {
                        if let Some(out) = item.get("out").and_then(|v| v.as_str()) {
                            if !out.is_empty() {
                                lines.push(out.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        finalize(lines.join("\n"), from.to_string(), "iCiba")
    }
}

fn finalize(
    translated: String,
    detected: String,
    engine: &str,
) -> AppResult<TextTranslationResult> {
    if translated.trim().is_empty() {
        return Err(AppError::Api(format!("{engine} returned empty result")));
    }
    Ok(TextTranslationResult {
        translated_text: translated,
        from_lang_detected: detected,
        alternatives: Vec::new(),
    })
}

/// Build the `X-MT-Signature` header value for the Microsoft "android app"
/// free endpoint. `request_path` is the URL without the `https://` prefix.
fn ms_signature(request_path: &str) -> String {
    let guid = uuid::Uuid::new_v4().simple().to_string(); // 32 lowercase hex
    let escaped_url = utf8_percent_encode(request_path, ESCAPE_SET).to_string();
    // e.g. "Sun, 12 Jul 2026 15:35:39GMT"
    let date_time = format!("{}GMT", Utc::now().format("%a, %d %b %Y %H:%M:%S"));

    let raw = format!("MSTranslatorAndroidApp{escaped_url}{date_time}{guid}").to_lowercase();

    let mut mac = HmacSha256::new_from_slice(&MS_PRIVATE_KEY).expect("HMAC key of valid length");
    mac.update(raw.as_bytes());
    let hash = mac.finalize().into_bytes();
    let sig_b64 = BASE64_STANDARD.encode(hash);

    format!("MSTranslatorAndroidApp::{sig_b64}::{date_time}::{guid}")
}

// ── Per-provider language code mapping (from Glance internal codes) ─────────

fn google_lang(code: &str) -> String {
    match code {
        "auto" => "auto",
        "zh-CHS" => "zh-CN",
        "zh-CHT" => "zh-TW",
        other => other,
    }
    .to_string()
}

fn microsoft_lang(code: &str) -> String {
    match code {
        "auto" => "auto",
        "zh-CHS" => "zh-Hans",
        "zh-CHT" => "zh-Hant",
        other => other,
    }
    .to_string()
}

fn transmart_lang(code: &str) -> String {
    match code {
        "auto" => "auto",
        "zh-CHS" => "zh",
        "zh-CHT" => "zh-TW",
        other => other,
    }
    .to_string()
}

fn yandex_lang(code: &str) -> String {
    match code {
        "auto" => "auto",
        "zh-CHS" | "zh-CHT" => "zh",
        other => other,
    }
    .to_string()
}

fn iciba_lang(code: &str) -> String {
    match code {
        "auto" => "auto",
        "zh-CHS" => "zh",
        "zh-CHT" => "cht",
        other => other,
    }
    .to_string()
}

/// Read the Windows system proxy (as configured by proxy apps / Internet
/// Settings). Returns an `http://host:port` URL usable by reqwest, or None.
#[cfg(target_os = "windows")]
pub fn system_proxy_url() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enable: u32 = key.get_value("ProxyEnable").ok()?;
    if enable == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    if server.is_empty() {
        return None;
    }

    // "ProxyServer" is either "host:port" or
    // "http=host:port;https=host:port;socks=host:port".
    if server.contains('=') {
        let mut http_fallback = None;
        for part in server.split(';') {
            let mut kv = part.splitn(2, '=');
            let scheme = kv.next().unwrap_or("").trim();
            let addr = kv.next().unwrap_or("").trim();
            if addr.is_empty() {
                continue;
            }
            if scheme.eq_ignore_ascii_case("https") {
                return Some(format!("http://{addr}"));
            }
            if scheme.eq_ignore_ascii_case("http") {
                http_fallback = Some(format!("http://{addr}"));
            }
        }
        http_fallback
    } else {
        Some(format!("http://{server}"))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn system_proxy_url() -> Option<String> {
    None
}

/// Normalize a user-supplied custom proxy string into a URL reqwest accepts.
/// Bare `host:port` is assumed to be an HTTP proxy.
pub fn normalize_proxy(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        Some(trimmed.to_string())
    } else {
        Some(format!("http://{trimmed}"))
    }
}
