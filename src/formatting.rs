use super::*;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

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

pub fn twine_response(req: Request, result: Twine) -> Result<Response> {
  let strand = (*result.strand()).clone();
  let mut res = ResponseData::from_twine(result);
  let q : RequestQuery = req.query()?;
  if q.full.is_some() {
    res.with_strand(strand);
  }
  Response::from_json(&res)
}

pub fn twine_collection_response(req: Request, results: Vec<Twine>) -> Result<Response> {
  let strand = if let Some(first) = results.first() {
    Some((*first.strand()).clone())
  } else {
    None
  };
  let mut res = ResponseData::from_collection(results);
  let q : RequestQuery = req.query()?;
  if q.full.is_some() && strand.is_some() {
    res.with_strand(strand.unwrap());
  }
  Response::from_json(&res)
}

pub fn strand_collection_response(_req: Request, results: Vec<Arc<Strand>>) -> Result<Response> {
  let res = ResponseData::from_collection(results);
  Response::from_json(&res)
}