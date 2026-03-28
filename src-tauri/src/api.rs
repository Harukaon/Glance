use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::{
    BoundingBox, OverlayRegion, RegionLine, SelectionPayload, TranslationHistoryItem,
    TranslationPair, TranslationResponse, TranslatorSettings,
};

const IMAGE_TRANSLATE_SECRET: &str = "VPaHE3kX_vl4BhgYiu2n";

#[derive(Debug, Clone)]
pub struct YoudaoClient {
    http: Arc<Client>,
}

impl YoudaoClient {
    pub fn new(http: Arc<Client>) -> Self {
        Self { http }
    }

    pub async fn translate_image_bytes(
        &self,
        bytes: Vec<u8>,
        file_name: String,
        mime_type: String,
        from_lang: String,
        to_lang: String,
        selection: SelectionPayload,
        salt: Option<String>,
        settings: &TranslatorSettings,
    ) -> AppResult<TranslationResponse> {
        let salt = salt.unwrap_or_else(random_salt_string);
        let sign = build_upload_sign(settings.clientele.as_str(), &bytes, salt.as_str());

        let form = Form::new()
            .part(
                "multipartFile",
                Part::bytes(bytes)
                    .file_name(file_name)
                    .mime_str(mime_type.as_str())
                    .map_err(AppError::Mime)?,
            )
            .text("clientele", settings.clientele.clone())
            .text("salt", salt)
            .text("sign", sign)
            .text("from", from_lang)
            .text("to", to_lang)
            .text("isSaveHistory", "true")
            .text("isSyncSaveHistory", "true")
            .text("funDesc", "photo_translate");

        let payload = self
            .http
            .post("https://ocrtran.youdao.com/ocr/imgtranocr")
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json::<RawTranslationResponse>()
            .await?;

        payload.into_response(selection)
    }
}

#[derive(Debug, Deserialize)]
struct RawRegion {
    #[serde(default)]
    #[serde(rename = "boundingBox")]
    bounding_box: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    #[serde(rename = "tranContent")]
    translated: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    lines: Vec<RawLine>,
}

#[derive(Debug, Deserialize)]
struct RawLine {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct RawTranslationResponse {
    #[serde(default)]
    #[serde(rename = "errorCode")]
    error_code: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    #[serde(rename = "lanFrom")]
    lan_from: String,
    #[serde(default)]
    #[serde(rename = "lanTo")]
    lan_to: String,
    #[serde(default)]
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(default)]
    #[serde(rename = "resRegions")]
    regions: Vec<RawRegion>,
    #[serde(flatten)]
    extra: Value,
}

impl RawTranslationResponse {
    fn into_response(self, selection: SelectionPayload) -> AppResult<TranslationResponse> {
        if self.error_code != "0" {
            return Err(AppError::Api(format!(
                "youdao errorCode={}",
                self.error_code
            )));
        }

        let mut overlay_regions = Vec::new();
        let mut pairs = Vec::new();
        for region in self.regions {
            let bounds = parse_bounding_box(&region.bounding_box)?;
            let lines = region
                .lines
                .into_iter()
                .map(|line| RegionLine { text: line.text })
                .collect::<Vec<_>>();

            if !region.context.is_empty() || !region.translated.is_empty() {
                pairs.push(TranslationPair {
                    source: region.context.clone(),
                    target: region.translated.clone(),
                });
            }

            overlay_regions.push(OverlayRegion {
                rect: BoundingBox {
                    x: selection.x + bounds.x,
                    y: selection.y + bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                },
                local_rect: bounds,
                source: region.context,
                translated: region.translated,
                color: if region.color.is_empty() {
                    "default".to_string()
                } else {
                    region.color
                },
                lines,
            });
        }

        let history_item = TranslationHistoryItem::from_response(
            selection,
            self.request_id.clone(),
            self.lan_from.clone(),
            self.lan_to.clone(),
            pairs.clone(),
        );

        Ok(TranslationResponse {
            request_id: self.request_id,
            lan_from: self.lan_from,
            lan_to: self.lan_to,
            rendered_image_base64: self.image,
            regions: overlay_regions,
            pairs,
            raw: self.extra,
            history_item,
        })
    }
}

fn parse_bounding_box(raw: &str) -> AppResult<BoundingBox> {
    let parts = raw
        .split(',')
        .map(|item| item.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Parse(format!("invalid bounding box: {raw}")))?;
    if parts.len() != 4 {
        return Err(AppError::Parse(format!("invalid bounding box len: {raw}")));
    }

    Ok(BoundingBox {
        x: parts[0],
        y: parts[1],
        width: parts[2],
        height: parts[3],
    })
}

fn build_upload_sign(clientele: &str, bytes: &[u8], salt: &str) -> String {
    let image_b64 = BASE64_STANDARD.encode(bytes);
    let digest_source = format!(
        "{}{}{}",
        &image_b64[..10],
        image_b64.len(),
        &image_b64[image_b64.len() - 10..]
    );
    md5_hex(&format!(
        "{clientele}{digest_source}{salt}{IMAGE_TRANSLATE_SECRET}"
    ))
}

fn md5_hex(value: &str) -> String {
    format!("{:x}", md5::compute(value.as_bytes()))
}

fn random_salt_string() -> String {
    let random = uuid::Uuid::new_v4().as_u128();
    let fraction = (random % 1_000_000_000_000_000_000u128) as f64 / 1_000_000_000_000_000_000f64;
    format!("{fraction}")
}
