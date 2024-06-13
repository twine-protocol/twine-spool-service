use super::*;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use twine::twine_core::car::to_car_stream;
use futures::{stream::iter, StreamExt};

#[derive(Serialize)]
pub struct TwineWrapper {
  cid: Cid,
  data: AnyTwine,
}

impl From<AnyTwine> for TwineWrapper {
  fn from(twine: AnyTwine) -> Self {
    let cid = twine.cid().clone();
    TwineWrapper { cid, data: twine }
  }
}

#[derive(Serialize)]
pub struct ResponseData {
  #[serde(with = "crate::dag_json")]
  items: Vec<TwineWrapper>,
  #[serde(skip_serializing_if = "Option::is_none", with = "crate::dag_json")]
  strand: Option<Strand>,
}

impl ResponseData {
  pub fn from_collection<T: Into<AnyTwine>>(items: Vec<T>) -> Self {
    let items = items.into_iter().map(|t| TwineWrapper::from(t.into())).collect();
    ResponseData {
      items,
      strand: None,
    }
  }

  pub fn from_twine<T: Into<AnyTwine>>(twine: T) -> Self {
    let twine = twine.into();
    ResponseData {
      items: vec![twine.into()],
      strand: None,
    }
  }

  pub fn with_strand(&mut self, strand: Strand) -> &mut Self {
    self.strand = Some(strand);
    self
  }
}

#[derive(Debug, Deserialize)]
pub struct RequestQuery {
  full: Option<String>,
}

pub async fn car_response(items: Vec<AnyTwine>) -> Result<Response> {
  let carstream = to_car_stream(iter(items), vec![Cid::default()]);
  let car = carstream.concat().await;
  Response::from_bytes(car)
}

pub async fn twine_response(req: Request, result: Twine) -> Result<Response> {
  let q : RequestQuery = req.query()?;
  let accepts = req.headers().get("accept")?.unwrap_or_default();
  if accepts == "application/octet-stream" || accepts == "application/vnd.ipld.car" {
    let blocks: Vec<AnyTwine> = if q.full.is_some() {
      vec![result.strand().into(), result.tixel().into()]
    } else {
      vec![result.tixel().into()]
    };
    return car_response(blocks).await;
  }
  let strand = (*result.strand()).clone();
  let mut res = ResponseData::from_twine(result);
  if q.full.is_some() {
    res.with_strand(strand);
  }
  Response::from_json(&res)
}

pub async fn twine_collection_response(req: Request, results: Vec<Twine>) -> Result<Response> {
  let q : RequestQuery = req.query()?;
  let strand = if let Some(first) = results.first() {
    Some((*first.strand()).clone())
  } else {
    None
  };
  let accepts = req.headers().get("accept")?.unwrap_or_default();
  if accepts == "application/octet-stream" || accepts == "application/vnd.ipld.car" {
    let mut blocks: Vec<AnyTwine> = results.into_iter().map(|t| t.tixel().into()).collect();
    if q.full.is_some() {
      blocks.insert(0, strand.unwrap().into());
    }
    return car_response(blocks).await;
  }
  let mut res = ResponseData::from_collection(results);
  if q.full.is_some() && strand.is_some() {
    res.with_strand(strand.unwrap());
  }
  Response::from_json(&res)
}

pub async fn strand_collection_response(req: Request, results: Vec<Arc<Strand>>) -> Result<Response> {
  let accepts = req.headers().get("accept")?.unwrap_or_default();
  if accepts == "application/octet-stream" || accepts == "application/vnd.ipld.car" {
    let blocks: Vec<AnyTwine> = results.into_iter().map(|s| s.into()).collect();
    return car_response(blocks).await;
  }
  let res = ResponseData::from_collection(results);
  Response::from_json(&res)
}