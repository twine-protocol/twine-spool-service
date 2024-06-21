use futures::future::join;
use twine::{prelude::*, twine_core::ipld_core::codec::Codec};
use std::{str::FromStr, sync::Arc};
use worker::{query, D1Database};
use twine::twine_core::serde_ipld_dagjson::codec::DagJsonCodec;

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
}

impl ApiError {
  pub fn to_response(&self) -> Result<worker::Response, worker::Error> {
    match self {
      ApiError::ServerError(e) => worker::Response::error(e.to_string(), 500),
      ApiError::VerificationError(e) => worker::Response::error(e.to_string(), 400),
      ApiError::InvalidQuery(e) => worker::Response::error(e.to_string(), 400),
      ApiError::NotFound => worker::Response::error("Not found", 404),
      ApiError::Corrupted(e) => worker::Response::error(e.to_string(), 500),
      ApiError::BadRequestData(e) => worker::Response::error(e.to_string(), 400),
    }
  }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BlockRecord {
  #[serde(with = "serde_bytes")]
  cid: Vec<u8>,
  #[serde(with = "serde_bytes")]
  data: Vec<u8>,
}

impl BlockRecord {
  pub fn into_strand(self) -> Result<Strand, ApiError> {
    Ok(Strand::from_block(Cid::try_from(self.cid)?, self.data)?)
  }

  pub fn into_tixel(self) -> Result<Tixel, ApiError> {
    Ok(Tixel::from_block(Cid::try_from(self.cid)?, self.data)?)
  }
}

pub struct D1Store(pub D1Database);

impl D1Store {
  pub async fn get_strands(&self) -> Result<Vec<Arc<Strand>>, ApiError> {
    let query = query!(&self.0, "SELECT cid, data FROM Strands");
    let result = query.all().await?;
    let results = result.results::<BlockRecord>()?;
    let strands = results.into_iter().map(|block| {
      Ok(Arc::new(block.into_strand()?))
    });
    strands.collect()
  }

  pub async fn get_strand(&self, cid: &Cid) -> Result<Arc<Strand>, ApiError> {
    let query = query!(&self.0, "SELECT data FROM Strands WHERE cid = ?1", cid.to_bytes())?;
    let result = query.first::<Vec<u8>>(Some("data")).await?;
    let bytes = result.ok_or(ApiError::NotFound)?;
    let strand = Strand::from_block(*cid, bytes)?;
    Ok(Arc::new(strand))
  }

