use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatorSettings {
    pub from_lang: String,
    pub to_lang: String,
    pub clientele: String,
    pub client: String,
    pub vendor: String,
    pub input_channel: String,
    pub app_version: String,
    pub ab_test: String,
    pub model: String,
    pub screen: String,
    pub os_version: String,
    pub network: String,
    pub mid: String,
    pub product: String,
    pub yduuid: String,
    pub overlay_opacity: f32,
    pub overlay_font_scale: f32,
    pub close_on_outside_click: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
}

impl Default for TranslatorSettings {
    fn default() -> Self {
        Self {
            from_lang: "auto".to_string(),
            to_lang: "zh-CHS".to_string(),
            clientele: "deskdict".to_string(),
            client: "deskdict".to_string(),
            vendor: "fanyiweb_navigation".to_string(),
            input_channel: "YoudaoDict_fanyiweb_navigation".to_string(),
            app_version: "10.3.0.0".to_string(),
            ab_test: "2".to_string(),
            model: "default".to_string(),
            screen: "1920*1080".to_string(),
            os_version: "10.0".to_string(),
            network: "none".to_string(),
            mid: "windows10.0".to_string(),
            product: "deskdict".to_string(),
            yduuid: uuid::Uuid::new_v4().to_string().replace("-", "")[..17].to_string(),
            overlay_opacity: 0.92,
            overlay_font_scale: 1.0,
            close_on_outside_click: true,
            autostart: false,
            hotkey: default_hotkey(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPayload {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub monitor_id: String,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    #[serde(default = "default_monitor_scale_factor")]
    pub monitor_scale_factor: f64,
}

fn default_monitor_scale_factor() -> f64 {
    1.0
}

fn default_hotkey() -> String {
    "CommandOrControl+Shift+X".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationResponse {
    pub request_id: String,
    pub lan_from: String,
    pub lan_to: String,
    pub rendered_image_base64: String,
    pub regions: Vec<OverlayRegion>,
    pub pairs: Vec<TranslationPair>,
    pub raw: Value,
    pub history_item: TranslationHistoryItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPayload {
    pub request_id: String,
    pub lan_from: String,
    pub lan_to: String,
    pub selection: SelectionPayload,
    pub overlay_opacity: f32,
    pub overlay_font_scale: f32,
    pub close_on_outside_click: bool,
    pub rendered_image_base64: String,
    pub regions: Vec<OverlayRegion>,
    pub pairs: Vec<TranslationPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayRegion {
    pub rect: BoundingBox,
    pub local_rect: BoundingBox,
    pub source: String,
    pub translated: String,
    pub color: String,
    pub lines: Vec<RegionLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionLine {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPair {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationHistoryItem {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub from_lang: String,
    pub to_lang: String,
    pub selection: SelectionPayload,
    pub pairs: Vec<TranslationPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextTranslationResult {
    pub translated_text: String,
    pub from_lang_detected: String,
}

impl TranslationHistoryItem {
    pub fn from_response(
        selection: SelectionPayload,
        request_id: String,
        from_lang: String,
        to_lang: String,
        pairs: Vec<TranslationPair>,
    ) -> Self {
        Self {
            id: request_id,
            created_at: Utc::now(),
            from_lang,
            to_lang,
            selection,
            pairs,
        }
    }
}
