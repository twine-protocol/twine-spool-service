use axum::extract::{State, Path, Json};
use axum::routing::{get, post, delete};
use axum::Router;

pub mod api_keys {
  use chrono::Utc;
  use serde::Deserialize;

  use super::*;
  use crate::{access_control::{ApiKey, ApiKeyRecord}, errors::ApiError, Env};

  pub fn router() -> Router<Env> {
    Router::new()
      .route("/apikeys", get(list_keys))
      .route("/apikeys/{:id}", get(get_key))
      .route("/apikeys", post(create_key))
      .route("/apikeys/{:id}", delete(delete_key))
  }

  #[worker::send]
  pub async fn list_keys(
    State(env): State<Env>,
  ) -> std::result::Result<Json<Vec<ApiKeyRecord>>, ApiError> {
    let db = env.d1("DB")?;
    Ok(Json(ApiKeyRecord::get_all(&db).await?))
  }

  #[worker::send]
  pub async fn get_key(
    State(env): State<Env>,
    Path(id): Path<u64>,
  ) -> std::result::Result<Json<ApiKeyRecord>, ApiError> {
    let db = env.d1("DB")?;
    let record = ApiKeyRecord::get(&db, id).await?;
    if record.is_none() {
      return Err(ApiError::NotFound);
    }
    Ok(Json(record.unwrap()))
  }

  #[derive(Debug, Clone, Deserialize)]
  struct KeyPostData {
    pub key: String,
    pub description: String,
    pub expires_at: Option<chrono::DateTime<Utc>>
  }

  #[worker::send]
  pub async fn create_key(
    State(env): State<Env>,
    Json(payload): Json<KeyPostData>
  ) -> std::result::Result<Json<ApiKeyRecord>, ApiError> {
    use std::str::FromStr;
    let db = env.d1("DB")?;
    let key = ApiKey::from_str(&payload.key).map_err(|e| ApiError::BadRequestData(e.to_string()))?;
    let mut record = ApiKeyRecord::new(
      &key,
      payload.description,
      payload.expires_at.map(|d| d.naive_utc())
    );
    record.save(&db).await?;
    log::info!("New API Key created: {}", record.description);
    Ok(Json(record))
  }

  #[worker::send]
  pub async fn delete_key(
    State(env): State<Env>,
    Path(id): Path<u64>,
  ) -> std::result::Result<(), ApiError> {
    let db = env.d1("DB")?;
    ApiKeyRecord::delete(&db, id).await?;
    log::info!("API Key deleted: id {}", id);
    Ok(())
  }

}