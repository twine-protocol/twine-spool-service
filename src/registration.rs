use super::*;
use serde::{Deserialize, Serialize};
use serde_email::Email;
use twine::twine_core::twine::Tagged;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct RegistrationRequest {
  pub email: Email,
  #[serde(with = "crate::dag_json")]
  pub strand: Tagged<Strand>,
}

impl From<RegistrationRequest> for RegistrationRecord {
  fn from(req: RegistrationRequest) -> Self {
    RegistrationRecord::new(req.email, req.strand.unpack())
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RegistrationStatus {
  Pending,
  Approved,
  Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRecord {
  #[serde(with = "uuid::serde::simple")]
  pub uuid: Uuid,
  pub email: Email,
  #[serde(with = "serde_bytes")]
  pub strand_cid: Vec<u8>,
  #[serde(with = "serde_bytes")]
  pub strand: Vec<u8>,
  pub status: RegistrationStatus,
}

impl RegistrationRecord {
  pub fn new(email: Email, strand: Strand) -> Self {
    RegistrationRecord {
      uuid: Uuid::new_v4(),
      email,
      strand_cid: strand.cid().to_bytes(),
      strand: strand.bytes().to_vec(),
      status: RegistrationStatus::Pending,
    }
  }

  pub fn new_preapproved(email: Email, strand_cid: Cid) -> Self {
    RegistrationRecord {
      uuid: Uuid::new_v4(),
      email,
      strand_cid: strand_cid.to_bytes(),
      strand: Vec::new(),
      status: RegistrationStatus::Approved,
    }
  }

  pub async fn save(&self, db: &D1Database) -> Result<()> {
    let query = query!(
      db,
      "INSERT INTO registrations (uuid, email, status, strand_cid, strand)
      VALUES ($1, $2, $3, $4, $5)",
      self.uuid.as_simple(),
      self.email,
      self.status,
      self.strand_cid,
      self.strand,
    )?;

    query.run().await?;
    Ok(())
  }

  pub async fn set_status(&mut self, db: &D1Database, status: RegistrationStatus) -> Result<()> {
    let query = query!(
      db,
      "UPDATE registrations SET status = $1 WHERE uuid = $2",
      status,
      self.uuid.as_simple(),
    )?;

    query.run().await?;
    self.status = status;

    Ok(())
  }

  pub async fn fetch(db: &D1Database, uuid: Uuid) -> Result<Option<Self>> {
    let query = query!(
      db,
      "SELECT * FROM registrations WHERE uuid = $1",
      uuid.as_simple(),
    )?;

    let result = query.first::<RegistrationRecord>(None).await?;
    Ok(result)
  }

  pub async fn check_approved(db: &D1Database, strand: &Strand) -> Result<Option<Self>> {
    let query = query!(
      db,
      "SELECT * FROM registrations WHERE strand_cid = $1 AND status = $2",
      strand.cid().to_bytes(),
      RegistrationStatus::Approved,
    )?;

    let result = query.first::<RegistrationRecord>(None).await?;
    Ok(result)
  }
}

