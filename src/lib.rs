use std::str::FromStr;

use car::car_to_twines;
use futures::TryStreamExt;
use uuid::Uuid;
use worker::*;
use twine_protocol::{prelude::{unchecked_base::BaseResolver, *}, twine_lib::{self, ipld_core::ipld}};

mod errors;
use errors::*;
mod store;
mod d1_store;
mod formatting;
use formatting::*;
mod registration;
use registration::*;
mod dag_json;
mod car;

type Ctx = RouteContext<d1_store::D1Store>;

fn get_max_batch_size(env: &Env) -> u64 {
  env.var("MAX_BATCH_SIZE")
    .map_or(Ok(1000), |s| s.to_string().parse())
    .unwrap_or(1000)
}

fn as_multi_resolver(store: d1_store::D1Store) -> ResolverSetSeries<Box<dyn twine_lib::resolver::unchecked_base::BaseResolver>> {
  use twine_protocol::twine_http_store::{reqwest, v1::HttpStore};
  let cfg = twine_protocol::twine_http_store::v1::HttpStoreOptions::default()
    .url("https://random.colorado.edu/api");

  let mut resolver = ResolverSetSeries::default();
  resolver.add_boxed(HttpStore::new(reqwest::Client::new(), cfg));
  resolver.add_boxed(store);
  resolver
}

async fn list_strands(_req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;

  let strands = match store.strands().await {
    Ok(strands) => strands,
    Err(e) => return ApiError::from(e).to_response(),
  };
  match strands.try_collect().await {
    Ok(strands) => strand_collection_response(_req, strands).await,
    Err(e) => ApiError::from(e).to_response(),
  }
}

async fn exec_has(_req: Request, ctx: Ctx) -> Result<Response> {
  async fn handle(ctx: &Ctx, query: &str) -> std::result::Result<bool, ApiError> {
    let store = &ctx.data;
    let query = AnyQuery::from_str(query)?;
    match query {
      AnyQuery::One(q) => {
        Ok(store.has(q).await?)
      },
      AnyQuery::Strand(cid) => {
        Ok(store.has(cid).await?)
      },
      AnyQuery::Many(_) => Err(ApiError::InvalidQuery(
        "May only call HEAD for queries of a single item".to_string()
      )),
    }
  }

  match ctx.param("query") {
    Some(query) => {
      match handle(&ctx, query).await {
        Ok(res) => if res {
          Response::empty()
        } else {
          Response::empty().map(|r| r.with_status(404))
        },
        Err(e) => e.to_response(),
      }
    },
    None => Response::error("Missing query", 400),
  }
}

async fn exec_query(req: Request, ctx: Ctx) -> Result<Response> {
  async fn handle(ctx: &Ctx, query: &str) -> std::result::Result<QueryResult, ApiError> {
    let store = &ctx.data;
    let max_batch_size: u64 = get_max_batch_size(&ctx.env);
    let query = AnyQuery::from_str(query)?;
    match query {
      AnyQuery::One(q) => {
        let result = store.resolve(q).await?;
        Ok(QueryResult::Twine(result.unpack()))
      },
      AnyQuery::Strand(cid) => {
        let result = store.resolve_strand(cid).await?;
        Ok(QueryResult::Strand(result.unpack()))
      },
      AnyQuery::Many(range) => {
        let abs_range = match range.try_to_absolute(store).await? {
          Some(r) => r,
          None => return Ok(QueryResult::List(vec![])),
        };
        if abs_range.len() > max_batch_size {
          return Err(ApiError::BadRequestData(format!("Range size too large. Must be less than or equal to {}", max_batch_size)));
        }
        let result = store.resolve_range(range).await?.try_collect().await?;
        Ok(QueryResult::List(result))
      },
    }
  }
  match ctx.param("query") {
    Some(query) => {
      match handle(&ctx, query).await {
        Ok(res) => query_response(req, res).await,
        Err(e) => e.to_response(),
      }
    },
    None => Response::error("Missing query", 400),
  }
}

