use twine_protocol::{prelude::*, twine_lib::ipld_core::codec::Codec};
use worker::{query, D1Database};
use twine_protocol::twine_lib::serde_ipld_dagjson::codec::DagJsonCodec;
use async_trait::async_trait;
use futures::stream::{unfold, Stream};
use futures::stream::{StreamExt, TryStreamExt};
use twine_protocol::twine_lib::as_cid::AsCid;
use twine_protocol::twine_lib::twine::{AnyTwine, TwineBlock};
use std::pin::Pin;
use std::sync::Arc;
use twine_protocol::twine_lib::errors::{ResolutionError, StoreError};
use twine_protocol::twine_lib::{twine::{Strand, Tixel}, Cid};
use twine_protocol::twine_lib::resolver::{unchecked_base, Resolver};
use twine_protocol::twine_lib::store::Store;
use twine_protocol::twine_lib::resolver::AbsoluteRange;

const BATCH_SIZE : u64 = 1000;

fn to_resolution_error(err: worker::Error) -> ResolutionError {
  match err {
    _ => ResolutionError::Fetch(err.to_string())
  }
}

fn to_storage_error(err: worker::Error) -> StoreError {
  StoreError::Saving(err.to_string())
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BlockRecord {
  #[serde(with = "serde_bytes")]
  cid: Vec<u8>,
  #[serde(with = "serde_bytes")]
  data: Vec<u8>,
}

impl BlockRecord {
  pub fn into_strand(self) -> Result<Strand, VerificationError> {
    let cid = Cid::try_from(self.cid).map_err(|e| VerificationError::General(e.to_string()))?;
    Strand::from_block(cid, self.data)
  }

  pub fn into_tixel(self) -> Result<Tixel, VerificationError> {
    let cid = Cid::try_from(self.cid).map_err(|e| VerificationError::General(e.to_string()))?;
    Tixel::from_block(cid, self.data)
  }
}

#[derive(Clone)]
pub struct D1Store {
  pub db: Arc<D1Database>,
}

impl D1Store {
  pub fn new(db: D1Database) -> Self {
    Self { db: Arc::new(db) }
  }

  async fn all_strands(&self) -> Result<Pin<Box<dyn Stream<Item = Result<Strand, ResolutionError>> + '_>>, ResolutionError> {
    async fn next_page(db: &D1Database, offset: i64) -> Result<Vec<Result<Strand, ResolutionError>>, ResolutionError> {
      let query_str = "SELECT cid, data FROM Strands LIMIT 100 OFFSET $1";
      let query = query!(db, query_str, offset).map_err(to_resolution_error)?;
      let result = query.all().await.map_err(to_resolution_error)?;
      let results = result.results::<BlockRecord>().map_err(to_resolution_error)?;
      let strands = results.into_iter().map(|block| {
        Ok(block.into_strand()?)
      });
      Ok(strands.collect())
    }

    let stream = unfold(0, move |offset| {
      async move {
        let strands = match next_page(&self.db, offset).await {
          Ok(strands) => strands,
          Err(e) => return Some((Err(e), offset)),
        };
        if strands.is_empty() {
          return None;
        }
        Some((Ok(strands), offset + 100))
      }
    })
    .map_ok(|v| futures::stream::iter(v.into_iter()))
    .try_flatten()
    .boxed_local();

    Ok(stream)
  }

  pub async fn get_strand(&self, cid: &Cid) -> Result<Strand, ResolutionError> {
    let query = query!(&self.db, "SELECT data FROM Strands WHERE cid = ?1", cid.to_bytes()).map_err(to_resolution_error)?;
    let result = query.first::<Vec<u8>>(Some("data")).await.map_err(to_resolution_error)?;
    let bytes = result.ok_or(ResolutionError::NotFound)?;
    let strand = Strand::from_block(*cid, bytes)?;
    Ok(strand)
  }

  pub async fn has_tixel(&self, cid: &Cid) -> Result<bool, ResolutionError> {
    let query = query!(
      &self.db,
      "SELECT TRUE FROM Tixels WHERE cid = ?1 LIMIT 1",
      cid.to_bytes()
    ).map_err(to_resolution_error)?;
    let result = query.first::<u8>(Some("TRUE")).await.map_err(to_resolution_error)?;
    Ok(result.is_some())
  }

  async fn has_strand_cid(&self, cid: &Cid) -> Result<bool, ResolutionError> {
    let query = query!(
      &self.db,
      "SELECT TRUE FROM Strands WHERE cid = ?1 LIMIT 1",
      cid.to_bytes()
    ).map_err(to_resolution_error)?;
    let result = query.first::<u8>(Some("TRUE")).await.map_err(to_resolution_error)?;
    Ok(result.is_some())
  }

  async fn cid_for_index(&self, strand: &Cid, index: u64) -> Result<Cid, ResolutionError> {
    let query = "SELECT t.cid FROM Tixels t JOIN Strands s ON t.strand = s.id WHERE s.cid = $1 AND t.idx = $2";

    let query = query!(&self.db, query, strand.to_bytes(), index as i64).map_err(to_resolution_error)?;
    let result: Option<Vec<u8>> = query.first(Some("cid")).await.map_err(to_resolution_error)?;
    let bytes = result.ok_or(ResolutionError::NotFound)?;

    Ok(Cid::try_from(bytes).map_err(|e| ResolutionError::Fetch(e.to_string()))?)
  }

  pub async fn get_tixel(&self, cid: &Cid) -> Result<Tixel, ResolutionError> {
    let query = query!(&self.db, "SELECT data FROM Tixels WHERE cid = ?1", cid.to_bytes()).map_err(to_resolution_error)?;
    let result = query.first::<Vec<u8>>(Some("data")).await.map_err(to_resolution_error)?;
    let bytes = result.ok_or(ResolutionError::NotFound)?;
    Ok(Tixel::from_block(*cid, bytes)?)
  }

  async fn get_tixel_by_index(&self, strand_cid: &Cid, index: u64) -> Result<Tixel, ResolutionError> {
    let query = query!(
      &self.db,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      AND Tixels.idx = ?2;",
      strand_cid.to_bytes(),
      index
    ).map_err(to_resolution_error)?;
    let result = query.first::<BlockRecord>(None).await.map_err(to_resolution_error)?;
    let block = result.ok_or(ResolutionError::NotFound)?;
    Ok(block.into_tixel()?)
  }

  async fn latest_tixel(&self, strand_cid: &Cid) -> Result<Tixel, ResolutionError> {
    let query = query!(
      &self.db,
      "SELECT Tixels.cid, Tixels.data
      FROM Tixels
      JOIN Strands ON Tixels.strand = Strands.id
      WHERE Strands.cid = ?1
      ORDER BY Tixels.idx DESC
      LIMIT 1;",
      strand_cid.to_bytes()
    ).map_err(to_resolution_error)?;
    let result = query.first::<BlockRecord>(None).await.map_err(to_resolution_error)?;
    let block = result.ok_or(ResolutionError::NotFound)?;
    Ok(block.into_tixel()?)
  }

  async fn save_strand(&self, strand: &Strand) -> Result<(), StoreError> {
    let query = query!(
      &self.db,
      "INSERT OR IGNORE INTO Strands (cid, data, spec, details)
      VALUES (?1, ?2, ?3, ?4);",
      strand.cid().to_bytes(),
      strand.bytes(),
      strand.spec_str(),
      String::from_utf8(DagJsonCodec::encode_to_vec(strand.details()).unwrap()).unwrap()
    ).map_err(to_storage_error)?;
    query.run().await.map_err(to_storage_error)?;
    log::info!("New strand saved: {}", strand.cid());
    Ok(())
  }

  async fn save_tixel(&self, tixel: &Tixel) -> Result<(), StoreError> {
    let query = "
      INSERT OR IGNORE INTO Tixels (cid, data, strand, idx)
      SELECT ?1, ?2, s.id, ?4
      FROM Strands s
      WHERE s.cid = ?3
        AND s.writable = 1
        AND (
          ?4 = 0 OR EXISTS (
            SELECT 1
            FROM Tixels t
            WHERE t.strand = s.id
              AND t.idx = ?4 - 1
              AND t.cid = ?5
          )
        );
    ";
    query!(
      &self.db,
      query,
      tixel.cid().to_bytes(),
      tixel.bytes(),
      tixel.strand_cid().to_bytes(),
      tixel.index() as i64,
      tixel.previous().map(|s| s.tixel.to_bytes())
    )
    .map_err(to_storage_error)?
    .run()
    .await
    .map_err(to_storage_error)?;
    log::debug!("Saved Tixel {}:{}", tixel.strand_cid(), tixel.cid());
    Ok(())
  }

  async fn remove_strand(&self, _cid: &Cid) -> Result<(), StoreError> {
    unimplemented!()
  }

  async fn remove_tixel_if_latest(&self, _cid: &Cid) -> Result<(), StoreError> {
    unimplemented!()
  }
}

