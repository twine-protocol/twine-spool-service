use futures::future::join;
use twine::prelude::*;
use std::{str::FromStr, sync::Arc};
use worker::{query, D1Database};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
  #[error("Server error: {0}")]
  ServerError(#[from] worker::Error),
  #[error("Verification error: {0}")]
  VerificationError(#[from] VerificationError),
  #[error("Invalid query: {0}")]
  InvalidQuery(#[from] ConversionError),
  #[error("Not found")]
  NotFound,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BlockRecord {
  cid: Cid,
  data: Vec<u8>,
}

pub struct D1Store(pub D1Database);

impl D1Store {
  async fn get_strands(&self) -> Result<Vec<Arc<Strand>>, ApiError> {
    let query = query!(&self.0, "SELECT cid, data FROM Strands");
    let result = query.all().await?;
    let results = result.results::<BlockRecord>()?;
    let strands = results.into_iter().map(|block| {
      let strand = Strand::from_block(block.cid, block.data)?;
      Ok(Arc::new(strand))
    });
    strands.collect()
  }

  async fn get_strand(&self, cid: &Cid) -> Result<Arc<Strand>, ApiError> {
    let query = query!(&self.0, "SELECT data FROM Strands WHERE cid = ?1", cid)?;
    let result = query.first::<Vec<u8>>(Some("data")).await?;
    let bytes = result.ok_or(ApiError::NotFound)?;
    let strand = Strand::from_block(*cid, bytes)?;
    Ok(Arc::new(strand))
  }

  async fn get_tixel(&self, cid: &Cid) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(&self.0, "SELECT data FROM Tixels WHERE cid = ?1", cid)?;
    let result = query.first::<Vec<u8>>(Some("data")).await?;
    let bytes = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(Tixel::from_block(*cid, bytes)?))
  }

  async fn get_by_cid(&self, cid: &Cid) -> Result<AnyTwine, ApiError> {
    // first try strands
    match self.get_strand(cid).await {
      Ok(strand) => return Ok(strand.into()),
      Err(ApiError::NotFound) => (),
      Err(e) => return Err(e),
    }
    let tixel = self.get_tixel(cid).await?;
    Ok(tixel.into())
  }

  async fn get_by_index(&self, strand_cid: &Cid, index: u64) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx = ?2;",
      strand_cid,
      index
    )?;
    let result = query.first::<BlockRecord>(None).await?;
    let block = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(Tixel::from_block(block.cid, block.data)?))
  }

  async fn latest_index(&self, strand_cid: &Cid) -> Result<u64, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.idx
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      ORDER BY Tixels.idx DESC
      LIMIT 1;",
      strand_cid
    )?;
    let result = query.first::<u64>(Some("idx")).await?;
    let index = result.ok_or(ApiError::NotFound)?;
    Ok(index)
  }

  async fn latest(&self, strand_cid: &Cid) -> Result<Arc<Tixel>, ApiError> {
    let query = query!(
      &self.0,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      ORDER BY Tixels.idx DESC
      LIMIT 1;",
      strand_cid
    )?;

    let result = query.first::<BlockRecord>(None).await?;
    let block = result.ok_or(ApiError::NotFound)?;
    Ok(Arc::new(Tixel::from_block(block.cid, block.data)?))
  }

  async fn range_query(&self, query: &str) -> Result<Vec<Twine>, ApiError> {
    let q = RangeQuery::from_str(query)?;
    let latest = self.latest_index(q.strand_cid()).await?;
    let range = q.to_absolute(latest);
    let increasing = range.is_increasing();
    let query = query!(
      &self.0,
      "SELECT Tixels.idx, Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx >= ?2
      AND Tixels.idx <= ?3
      ORDER BY Tixels.idx ?4;",
      q.strand_cid(),
      range.lower(),
      range.upper(),
      if increasing { "ASC" } else { "DESC" }
    )?;

    let (strand, result) = join(self.get_strand(q.strand_cid()), query.all()).await;
    let strand = strand?;
    let results = result?.results::<BlockRecord>()?;
    let twines = results.into_iter().map(|block| {
      let tixel = Arc::new(Tixel::from_block(block.cid, block.data)?);
      Ok(Twine::try_new_from_shared(strand.clone(), tixel)?)
    });

    twines.collect()
  }

  async fn twine_query(&self, query: &str) -> Result<Twine, ApiError> {
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
}