async fn put_strands(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  // check that the request is either content-type application/octet-stream or application/vnd.ipld.car
  let content_type = req.headers().get("content-type")?.unwrap_or_default();
  if content_type != "application/octet-stream" && content_type != "application/vnd.ipld.car" {
    return Response::error("Invalid content type", 400);
  }
  let bytes = req.bytes().await?;
  let strands: Vec<Strand> = match car_to_twines(bytes.as_ref()).await {
    Ok(strands) => strands,
    Err(e) => return e.to_response(),
  };

  use futures::stream::StreamExt;
  use futures::stream::TryStreamExt;
  match futures::stream::iter(strands)
    .inspect(|strand| console_log!("saving strand {}", strand.cid()))
    .map(|strand| async move {
      store.save(strand).await
    })
    .buffer_unordered(10)
    .try_collect::<Vec<()>>().await
  {
    Ok(_) => Response::ok("Put strands"),
    Err(e) => ApiError::from(e).to_response(),
  }
}

async fn put_tixels(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  // check that the request is either content-type application/octet-stream or application/vnd.ipld.car
  let content_type = req.headers().get("content-type")?.unwrap_or_default();
  if content_type != "application/octet-stream" && content_type != "application/vnd.ipld.car" {
    return Response::error("Invalid content type", 400);
  }
  let bytes = req.bytes().await?;
  // check to ensure the strand is allowed
  let strand_cid = match ctx.param("strand_cid") {
    Some(cid) => match cid.parse::<Cid>() {
      Ok(cid) => cid,
      Err(_) => return Response::error("Invalid strand cid", 400),
    },
    None => return Response::error("Missing strand cid", 400),
  };

  let strand = match store.resolve_strand(strand_cid).await {
    Ok(s) => s,
    Err(ResolutionError::NotFound) => return Response::error("Strand not yet authorized", 401),
    Err(e) => return ApiError::from(e).to_response(),
  };

  let tixels: Vec<Tixel> = match car_to_twines(bytes.as_ref()).await {
    Ok(tixels) => tixels,
    Err(e) => return e.to_response(),
  };

  let mut twines = match tixels.into_iter()
    .map(|t| Twine::try_new(strand.clone().unpack(), t))
    .collect::<std::result::Result<Vec<Twine>, _>>()
  {
    Ok(twines) => twines,
    Err(e) => return ApiError::from(e).to_response(),
  };

  // sort the twines by their index
  twines.sort_by(|a, b| a.index().cmp(&b.index()));

  match store.save_many(twines).await {
    Ok(_) => {},
    Err(e) => return ApiError::from(e).to_response(),
  };

  Response::ok("Put tixels")
}

async fn register_strand(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  let db = &store.db;
  let reg = match req.json::<RegistrationRequest>().await {
    Ok(reg) => reg,
    Err(e) => {
      console_debug!("{:?}", e);
      return Response::error(e.to_string(), 400);
    },
  };

  let strand = reg.strand.clone().unpack();

  // check if the strand is already registered
  if let Ok(true) = store.has_strand(&strand.cid()).await {
    return Response::error("Strand already registered", 409);
  }

  // check if we're accepting all
  if ctx.var("ACCEPT_ALL_STRANDS").map(|s| s.to_string()).unwrap_or("false".to_string()) == "true" {
    let record = RegistrationRecord::new_preapproved(reg.email, strand.cid(), strand.clone());
    record.save(db).await?;
    return match store.save(strand).await {
      Ok(_) => {
        let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
        Response::from_json(&record)
      },
      Err(e) => ApiError::from(e).to_response(),
    };
  }

  // check if it's preapproved
  if let Some(existing) = RegistrationRecord::check_approved(db, &strand).await? {
    // it's preapproved so we can save the strand
    match store.save(strand).await {
      Ok(_) => {
        let existing: RegistrationRecordJson = existing.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
        Response::from_json::<RegistrationRecordJson>(&existing)
      },
      Err(e) => ApiError::from(e).to_response(),
    }
  } else {
    let record: RegistrationRecord = reg.into();
    record.save(db).await?;
    let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
    Response::from_json::<RegistrationRecordJson>(&record)
  }
}