#[async_trait(?Send)]
impl unchecked_base::BaseResolver for D1Store {

  async fn fetch_strands(&self) -> Result<Pin<Box<dyn Stream<Item = Result<Strand, ResolutionError>> + '_>>, ResolutionError> {
    self.all_strands().await
  }

  async fn has_strand(&self, cid: &Cid) -> Result<bool, ResolutionError> {
    self.has_strand_cid(cid).await
  }

  async fn has_index(&self, strand: &Cid, index: u64) -> Result<bool, ResolutionError> {
    self.cid_for_index(strand, index).await.map(|_| true).or_else(|e| {
      if let ResolutionError::NotFound = e {
        Ok(false)
      } else {
        Err(e)
      }
    })
  }

  async fn has_twine(&self, _strand: &Cid, cid: &Cid) -> Result<bool, ResolutionError> {
    self.has_tixel(cid).await
  }

  async fn fetch_strand(&self, strand: &Cid) -> Result<Strand, ResolutionError> {
    self.get_strand(strand).await
  }

  async fn fetch_tixel(&self, _strand: &Cid, tixel: &Cid) -> Result<Tixel, ResolutionError> {
    self.get_tixel(tixel).await
  }

  async fn fetch_index(&self, strand: &Cid, index: u64) -> Result<Tixel, ResolutionError> {
    self.get_tixel_by_index(strand, index).await
  }

  async fn fetch_latest(&self, strand: &Cid) -> Result<Tixel, ResolutionError> {
    self.latest_tixel(strand).await
  }

  async fn range_stream(&self, range: AbsoluteRange) -> Result<Pin<Box<dyn Stream<Item = Result<Tixel, ResolutionError>> + '_>>, ResolutionError> {
    let batches = range.batches(BATCH_SIZE);

    async fn get_batch(db: &D1Database, range: &AbsoluteRange) -> Result<Vec<Result<Tixel, ResolutionError>>, ResolutionError> {
      let dir = if range.is_increasing() { "ASC" } else { "DESC" };
      let query = query!(
        &db,
        &format!("
          SELECT t.cid, t.data
          FROM Tixels t JOIN Strands s ON t.strand = s.id
          WHERE s.cid = ?1 AND t.idx >= ?2 AND t.idx <= ?3
          ORDER BY t.idx {}
        ", dir),
        range.strand.to_bytes(),
        range.lower() as i64,
        range.upper() as i64
      ).map_err(to_resolution_error)?;

      let results = query.all().await.map_err(to_resolution_error)?;
      let tixels = results
        .results::<BlockRecord>()
        .map_err(to_resolution_error)?
        .into_iter()
        .map(|block| {
          let cid = Cid::try_from(block.cid).map_err(|e| ResolutionError::Fetch(e.to_string()))?;
          Ok(Tixel::from_block(cid, block.data)?)
        })
        .collect::<Vec<_>>();

      Ok(tixels)
    }

    let stream = unfold(batches.into_iter(), move |mut batches| {
      async move {
        let batch = batches.next()?;
        let tixels = match get_batch(&self.db, &batch).await {
          Ok(tixels) => tixels,
          Err(e) => return Some((Err(e), batches)),
        };
        if tixels.is_empty() {
          return None;
        }
        Some((Ok(tixels), batches))
      }
    })
    .map_ok(|v| futures::stream::iter(v.into_iter()))
    .try_flatten()
    .boxed_local();

    Ok(stream)
  }
}

