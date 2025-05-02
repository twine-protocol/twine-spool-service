use axum::response::{IntoResponse, Response};
use http::StatusCode;
use twine_protocol::{prelude::{ResolutionError, StoreError}, twine_lib::errors::{ConversionError, VerificationError}};
use worker::{console_error, console_log};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
  #[error("Corrupted cid: {0}")]
  Corrupted(#[from] twine_protocol::twine_lib::cid::Error),
  #[error("Bad Data: {0}")]
  BadRequestData(String),
  #[error("Server error: {0}")]
  ServerError(#[from] worker::Error),
  #[error("Verification error: {0}")]
  VerificationError(#[from] VerificationError),
  #[error("Resolution error: {0}")]
  ResolutionError(#[from] ResolutionError),
  #[error("Store Error: {0}")]
  StoreError(#[from] StoreError),
  #[error("Invalid query: {0}")]
  InvalidQuery(String),
  #[error("Api key error: {0}")]
  ApiKeyError(#[from] ApiKeyValidationError),
  #[error("Not found")]
  NotFound,
  #[error("Unauthorized")]
  Unauthorized,
}

impl From<ConversionError> for ApiError {
  fn from(e: ConversionError) -> Self {
    ApiError::BadRequestData(e.to_string())
  }
}

impl ApiError {
  pub fn to_response(&self) -> Result<worker::Response, worker::Error> {
    let (text, status) = self.response_info();
    if status == 500 {
      log::error!("API Error: {}", text);
    } else {
      log::debug!("API response (code: {}): {}", status, text);
    }
    worker::Response::error(text, status)
  }

  pub fn response_info(&self) -> (String, u16) {
    match self {
      ApiError::ServerError(e) => (e.to_string(), 500),
      ApiError::VerificationError(e) => (e.to_string(), 500),
      ApiError::InvalidQuery(e) => (e.to_string(), 400),
      ApiError::NotFound => ("Not found".into(), 404),
      ApiError::Corrupted(e) => (e.to_string(), 500),
      ApiError::BadRequestData(e) => (e.to_string(), 400),
      ApiError::Unauthorized => ("Unauthorized".into(), 401),
      ApiError::ResolutionError(e) => match e {
        ResolutionError::NotFound => ("Not found".into(), 404),
        _ => (e.to_string(), 500),
      },
      ApiError::StoreError(e) => match e {
        StoreError::Fetching(e) => match e {
          ResolutionError::NotFound => ("Not found".into(), 404),
          _ => (e.to_string(), 500),
        },
        _ => (e.to_string(), 500),
      },
      ApiError::ApiKeyError(e) => match e {
        ApiKeyValidationError::InvalidKey => ("Invalid API key".into(), 401),
        ApiKeyValidationError::ExpiredKey => ("Expired API key".into(), 401),
        ApiKeyValidationError::DatabaseError(_) => {
          ("Server error".into(), 500)
        }
      },
    }
  }
}

impl IntoResponse for ApiError {
  fn into_response(self) -> Response<axum::body::Body> {
    let (msg, code) = self.response_info();
    let code = StatusCode::from_u16(code).unwrap();
    if code == 500 {
      log::error!("API Error: {}", msg);
    } else {
      log::debug!("API response (code: {}): {}", code, msg);
    }
    (code, msg).into_response()
  }
}


#[derive(Debug, thiserror::Error)]
pub enum ApiKeyValidationError {
  #[error("Invalid api key")]
  InvalidKey,
  #[error("Expired api key")]
  ExpiredKey,
  #[error("Error reading database")]
  DatabaseError(#[from] worker::Error),
}

impl IntoResponse for ApiKeyValidationError {
  fn into_response(self) -> Response<axum::body::Body> {
    match self {
      ApiKeyValidationError::InvalidKey => {
        (StatusCode::UNAUTHORIZED, "Invalid API key").into_response()
      }
      ApiKeyValidationError::ExpiredKey => {
        (StatusCode::UNAUTHORIZED, "Expired API key").into_response()
      }
      ApiKeyValidationError::DatabaseError(_) => {
        (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response()
      }
    }
  }
}