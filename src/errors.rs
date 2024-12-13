use twine::twine_core::errors::{ConversionError, VerificationError};
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
  #[error("Invalid query: {0}")]
  InvalidQuery(#[from] ConversionError),
  #[error("Not found")]
  NotFound,
  #[error("Unauthorized")]
  Unauthorized,
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
    }
  }
}