use tauri::ipc::InvokeError;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("api error: {0}")]
    Api(String),
    #[error("base64 error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("capture error: {0}")]
    Capture(String),
    #[error("http error {0}: {1}")]
    HttpStatus(u16, String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("mime error: {0}")]
    Mime(reqwest::Error),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("request timed out: {0}")]
    Timeout(String),
}

impl AppError {
    /// Whether retrying the same request has a reasonable chance of succeeding.
    /// Used by the retry/fallback logic in the translation engine.
    pub fn is_transient(&self) -> bool {
        match self {
            // Network failures include DNS, connect and request timeouts.
            AppError::Network(e) => {
                e.is_connect() || e.is_timeout() || e.is_request() || e.is_body()
            }
            AppError::Timeout(_) => true,
            // 408 (timeout), 429 (rate limited), 5xx (server errors).
            AppError::HttpStatus(code, _) => {
                *code == 408 || *code == 429 || (500..600).contains(code)
            }
            _ => false,
        }
    }
}

impl From<AppError> for InvokeError {
    fn from(value: AppError) -> Self {
        InvokeError::from(value.to_string())
    }
}
