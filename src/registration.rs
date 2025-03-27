use super::*;
use serde::{Deserialize, Serialize};
use serde_email::Email;
use twine_protocol::twine_lib::twine::Tagged;
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

#[derive(Debug, Clone, Deserialize)]
pub struct RegistrationRecord {
  pub uuid: String,
  pub email: Email,
  pub strand_cid: Cid,
  #[serde(with = "serde_bytes")]
  pub strand: Vec<u8>,
  pub status: RegistrationStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistrationRecordJson {
  pub uuid: String,
  pub email: Email,
  #[serde(with = "crate::dag_json")]
  pub strand_cid: Cid,
  #[serde(with = "crate::dag_json")]
  pub strand: Tagged<Strand>,
  pub status: RegistrationStatus,
}

impl TryFrom<RegistrationRecord> for RegistrationRecordJson {
  type Error = VerificationError;
  fn try_from(value: RegistrationRecord) -> std::result::Result<Self, Self::Error> {
    let strand = Strand::from_block(value.strand_cid, value.strand)?;
    Ok(RegistrationRecordJson {
      uuid: value.uuid,
      email: value.email,
      strand_cid: value.strand_cid,
      strand: Tagged::new(strand),
      status: value.status,
    })
  }
}

impl RegistrationRecord {
  pub fn new(email: Email, strand: Strand) -> Self {
    RegistrationRecord {
      uuid: Uuid::new_v4().to_string(),
      email,
      strand_cid: strand.cid(),
      strand: strand.bytes().to_vec(),
      status: RegistrationStatus::Pending,
    }
  }

  pub fn new_preapproved(email: Email, strand_cid: Cid, strand: Strand) -> Self {
    RegistrationRecord {
      uuid: Uuid::new_v4().to_string(),
      email,
      strand_cid,
      strand: strand.bytes().to_vec(),
      status: RegistrationStatus::Approved,
    }
  }

  pub async fn save(&self, db: &D1Database) -> Result<()> {
    let query = query!(
      db,
      "INSERT INTO registrations (uuid, email, status, strand_cid, strand)
      VALUES ($1, $2, $3, $4, $5)",
      self.uuid,
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
      self.uuid,
    )?;

    query.run().await?;
    self.status = status;

    Ok(())
  }

  pub async fn fetch(db: &D1Database, uuid: Uuid) -> Result<Option<Self>> {
    let query = query!(
      db,
      "SELECT * FROM registrations WHERE uuid = $1",
      uuid.to_string(),
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

