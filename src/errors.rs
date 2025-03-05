use twine::{prelude::{ResolutionError, StoreError}, twine_core::errors::{ConversionError, VerificationError}};
use worker::console_log;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
  #[error("Corrupted cid: {0}")]
  Corrupted(#[from] twine::twine_core::cid::Error),
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
    console_log!("ApiError: {:?}", self);
    match self {
      ApiError::ServerError(e) => worker::Response::error(e.to_string(), 500),
      ApiError::VerificationError(e) => worker::Response::error(e.to_string(), 500),
      ApiError::InvalidQuery(e) => worker::Response::error(e.to_string(), 400),
      ApiError::NotFound => worker::Response::error("Not found", 404),
      ApiError::Corrupted(e) => worker::Response::error(e.to_string(), 500),
      ApiError::BadRequestData(e) => worker::Response::error(e.to_string(), 400),
      ApiError::Unauthorized => worker::Response::error("Unauthorized", 401),
      ApiError::ResolutionError(e) => match e {
        ResolutionError::NotFound => worker::Response::error("Not found", 404),
        _ => worker::Response::error(e.to_string(), 500),
      },
      ApiError::StoreError(e) => match e {
        StoreError::Fetching(e) => match e {
          ResolutionError::NotFound => worker::Response::error("Not found", 404),
          _ => worker::Response::error(e.to_string(), 500),
        },
        _ => worker::Response::error(e.to_string(), 500),
      },
    }
  }
}