  pub async fn get_tixel(&self, cid: &Cid) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(&self.0, "SELECT data FROM Tixels WHERE cid = ?1", cid.to_bytes())?;
    let result = query.first::<Vec<u8>>(Some("data")).await?;
    let bytes = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(Tixel::from_block(*cid, bytes)?))
  }

  pub async fn get_by_cid(&self, cid: &Cid) -> Result<AnyTwine, ApiError> {
    // first try strands
    match self.get_strand(cid).await {
      Ok(strand) => return Ok(strand.into()),
      Err(ApiError::NotFound) => (),
      Err(e) => return Err(e),
    }
    let tixel = self.get_tixel(cid).await?;
    Ok(tixel.into())
  }

  pub async fn has_strand(&self, cid: &Cid) -> Result<bool, ApiError> {
    let query = query!(
      &self.0,
      "SELECT TRUE FROM Strands WHERE cid = ?1",
      cid.to_bytes()
    )?;
    let result = query.first::<u8>(Some("TRUE")).await?;
    Ok(result.is_some())
  }

  pub async fn has_tixel(&self, cid: &Cid) -> Result<bool, ApiError> {
    let query = query!(
      &self.0,
      "SELECT TRUE FROM Tixels WHERE cid = ?1",
      cid.to_bytes()
    )?;
    let result = query.first::<u8>(Some("TRUE")).await?;
    Ok(result.is_some())
  }

  pub async fn has_cid(&self, cid: &Cid) -> Result<bool, ApiError> {
    Ok(self.has_strand(cid).await? || self.has_tixel(cid).await?)
  }

  pub async fn has_index(&self, strand_cid: &Cid, index: u64) -> Result<bool, ApiError> {
    let query = query!(
      &self.0,
      "SELECT TRUE FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx = ?2;",
      strand_cid.to_bytes(),
      index
    )?;
    let result = query.first::<u8>(Some("TRUE")).await?;
    Ok(result.is_some())
  }

  pub async fn has(&self, query: &str) -> Result<bool, ApiError> {
    let q = Query::from_str(query)?;
    match q {
      Query::Stitch(stitch) => {
        Ok(self.has_tixel(&stitch.tixel).await?)
      },
      Query::Index(strand_cid, index) => {
        let index = if index >= 0 { index as u64 } else {
          let latest = match self.latest_index(&strand_cid).await {
            Ok(latest) => latest,
            Err(ApiError::NotFound) => return Ok(false),
            Err(e) => return Err(e),
          };
          latest + 1 + index as u64
        };
        Ok(self.has_index(&strand_cid, index).await?)
      },
      Query::Latest(strand_cid) => {
        match self.latest_index(&strand_cid).await {
          Ok(_) => Ok(true),
          Err(ApiError::NotFound) => return Ok(false),
          Err(e) => return Err(e),
        }
      },
    }
  }

  pub async fn get_by_index(&self, strand_cid: &Cid, index: u64) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx = ?2;",
      strand_cid.to_bytes(),
      index
    )?;
    let result = query.first::<BlockRecord>(None).await?;
    let block = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(block.into_tixel()?))
  }

  pub async fn latest_index(&self, strand_cid: &Cid) -> Result<u64, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.idx
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      ORDER BY Tixels.idx DESC
      LIMIT 1;",
      strand_cid.to_bytes()
    )?;
    let result = query.first::<u64>(Some("idx")).await?;
    let index = result.ok_or(ApiError::NotFound)?;
    Ok(index)
  }

  pub async fn latest(&self, strand_cid: &Cid) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      ORDER BY Tixels.idx DESC
      LIMIT 1;",
      strand_cid.to_bytes()
    )?;

    let result = query.first::<BlockRecord>(None).await?;
    let block = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(block.into_tixel()?))
  }

  pub async fn range_query(&self, query: &str) -> Result<Vec<Twine>, ApiError> {
    let q = RangeQuery::from_str(query)?;
    let latest = self.latest_index(q.strand_cid()).await?;
    let range = q.to_absolute(latest);

    if !self.has_index(q.strand_cid(), range.upper()).await? {
      return Err(ApiError::NotFound);
    }

    let increasing = range.is_increasing();
    let query = query!(
      &self.0,
      format!("SELECT Tixels.idx, Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx >= ?2
      AND Tixels.idx <= ?3
      ORDER BY Tixels.idx {};", if increasing { "ASC" } else { "DESC" }),
      q.strand_cid().to_bytes(),
      range.lower(),
      range.upper()
    )?;

    let (strand, result) = join(self.get_strand(q.strand_cid()), query.all()).await;
    let strand = strand?;
    let results = result?.results::<BlockRecord>()?;
    let twines = results.into_iter().map(|block| {
      let tixel = Arc::new(block.into_tixel()?);
      Ok(Twine::try_new_from_shared(strand.clone(), tixel)?)
    });

    twines.collect()
  }

  pub async fn twine_query(&self, query: &str) -> Result<Twine, ApiError> {
    let q = Query::from_str(query)?;
    match q {
      Query::Stitch(stitch) => {
        let (strand, tixel) = join(
          self.get_strand(&stitch.strand),
          self.get_tixel(&stitch.tixel),
        ).await;
        let strand = strand?;
        let tixel = tixel?;
        let twine = Twine::try_new_from_shared(strand, tixel)?;
        Ok(twine)
      },
      Query::Index(strand_cid, rel_index) => {
        let index = if rel_index < 0 {
          let latest = self.latest_index(&strand_cid).await?;
          latest + 1 + rel_index as u64
        } else {
          rel_index as u64
        };
        let (strand, tixel) = join(
          self.get_strand(&strand_cid),
          self.get_by_index(&strand_cid, index),
        ).await;
        let twine = Twine::try_new_from_shared(strand?, tixel?)?;
        Ok(twine)
      },
      Query::Latest(strand_cid) => {
        let (strand, tixel) = join(
          self.get_strand(&strand_cid),
          self.latest(&strand_cid),
        ).await;
        let twine = Twine::try_new_from_shared(strand?, tixel?)?;
        Ok(twine)
      },
    }
  }

  pub async fn put_strand(&self, strand: &Strand) -> Result<(), ApiError> {
    let query = query!(
      &self.0,
      "INSERT IGNORE INTO Strands (cid, data, spec, details)
      VALUES (?1, ?2, ?3, ?4);",
      strand.cid().to_bytes(),
      strand.bytes(),
      strand.spec_str(),
      String::from_utf8(DagJsonCodec::encode_to_vec(strand.details()).unwrap()).unwrap()
    )?;
    query.run().await?;
    Ok(())
  }

  pub async fn put_tixel(&self, tixel: &Tixel) -> Result<(), ApiError> {
    // we only insert the tixel if we have the previous index already
    let query = query!(
      &self.0,
      format!(
        "INSERT IGNORE INTO Tixels (cid, data, strand, idx)
        VALUES (?1, ?2, (SELECT id FROM Strands WHERE cid = ?3), ?4)
        {};",
        if tixel.index() == 0 { "" } else { "
          WHERE EXISTS(SELECT TRUE FROM Tixels WHERE strand = (SELECT id FROM Strands WHERE cid = ?3) AND idx = ?4 - 1)"
        }
      ),
      tixel.cid().to_bytes(),
      tixel.bytes(),
      tixel.strand_cid(),
      tixel.index()
    )?;
    query.run().await?;
    Ok(())
  }

  pub async fn put_many_twines<T: IntoIterator<Item = Twine>>(&self, strand_cid: &Cid, twines: T) -> Result<(), ApiError> {
    let statements = twines.into_iter()
    .scan(None, |prev, twine| {
      // This checks to see if the twine is contiguous with the previous one
      let cur_index = twine.index();
      let ret = match prev {
        Some(prev_index) => {
          if twine.index() == *prev_index + 1 {
            Some((false, twine))
          } else {
            Some((true, twine))
          }
        },
        None => {
          Some((cur_index != 0, twine))
        }
      };
      *prev = Some(cur_index);
      ret
    })
    .map(|(needs_where, t)| {
      let tixel = t.tixel();
      if tixel.strand_cid() != *strand_cid {
        return Err(ApiError::BadRequestData("Twine does not belong to specified strand".to_string()));
      }
      Ok(query!(
        &self.0,
        format!(
          "INSERT IGNORE INTO Tixels (cid, data, strand, idx)
          VALUES (?1, ?2, (SELECT id FROM Strands WHERE cid = ?3), ?4)
          {};",
          if needs_where {
            "WHERE EXISTS(SELECT TRUE FROM Tixels WHERE strand = (SELECT id FROM Strands WHERE cid = ?3) AND idx = ?4 - 1)"
          } else { "" }
        ),
        tixel.cid().to_bytes(),
        tixel.bytes(),
        strand_cid,
        tixel.index()
      )?)
    })
    .collect::<Result<Vec<_>, _>>()?;

    self.0.batch(statements).await?;

    Ok(())
  }

  pub async fn upcast(&self, tixel: Arc<Tixel>) -> Result<Twine, ApiError> {
    let strand = self.get_strand(&tixel.strand_cid()).await?;
    Ok(Twine::try_new_from_shared(strand, tixel)?)
  }
}
