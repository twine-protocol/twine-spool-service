use std::{fmt::Display, str::FromStr};
use worker::*;

use chrono::{NaiveDateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use scrypt::{
  Scrypt,
  password_hash::{
    PasswordHash,
    PasswordHasher,
    PasswordVerifier,
    SaltString,
  }
};
use worker::D1Database;
use crate::errors::ApiKeyValidationError;

const SALT_STR : &str = "7IvnC9XW2D9FQrdEA/srAQ";

#[derive(Debug, Clone)]
pub struct ApiKey(Vec<u8>);

impl FromStr for ApiKey {
  type Err = hex::FromHexError;

  fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
    let bytes = hex::decode(s)?;
    Ok(ApiKey(bytes))
  }
}

impl Display for ApiKey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let hex_str = hex::encode(&self.0);
    write!(f, "{}", hex_str)
  }
}

impl ApiKey {
  pub fn new(bytes: Vec<u8>) -> Self {
    Self(bytes)
  }

  pub fn generate() -> Self {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    Self(bytes.to_vec())
  }

  pub fn bytes(&self) -> &[u8] {
    &self.0
  }

  pub async fn validate(&self, db: &D1Database) -> std::result::Result<(), ApiKeyValidationError> {
    ApiKeyRecord::key_is_valid(db, self).await
  }
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiKeyRecord {
  pub id: i64,
  pub description: String,
  pub hashed_key: String,
  pub created_at: NaiveDateTime,
  pub last_used_at: NaiveDateTime,
  pub expires_at: Option<NaiveDateTime>,
}

impl ApiKeyRecord {
  pub fn new<S: Into<String>>(api_key: &ApiKey, description: S, expires_at: Option<NaiveDateTime>) -> Self {
    let bytes = api_key.bytes();
    let salt = SaltString::from_b64(SALT_STR).unwrap();
    let hashed_key = Scrypt.hash_password(&bytes, &salt).unwrap().to_string();

    Self {
      id: -1,
      description: description.into(),
      hashed_key,
      created_at: Utc::now().naive_utc(),
      last_used_at: Utc::now().naive_utc(),
      expires_at,
    }
  }

  pub async fn get(db: &D1Database, id: u64) -> std::result::Result<Option<ApiKeyRecord>, ApiKeyValidationError> {
    let query_str = "SELECT * FROM ApiKeys WHERE id = ? LIMIT 1";
    let query = query!(db, query_str, id)?;
    let record: Option<ApiKeyRecord> = query.first(None).await?;
    Ok(record)
  }

  pub async fn delete(db: &D1Database, id: u64) -> std::result::Result<(), ApiKeyValidationError> {
    let query_str = "DELETE FROM ApiKeys WHERE id = ?";
    let query = query!(db, query_str, id)?;
    query.run().await?;
    Ok(())
  }

  pub async fn get_all(db: &D1Database) -> std::result::Result<Vec<ApiKeyRecord>, ApiKeyValidationError> {
    let query_str = "SELECT * FROM ApiKeys";
    let query = query!(db, query_str);
    let result = query.all().await?;
    let records: Vec<ApiKeyRecord> = result.results()?;
    Ok(records)
  }

  pub fn validate(&self, api_key: &ApiKey) -> std::result::Result<(), ApiKeyValidationError> {
    // check if the key is expired
    if let Some(expires_at) = self.expires_at {
      if expires_at < Utc::now().naive_utc() {
        return Err(ApiKeyValidationError::ExpiredKey);
      }
    }

    let parsed_hash = PasswordHash::new(&self.hashed_key).unwrap();
    if Scrypt.verify_password(&api_key.bytes(), &parsed_hash).is_ok() {
      Ok(())
    } else {
      Err(ApiKeyValidationError::InvalidKey)
    }
  }

  pub async fn save(&mut self, db: &D1Database) -> std::result::Result<&mut Self, ApiKeyValidationError> {
    let query_str = r#"
    INSERT INTO ApiKeys (description, hashed_key, created_at, last_used_at, expires_at)
    VALUES (?, ?, ?, ?, ?)
    ON CONFLICT(hashed_key) DO UPDATE SET
      last_used_at = excluded.last_used_at;
    "#;
    let query = query!(db, query_str, self.description, self.hashed_key, self.created_at, self.last_used_at, self.expires_at)?;
    let meta = query.run().await?.meta()?.unwrap();
    if self.id == -1 {
      self.id = meta.last_row_id.unwrap_or(-1);
    }
    Ok(self)
  }

  pub async fn key_is_valid(db: &D1Database, api_key: &ApiKey) -> std::result::Result<(), ApiKeyValidationError> {
    let hashed = Scrypt.hash_password(&api_key.bytes(), &SaltString::from_b64(SALT_STR).unwrap()).unwrap().to_string();
    let query_str = "SELECT * FROM ApiKeys WHERE hashed_key = ? LIMIT 1";
    let query = query!(db, query_str, hashed)?;
    let record: Option<ApiKeyRecord> = query.first(None).await?;

    match record {
      Some(mut rec) => {
        rec.validate(api_key)?;
        rec.last_used_at = Utc::now().naive_utc();
        rec.save(db).await?;
        Ok(())
      }
      None => {
        Err(ApiKeyValidationError::InvalidKey)
      }
    }
  }
}