impl Resolver for D1Store {}

#[async_trait(?Send)]
impl Store for D1Store {
  async fn save<T: Into<AnyTwine>>(&self, twine: T) -> Result<(), StoreError> {
    match twine.into() {
      AnyTwine::Tixel(t) => self.save_tixel(&t).await,
      AnyTwine::Strand(s) => self.save_strand(&s).await,
    }
  }

  async fn save_many<I: Into<AnyTwine>, S: Iterator<Item = I>, T: IntoIterator<Item = I, IntoIter = S>>(&self, twines: T) -> Result<(), StoreError> {
    for twine in twines {
      self.save(twine).await?;
    }
    Ok(())
  }

  async fn save_stream<I: Into<AnyTwine>, T: Stream<Item = I> + Unpin>(&self, twines: T) -> Result<(), StoreError> {
    twines
      .chunks(100)
      .then(|chunk| self.save_many(chunk))
      .try_for_each(|_| async { Ok(()) })
      .await?;
    Ok(())
  }

  async fn delete<C: AsCid>(&self, cid: C) -> Result<(), StoreError> {
    if self.has_strand_cid(cid.as_cid()).await? {
      self.remove_strand(cid.as_cid()).await
    } else if self.has_tixel(cid.as_cid()).await? {
      self.remove_tixel_if_latest(cid.as_cid()).await
    } else {
      Ok(())
    }
  }
}