async fn check_registration(_req: Request, ctx: Ctx) -> Result<Response> {
  let uuid = match ctx.param("receipt_id") {
    Some(receipt_id) => match Uuid::try_parse(receipt_id) {
      Ok(uuid) => uuid,
      Err(_) => return Response::error("Invalid receipt_id", 400),
    },
    None => return Response::error("Missing receipt_id", 400),
  };

  let db = &ctx.data.db;
  match RegistrationRecord::fetch(db, uuid).await? {
    Some(record) => {
      let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
      Response::from_json(&record)
    },
    None => Response::error("Not found", 404),
  }
}

// only check POST and PUT requests
async fn check_auth(req: &Request, env: &Env) -> std::result::Result<(), ApiError> {
  if req.method() == Method::Get || req.method() == Method::Head {
    return Ok(());
  }
  if req.path() == "/register" {
    return Ok(());
  }
  let auth = req.headers().get("authorization")?.unwrap_or_default();
  if !auth.starts_with("ApiKey ") {
    return Err(ApiError::Unauthorized);
  }
  let api_key = auth.trim_start_matches("ApiKey ");
  let expected_key = env.var("API_KEY").map(|s| s.to_string()).unwrap_or("".to_string());
  if api_key != expected_key {
    return Err(ApiError::Unauthorized);
  }
  Ok(())
}

async fn get_v1(mut req: Request, ctx: Ctx) -> Result<Response> {
  // forward the request to v1 store
  let path = ctx.param("path").cloned().unwrap_or_else(|| "".to_string());
  let url = &format!("https://api.entwine.me/{}", path);
  let mut init = RequestInit::new();

  let mut headers = Headers::new();
  headers.append("accept", &req.headers().get("accept")?.unwrap_or_default())?;
  headers.append("content-type", &req.headers().get("content-type")?.unwrap_or_default())?;

  init
    .with_headers(headers);
  match req.method() {
    Method::Get | Method::Head => {
      init.with_method(req.method());
    },
    Method::Post | Method::Put => {
      init.with_method(req.method());
      init.with_body(Some(req.bytes().await?.into()));
    },
    _ => {
      return Response::error("Method not allowed", 405);
    },
  }
  let request = Request::new_with_init(url, &init)?;
  let response = Fetch::Request(request).send().await?;
  Ok(response)
}

#[event(fetch)]
async fn fetch(
  req: Request,
  env: Env,
  _ctx: Context,
) -> Result<Response> {
  console_error_panic_hook::set_once();

  // if let Err(e) = check_auth(&req, &env).await {
  //   return e.to_response();
  // }

  // let store = store::D1Store {
  //   db: env.d1("DB")?,
  //   max_batch_size: env.var("MAX_BATCH_SIZE").map_or(Ok(1000), |s| s.to_string().parse()).unwrap_or(1000),
  // };

  let store = d1_store::D1Store::new(env.d1("DB")?);

  let response = Router::with_data(store)
    .on_async("/v1", get_v1)
    .on_async("/v1/*path", get_v1)
    .get_async("/register", |_req, env| async move {
      let url: String = ["https://dummyurl.com/", &"register.html"].concat();
      env.env.assets("ASSETS")
        .expect("NEED ASSETS")
        .fetch(url, None).await
    })
    .post_async("/register", register_strand)
    .get_async("/register/:receipt_id", check_registration)
    .get_async("/", list_strands)
    .put_async("/", put_strands)
    .put_async("/:strand_cid", put_tixels)
    .get_async("/:query", exec_query)
    .head_async("/:query", exec_has)
    .head("/", |_, _| Response::empty())
    .run(req, env)
    .await;

  response.map(|mut r| {
    let _ = r.headers_mut()
      .set("X-Spool-Version", "2");
    r
  })
}

// #[event(scheduled)]
// pub async fn scheduled(_e: ScheduledEvent, env: Env, _: ScheduleContext) {
//   match handle_randomness_pulse(env).await {
//     Ok(t) => console_log!("Randomness pulse released: {}", t.cid()),
//     Err(e) => console_error!("Error handling randomness pulse: {:?}", e),
//   };
// }